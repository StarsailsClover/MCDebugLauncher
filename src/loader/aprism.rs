// Aprism JE Native loader support (Alpha 7).
//
// Aprism (https://github.com/NDBlockConnect/Aprism) is a JE/BE native mod
// loader. On JE it attaches as a javaagent:
//   -javaagent:<jar>=aprismVersion=<v>;mcEdit=JE;mcVersion=<mc>;gameRoot=<dir>
//
// MDL detects the applicable Aprism artifact from GitHub Releases (same
// policy as Despotes: stable preferred, pre-release opt-in fallback),
// downloads + caches it, and appends the `-javaagent` flag at launch.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const APRISM_OWNER: &str = "NDBlockConnect";
pub const APRISM_REPO: &str = "Aprism";
pub const APRISM_JAR_PREFIX: &str = "Aprism-";

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    prerelease: bool,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
    digest: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AprismAsset {
    pub name: String,
    pub url: String,
    pub digest: Option<String>,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct AprismRelease {
    pub tag: String,
    pub prerelease: bool,
    pub assets: Vec<AprismAsset>,
}

/// Fetch Aprism releases (newest first).
pub async fn fetch_releases() -> Result<Vec<AprismRelease>> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases?per_page=30",
        APRISM_OWNER, APRISM_REPO
    );
    let response = crate::util::http::create_http_client()?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Failed to query Aprism releases")?;
    if !response.status().is_success() {
        anyhow::bail!("GitHub returned HTTP {} for Aprism releases", response.status());
    }
    let raw: Vec<GhRelease> = response.json().await.context("Failed to parse Aprism releases")?;
    Ok(raw
        .into_iter()
        .map(|r| AprismRelease {
            tag: r.tag_name,
            prerelease: r.prerelease,
            assets: r
                .assets
                .into_iter()
                .map(|a| AprismAsset {
                    name: a.name,
                    url: a.browser_download_url,
                    digest: a.digest,
                    size: a.size.unwrap_or(0),
                })
                .collect(),
        })
        .collect())
}

/// Asset names look like `Aprism-v26.0-Alpha.8-JE-26.2.jar`.
pub fn parse_asset_name(name: &str) -> Option<(String, String, String)> {
    let stem = name.strip_suffix(".jar")?;
    let rest = stem.strip_prefix(APRISM_JAR_PREFIX)?;
    // <tag>-JE-<mcver>  (tag may contain dashes)
    let (tag_and_edit, mc) = rest.rsplit_once('-')?;
    let (tag, edit) = tag_and_edit.rsplit_once('-')?;
    Some((tag.to_string(), edit.to_string(), mc.to_string()))
}

/// Pick the applicable JE artifact for a Minecraft version, honouring the
/// stable-first / pre-release-fallback policy.
pub fn select_release(
    releases: &[AprismRelease],
    mc_version: &str,
    allow_prerelease: bool,
) -> Option<(AprismRelease, AprismAsset)> {
    let applicable = |a: &AprismAsset| {
        parse_asset_name(&a.name)
            .map(|(tag, edit, mc)| edit == "JE" && mc == mc_version && !tag.is_empty())
            .unwrap_or(false)
    };
    for r in releases.iter().filter(|r| !r.prerelease) {
        if let Some(a) = r.assets.iter().find(|a| applicable(a)) {
            return Some((r.clone(), a.clone()));
        }
    }
    if allow_prerelease {
        for r in releases.iter().filter(|r| r.prerelease) {
            if let Some(a) = r.assets.iter().find(|a| applicable(a)) {
                return Some((r.clone(), a.clone()));
            }
        }
    }
    None
}

fn cache_dir() -> Result<PathBuf> {
    let dir = crate::util::paths::get_data_dir()?.join("aprism");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Download and cache an Aprism artifact.
pub async fn download_asset(asset: &AprismAsset) -> Result<PathBuf> {
    let dir = cache_dir()?;
    let dest = dir.join(&asset.name);
    if dest.exists() {
        return Ok(dest);
    }
    crate::version::downloader::download_file(&asset.url, &dest, None).await?;
    Ok(dest)
}

/// Build the `-javaagent` argument for launching with Aprism.
pub fn javaagent_arg(jar: &Path, aprism_version: &str, mc_version: &str, game_root: &Path) -> String {
    format!(
        "-javaagent:{}=aprismVersion={};mcEdit=JE;mcVersion={};gameRoot={}",
        jar.display(),
        aprism_version,
        mc_version,
        game_root.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_asset_name() {
        let (tag, edit, mc) = parse_asset_name("Aprism-v26.0-Alpha.8-JE-26.2.jar").unwrap();
        assert_eq!(tag, "v26.0-Alpha.8");
        assert_eq!(edit, "JE");
        assert_eq!(mc, "26.2");
        assert!(parse_asset_name("Despotes-v26.0-Alpha.2-fabric-1.21.1.jar").is_none());
    }

    #[test]
    fn test_javaagent_arg() {
        let arg = javaagent_arg(
            Path::new("/x/aprism.jar"),
            "v26.0-Alpha.8",
            "26.2",
            Path::new("/game"),
        );
        assert!(arg.starts_with("-javaagent:/x/aprism.jar=aprismVersion=v26.0-Alpha.8;mcEdit=JE;mcVersion=26.2;gameRoot=/game"));
    }
}
