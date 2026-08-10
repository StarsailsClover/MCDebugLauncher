// AprismPrismate support (Alpha 10).
//
// AprismPrismate (https://github.com/NDBlockConnect/AprismPrismate) is the
// mirror counterpart of AprismRefract: it is a loader-side bridge mod that
// runs INSIDE Fabric/NeoForge/Forge and lets those loaders load Aprism-native
// `.aje` packs. MDL installs it as an ordinary mod into `mods/`.
//
// Mutual exclusion (AprismPrismate FACT 2 / docs 01 §9.2): Prismate and the
// Aprism javaagent must NOT both be active in one instance. MDL therefore
// refuses to enable both simultaneously and surfaces a named error.
//
// Release layout:
// - Tags: `v26.0`, `v26.1-Alpha.7`, ... (stable + pre-releases).
// - Asset naming: `AprismPrismate-v<ver>-<loaderkey>-<mcver>.jar` with
//   loader keys Fa (Fabric) / N (NeoForge) / Fo (Forge).

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const PRISMATE_OWNER: &str = "NDBlockConnect";
pub const PRISMATE_REPO: &str = "AprismPrismate";
pub const PRISMATE_JAR_PREFIX: &str = "AprismPrismate-";

/// Prismate loader keys for an MDL instance loader.
pub fn prismate_key_for_loader(loader: &str) -> Option<&'static str> {
    match loader.to_ascii_lowercase().as_str() {
        "fabric" => Some("Fa"),
        "neoforge" => Some("N"),
        "forge" => Some("Fo"),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    prerelease: bool,
    html_url: String,
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
pub struct PrismateAsset {
    pub name: String,
    pub url: String,
    pub digest: Option<String>,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct PrismateRelease {
    pub tag: String,
    pub prerelease: bool,
    pub html_url: String,
    pub assets: Vec<PrismateAsset>,
}

/// Fetch AprismPrismate releases (newest first).
pub async fn fetch_releases() -> Result<Vec<PrismateRelease>> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases?per_page=50",
        PRISMATE_OWNER, PRISMATE_REPO
    );
    let response = crate::util::http::create_http_client()?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Failed to query AprismPrismate releases")?;
    if !response.status().is_success() {
        anyhow::bail!(
            "GitHub returned HTTP {} for AprismPrismate releases",
            response.status()
        );
    }
    let raw: Vec<GhRelease> = response.json().await.context("Failed to parse AprismPrismate releases")?;
    Ok(raw
        .into_iter()
        .map(|r| PrismateRelease {
            tag: r.tag_name,
            prerelease: r.prerelease,
            html_url: r.html_url,
            assets: r
                .assets
                .into_iter()
                .map(|a| PrismateAsset {
                    name: a.name,
                    url: a.browser_download_url,
                    digest: a.digest,
                    size: a.size.unwrap_or(0),
                })
                .collect(),
        })
        .collect())
}

/// Parsed structure of a Prismate artifact filename:
/// `AprismPrismate-v<ver>-<loaderkey>-<mcver>.jar`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPrismateAsset {
    /// AprismPrismate version tag, e.g. "v26.1".
    pub tag: String,
    /// Loader key: Fa / N / Fo.
    pub loader_key: String,
    /// Target Minecraft version, e.g. "26.2".
    pub mc_version: String,
}

pub fn parse_asset_name(name: &str) -> Option<ParsedPrismateAsset> {
    let stem = name.strip_suffix(".jar")?;
    let rest = stem.strip_prefix(PRISMATE_JAR_PREFIX)?;
    // <tag>-<loaderkey>-<mcver> — split from the right.
    let (tag_and_key, mc_version) = rest.rsplit_once('-')?;
    let (tag, loader_key) = tag_and_key.rsplit_once('-')?;
    if !["Fa", "N", "Fo"].contains(&loader_key) {
        return None;
    }
    Some(ParsedPrismateAsset {
        tag: tag.to_string(),
        loader_key: loader_key.to_string(),
        mc_version: mc_version.to_string(),
    })
}

/// Pick the best Prismate jar for a loader + MC version (stable-first policy).
pub fn select_release(
    releases: &[PrismateRelease],
    loader_key: &str,
    mc_version: &str,
    allow_prerelease: bool,
) -> Option<(PrismateRelease, PrismateAsset)> {
    let applicable = |a: &PrismateAsset| {
        parse_asset_name(&a.name)
            .map(|p| p.loader_key == loader_key && p.mc_version == mc_version)
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
    let dir = crate::util::paths::get_data_dir()?.join("prismate");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Download a Prismate jar into the cache (reusing matching files).
pub async fn download_asset(asset: &PrismateAsset) -> Result<PathBuf> {
    let dir = cache_dir()?;
    let dest = dir.join(&asset.name);
    if dest.exists() {
        return Ok(dest);
    }
    tracing::info!("Downloading {} ...", asset.name);
    let client = crate::util::http::create_http_client()?;
    let response = client
        .get(&asset.url)
        .header("Accept", "application/octet-stream")
        .send()
        .await
        .with_context(|| format!("Failed to download {}", asset.url))?;
    if !response.status().is_success() {
        anyhow::bail!("HTTP error {} downloading {}", response.status(), asset.url);
    }
    let bytes = response.bytes().await.context("Failed to read download body")?;
    tokio::fs::write(&dest, &bytes)
        .await
        .with_context(|| format!("Failed to write {}", dest.display()))?;
    Ok(dest)
}

/// Install a cached Prismate jar into the instance's `mods/` directory.
/// Returns the installed filename.
pub async fn install_into(instance_dir: &Path, source: &Path) -> Result<String> {
    let mods_dir = instance_dir.join("mods");
    tokio::fs::create_dir_all(&mods_dir).await?;
    // Replace any previously installed Prismate build.
    for old in installed_prismate(instance_dir) {
        if old.file_name() != source.file_name() {
            let _ = tokio::fs::remove_file(&old).await;
        }
    }
    let dest = mods_dir.join(
        source.file_name().context("Invalid Prismate jar path")?,
    );
    tokio::fs::copy(source, &dest)
        .await
        .with_context(|| format!("Failed to copy Prismate into {}", mods_dir.display()))?;
    let name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string();
    tracing::info!("Installed AprismPrismate: {}", name);
    Ok(name)
}

/// List Prismate jars installed in the instance's mods dir.
pub fn installed_prismate(instance_dir: &Path) -> Vec<PathBuf> {
    let mods_dir = instance_dir.join("mods");
    let entries = match std::fs::read_dir(&mods_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("jar")
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.starts_with(PRISMATE_JAR_PREFIX))
                    .unwrap_or(false)
        })
        .collect()
}

/// Whether a Prismate jar is installed in the instance.
pub fn is_installed(instance_dir: &Path) -> bool {
    !installed_prismate(instance_dir).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_real_asset_names() {
        let p = parse_asset_name("AprismPrismate-v26.1-Fa-26.2.jar").unwrap();
        assert_eq!(p.tag, "v26.1");
        assert_eq!(p.loader_key, "Fa");
        assert_eq!(p.mc_version, "26.2");

        let p = parse_asset_name("AprismPrismate-v26.1-N-26.2.jar").unwrap();
        assert_eq!(p.loader_key, "N");

        // Signatures/checksums are not Prismate artifacts.
        assert!(parse_asset_name("AprismPrismate-v26.1-Fa-26.2.jar.sig").is_none());
        assert!(parse_asset_name("checksums.txt").is_none());
    }

    #[test]
    fn test_loader_key_mapping() {
        assert_eq!(prismate_key_for_loader("fabric"), Some("Fa"));
        assert_eq!(prismate_key_for_loader("neoforge"), Some("N"));
        assert_eq!(prismate_key_for_loader("quilt"), None);
    }

    #[test]
    fn test_select_release_stable_first() {
        let asset = PrismateAsset {
            name: "AprismPrismate-v26.1-Fa-26.2.jar".into(),
            url: "u".into(),
            digest: None,
            size: 1,
        };
        let stable = PrismateRelease {
            tag: "v26.1".into(),
            prerelease: false,
            html_url: String::new(),
            assets: vec![asset.clone()],
        };
        let pre = PrismateRelease {
            tag: "v26.2-Alpha.1".into(),
            prerelease: true,
            html_url: String::new(),
            assets: vec![asset],
        };
        let (r, _) = select_release(&[pre.clone(), stable.clone()], "Fa", "26.2", false).unwrap();
        assert_eq!(r.tag, "v26.1");
        assert!(select_release(&[pre.clone()], "Fa", "26.2", false).is_none());
        let (r, _) = select_release(&[pre], "Fa", "26.2", true).unwrap();
        assert_eq!(r.tag, "v26.2-Alpha.1");
    }
}
