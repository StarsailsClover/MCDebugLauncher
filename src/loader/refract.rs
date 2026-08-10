// AprismRefract support (Alpha 10).
//
// AprismRefract (https://github.com/NDBlockConnect/AprismRefract) ships
// loader-support extensions (*.aep) that teach the Aprism native loader how
// to run Fabric/NeoForge/Forge/Quilt/LiteLoader mods. Extensions live in the
// instance's `aprism-extensions/` directory; the Aprism runtime scans that
// directory itself at boot (Aprism FACT 9.14), so MDL only needs to detect,
// download and place the right artifact.
//
// Release layout:
// - Tags are loader-prefixed: `fabric/v26.0-Alpha.2`, `neoforge/v26.0-Alpha.1`...
// - Asset naming: `<Loader>-Support-A<aprismRange>-<Key><loaderRange>-JE-<mc>.aep`
//   where the range brackets have been sanitized to dots, e.g.
//   `Fabric-Support-A.26.0.27.0.-Fa.0.16.0.17.-JE-26.2.aep`.
//
// Selection policy mirrors Despotes: stable releases first, pre-releases only
// when explicitly allowed, fall back to the newest applicable pre-release when
// no stable artifact applies.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const REFRACT_OWNER: &str = "NDBlockConnect";
pub const REFRACT_REPO: &str = "AprismRefract";

/// Loader keys used inside Aprism extension artifact names (Aprism FACT 9.14).
pub fn refract_key_for_loader(loader: &str) -> Option<&'static str> {
    match loader.to_ascii_lowercase().as_str() {
        "fabric" => Some("Fa"),
        "forge" => Some("Fo"),
        "neoforge" => Some("N"),
        "quilt" => Some("Q"),
        "liteloader" => Some("L"),
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
pub struct RefractAsset {
    pub name: String,
    pub url: String,
    pub digest: Option<String>,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct RefractRelease {
    pub tag: String,
    pub prerelease: bool,
    pub html_url: String,
    pub assets: Vec<RefractAsset>,
}

/// Fetch AprismRefract releases (newest first).
pub async fn fetch_releases() -> Result<Vec<RefractRelease>> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases?per_page=50",
        REFRACT_OWNER, REFRACT_REPO
    );
    let response = crate::util::http::create_http_client()?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Failed to query AprismRefract releases")?;
    if !response.status().is_success() {
        anyhow::bail!(
            "GitHub returned HTTP {} for AprismRefract releases",
            response.status()
        );
    }
    let raw: Vec<GhRelease> = response.json().await.context("Failed to parse AprismRefract releases")?;
    Ok(raw
        .into_iter()
        .map(|r| RefractRelease {
            tag: r.tag_name,
            prerelease: r.prerelease,
            html_url: r.html_url,
            assets: r
                .assets
                .into_iter()
                .map(|a| RefractAsset {
                    name: a.name,
                    url: a.browser_download_url,
                    digest: a.digest,
                    size: a.size.unwrap_or(0),
                })
                .collect(),
        })
        .collect())
}

/// Parsed structure of an AprismRefract artifact filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRefractAsset {
    /// Loader key: Fa / Fo / N / Q / L.
    pub loader_key: String,
    /// Sanitized Aprism version range, e.g. `.26.0.27.0.`.
    pub aprism_range: String,
    /// Sanitized loader version range, e.g. `.0.16.0.17.`.
    pub loader_range: String,
    /// MC edition segment (currently always "JE").
    pub edit: String,
    /// Target Minecraft version, e.g. "26.2".
    pub mc_version: String,
}

/// Parse `<Loader>-Support-A<aprismRange>-<Key><loaderRange>-<Edit>-<mc>.aep`.
///
/// Example: `Fabric-Support-A.26.0.27.0.-Fa.0.16.0.17.-JE-26.2.aep`
/// -> key=Fa, aprism_range=.26.0.27.0., loader_range=.0.16.0.17., mc=26.2
pub fn parse_asset_name(name: &str) -> Option<ParsedRefractAsset> {
    let stem = name.strip_suffix(".aep")?;
    // Split edition + mc from the right: <...>-JE-26.2
    let (rest, mc_version) = stem.rsplit_once('-')?; // (.., "26.2")
    let (rest, edit) = rest.rsplit_once('-')?; // (.., "JE")
    // rest = "<Loader>-Support-A.26.0.27.0.-Fa.0.16.0.17."
    let (rest, key_and_range) = rest.rsplit_once('-')?; // (.., "Fa.0.16.0.17.")
    let (purpose, aprism_part) = rest.rsplit_once('-')?; // ("Fabric-Support", "A.26.0.27.0.")
    if !purpose.ends_with("-Support") && purpose != "Support" {
        return None;
    }
    let aprism_range = aprism_part.strip_prefix('A')?;
    // key_and_range = "Fa.0.16.0.17." — key is one of Fa/Fo/N/L/Q.
    let (loader_key, loader_range) = split_key_and_range(key_and_range)?;
    Some(ParsedRefractAsset {
        loader_key: loader_key.to_string(),
        aprism_range: aprism_range.to_string(),
        loader_range: loader_range.to_string(),
        edit: edit.to_string(),
        mc_version: mc_version.to_string(),
    })
}

fn split_key_and_range(s: &str) -> Option<(&'static str, &str)> {
    for key in ["Fa", "Fo", "N", "L", "Q"] {
        if let Some(range) = s.strip_prefix(key) {
            return Some((key, range));
        }
    }
    None
}

/// Pick the best `.aep` for a loader + MC version under the stable-first policy.
pub fn select_release(
    releases: &[RefractRelease],
    loader_key: &str,
    mc_version: &str,
    allow_prerelease: bool,
) -> Option<(RefractRelease, RefractAsset)> {
    let applicable = |a: &RefractAsset| {
        parse_asset_name(&a.name)
            .map(|p| {
                p.loader_key == loader_key
                    && p.edit == "JE"
                    && (p.mc_version == mc_version || p.mc_version.is_empty())
            })
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

/// List every applicable (release, asset) pair (stable first) for selection.
pub fn list_applicable(
    releases: &[RefractRelease],
    loader_key: &str,
    mc_version: &str,
    allow_prerelease: bool,
) -> Vec<(RefractRelease, RefractAsset)> {
    let mut out = Vec::new();
    let applicable = |a: &RefractAsset| {
        parse_asset_name(&a.name)
            .map(|p| p.loader_key == loader_key && p.edit == "JE" && p.mc_version == mc_version)
            .unwrap_or(false)
    };
    for r in releases.iter().filter(|r| !r.prerelease) {
        for a in r.assets.iter().filter(|a| applicable(a)) {
            out.push((r.clone(), a.clone()));
        }
    }
    if allow_prerelease {
        for r in releases.iter().filter(|r| r.prerelease) {
            for a in r.assets.iter().filter(|a| applicable(a)) {
                out.push((r.clone(), a.clone()));
            }
        }
    }
    out
}

fn cache_dir() -> Result<PathBuf> {
    let dir = crate::util::paths::get_data_dir()?.join("refract");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Download an `.aep` into the cache (reusing matching files).
pub async fn download_asset(asset: &RefractAsset) -> Result<PathBuf> {
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

/// Install a cached `.aep` into the instance's `aprism-extensions/` directory
/// (the location the Aprism runtime scans at boot). Returns the file name.
pub async fn install_into(instance_dir: &Path, source: &Path) -> Result<String> {
    let ext_dir = instance_dir.join("aprism-extensions");
    tokio::fs::create_dir_all(&ext_dir).await?;
    let dest = ext_dir.join(
        source
            .file_name()
            .context("Invalid AprismRefract artifact path")?,
    );
    tokio::fs::copy(source, &dest)
        .await
        .with_context(|| format!("Failed to copy .aep into {}", ext_dir.display()))?;
    let name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string();
    tracing::info!("Installed AprismRefract extension: {}", name);
    Ok(name)
}

/// List `.aep` extensions installed in the instance.
pub fn installed_extensions(instance_dir: &Path) -> Vec<PathBuf> {
    let ext_dir = instance_dir.join("aprism-extensions");
    let entries = match std::fs::read_dir(&ext_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("aep"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_real_asset_names() {
        let p = parse_asset_name("Fabric-Support-A.26.0.27.0.-Fa.0.16.0.17.-JE-26.2.aep").unwrap();
        assert_eq!(p.loader_key, "Fa");
        assert_eq!(p.aprism_range, ".26.0.27.0.");
        assert_eq!(p.loader_range, ".0.16.0.17.");
        assert_eq!(p.edit, "JE");
        assert_eq!(p.mc_version, "26.2");

        let p = parse_asset_name("Forge-Support-A.26.0.27.0.-Fo.54.0.55.0.-JE-26.2.aep").unwrap();
        assert_eq!(p.loader_key, "Fo");

        let p = parse_asset_name("Quilt-Support-A.26.0.27.0.-Q.0.29.0.30.-JE-26.2.aep").unwrap();
        assert_eq!(p.loader_key, "Q");

        // Non-extension files are rejected.
        assert!(parse_asset_name("checksums.txt").is_none());
        assert!(parse_asset_name("AprismPrismate-v26.1-Fa-26.2.jar").is_none());
    }

    #[test]
    fn test_loader_key_mapping() {
        assert_eq!(refract_key_for_loader("fabric"), Some("Fa"));
        assert_eq!(refract_key_for_loader("NEOFORGE"), Some("N"));
        assert_eq!(refract_key_for_loader("vanilla"), None);
    }

    #[test]
    fn test_select_release_prefers_stable() {
        let stable = RefractRelease {
            tag: "fabric/v26.1".into(),
            prerelease: false,
            html_url: String::new(),
            assets: vec![RefractAsset {
                name: "Fabric-Support-A.26.0.27.0.-Fa.0.16.0.17.-JE-26.2.aep".into(),
                url: "u".into(),
                digest: None,
                size: 1,
            }],
        };
        let pre = RefractRelease {
            tag: "fabric/v26.2-Alpha.1".into(),
            prerelease: true,
            html_url: String::new(),
            assets: stable.assets.clone(),
        };
        // Stable wins when present.
        let (r, _) = select_release(&[pre.clone(), stable.clone()], "Fa", "26.2", false).unwrap();
        assert_eq!(r.tag, "fabric/v26.1");
        // Pre-release only when allowed.
        assert!(select_release(&[pre.clone()], "Fa", "26.2", false).is_none());
        let (r, _) = select_release(&[pre], "Fa", "26.2", true).unwrap();
        assert_eq!(r.tag, "fabric/v26.2-Alpha.1");
    }
}
