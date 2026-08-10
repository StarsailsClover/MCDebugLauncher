// Despotes release detection, download and installation.
//
// Despotes (https://github.com/NDBlockConnect/Despotes) is the in-process
// game-control mod that replaces MDL's old bundled companion. MDL does not
// ship a Despotes JAR; it detects the best release on demand and downloads
// it into a local cache, then installs a copy into the instance's mods/.
//
// Release selection policy (requested behaviour):
// 1. Prefer the latest non-prerelease ("Latest Release") that has an asset
//    applicable to the instance (matching loader + Minecraft version).
// 2. Pre-releases are only used when explicitly requested.
// 3. When no applicable non-prerelease asset exists, fall back to the
//    applicable asset of the latest pre-release (this is what happens
//    today: the v26.0 line only ships Pre-Releases so far).
//
// Asset naming convention: `Despotes-<tag>-<loader>-<mcversion>.jar`,
// e.g. `Despotes-v26.0-Alpha.2-fabric-1.21.1.jar`.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const DESPOTES_OWNER: &str = "NDBlockConnect";
pub const DESPOTES_REPO: &str = "Despotes";
pub const DESPOTES_JAR_PREFIX: &str = "Despotes-";

/// One downloadable artifact of a Despotes release.
#[derive(Debug, Clone)]
pub struct DespotesAsset {
    pub name: String,
    pub url: String,
    /// `sha256:<hex>` digest advertised by the GitHub API, if present.
    pub digest: Option<String>,
    pub size: u64,
}

/// A Despotes release (stable or pre-release) with its assets.
#[derive(Debug, Clone)]
pub struct DespotesRelease {
    pub tag: String,
    pub prerelease: bool,
    pub html_url: String,
    pub published_at: String,
    pub assets: Vec<DespotesAsset>,
}

/// Loader/branch name used inside Despotes asset file names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DespotesLoader {
    Fabric,
    NeoForge,
    Forge,
    Native,
    Aprism,
}

impl DespotesLoader {
    pub fn slug(&self) -> &'static str {
        match self {
            DespotesLoader::Fabric => "fabric",
            DespotesLoader::NeoForge => "neoforge",
            DespotesLoader::Forge => "forge",
            DespotesLoader::Native => "native",
            DespotesLoader::Aprism => "aprism",
        }
    }
}

/// The loader the instance actually uses, mapped onto a Despotes loader slug.
///
/// Vanilla instances (loader == None, "none" or "vanilla") map to the
/// `native` Despotes branch, which attaches as a `-javaagent` instead of a
/// mods/ jar. Returns `None` only for loaders Despotes does not support
/// (e.g. quilt without the Fabric branch, optifine-only instances).
pub fn despotes_loader_for(instance_loader: Option<&str>) -> Option<DespotesLoader> {
    match instance_loader.map(|s| s.to_ascii_lowercase()) {
        None => Some(DespotesLoader::Native),
        Some(l) if l == "fabric" => Some(DespotesLoader::Fabric),
        Some(l) if l == "neoforge" => Some(DespotesLoader::NeoForge),
        Some(l) if l == "forge" => Some(DespotesLoader::Forge),
        Some(l) if l == "native" || l == "vanilla" || l == "none" => Some(DespotesLoader::Native),
        Some(l) if l == "aprism" => Some(DespotesLoader::Aprism),
        _ => None,
    }
}

/// Whether a Despotes build attaches as a JVM `-javaagent` (the `native`
/// branch) instead of being dropped into the instance's mods/ directory.
pub fn is_javaagent_variant(loader_slug: &str) -> bool {
    loader_slug == DespotesLoader::Native.slug()
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    prerelease: bool,
    html_url: String,
    published_at: Option<String>,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
    digest: Option<String>,
}

/// Fetch all releases of the Despotes repository, newest first.
pub async fn fetch_releases() -> Result<Vec<DespotesRelease>> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases?per_page=30",
        DESPOTES_OWNER, DESPOTES_REPO
    );

    let client = crate::util::http::create_http_client()?;
    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Failed to query Despotes releases from GitHub")?;

    if !response.status().is_success() {
        anyhow::bail!(
            "GitHub releases API returned HTTP {} for Despotes",
            response.status()
        );
    }

    let raw: Vec<GhRelease> = response
        .json()
        .await
        .context("Failed to parse Despotes releases JSON")?;

    Ok(raw
        .into_iter()
        .map(|r| DespotesRelease {
            tag: r.tag_name,
            prerelease: r.prerelease,
            html_url: r.html_url,
            published_at: r.published_at.unwrap_or_default(),
            assets: r
                .assets
                .into_iter()
                .map(|a| DespotesAsset {
                    name: a.name,
                    url: a.browser_download_url,
                    digest: a.digest,
                    size: a.size.unwrap_or(0),
                })
                .collect(),
        })
        .collect())
}

/// Parsed structure of a Despotes asset filename:
/// `Despotes-<tag>-<loader>-<mcversion>.jar`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAsset {
    pub tag: String,
    pub loader: String,
    pub mc_version: String,
}

/// Parse an asset filename into its components. Returns `None` when the
/// name does not follow the `Despotes-<tag>-<loader>-<mc>.jar` convention
/// (the Aprism variant ships `.aje` and is accepted as well).
pub fn parse_asset_name(name: &str) -> Option<ParsedAsset> {
    let stem = name
        .strip_suffix(".jar")
        .or_else(|| name.strip_suffix(".aje"))?;
    let rest = stem.strip_prefix(DESPOTES_JAR_PREFIX)?;
    // tag may itself contain dashes (v26.0-Alpha.2), so split from the end:
    // <...>-<loader>-<mcversion>
    let (tag_and_loader, mc_version) = rest.rsplit_once('-')?;
    let (tag, loader) = tag_and_loader.rsplit_once('-')?;
    Some(ParsedAsset {
        tag: tag.to_string(),
        loader: loader.to_string(),
        mc_version: mc_version.to_string(),
    })
}

/// Whether a Despotes build covers the given Minecraft version.
///
/// Rules derived from the v26.0 compatibility table:
/// - `fabric-1.21.1.jar` covers Minecraft 1.20.0 ..= 1.21.11
/// - `fabric-1.20.1.jar` covers Minecraft 1.20.0 ..= 1.20.6
/// - `fabric-26.2.jar`   covers Minecraft 26.1 ..= 26.2 (new year-based ids)
/// For anything not covered by these known ranges we fall back to exact
/// version equality, which is the safest default.
pub fn asset_covers_mc(mc_requested: &str, asset_mc: &str, loader_slug: &str) -> bool {
    if mc_requested == asset_mc {
        return true;
    }
    // Year-based Minecraft versions (26.x) only match their exact id.
    if is_year_version(mc_requested) || is_year_version(asset_mc) {
        return false;
    }
    if loader_slug == "fabric" {
        let req = parse_mc(mc_requested);
        let asset = parse_mc(asset_mc);
        if let (Some(req), Some(asset)) = (req, asset) {
            // The 1.21.1 artifact is the "modern" remapped build covering
            // 1.20-1.21.11; the 1.20.1 artifact is the legacy build for
            // 1.20-1.20.6.
            if asset == (1, 21, 1) {
                return req.0 == 1 && req.1 >= 20 && req.1 <= 21 && req.2 <= 11;
            }
            if asset == (1, 20, 1) {
                return req.0 == 1 && req.1 == 20 && req.2 <= 6;
            }
        }
    }
    false
}

fn is_year_version(v: &str) -> bool {
    v.split('.').next().map(|n| n.parse::<u32>().unwrap_or(0) >= 24) == Some(true)
}

fn parse_mc(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().map(|p| p.parse().unwrap_or(0)).unwrap_or(0);
    Some((major, minor, patch))
}

/// Pick the single best asset for an instance from the full release list.
///
/// Implements the requested policy:
/// - prefer newest stable release with an applicable asset;
/// - if `allow_prerelease` is true and no stable asset applies, use the
///   newest pre-release with an applicable asset;
/// - if no applicable asset exists at all, return `None`.
pub fn select_release(
    releases: &[DespotesRelease],
    loader_slug: &str,
    mc_version: &str,
    allow_prerelease: bool,
) -> Option<(DespotesRelease, DespotesAsset)> {
    // GitHub returns releases newest-first; rely on that ordering.
    for release in releases.iter().filter(|r| !r.prerelease) {
        if let Some(asset) = applicable_asset(release, loader_slug, mc_version) {
            return Some((release.clone(), asset.clone()));
        }
    }
    if allow_prerelease {
        for release in releases.iter().filter(|r| r.prerelease) {
            if let Some(asset) = applicable_asset(release, loader_slug, mc_version) {
                return Some((release.clone(), asset.clone()));
            }
        }
    }
    None
}

/// List every applicable (release, asset) pair for interactive selection,
/// honouring the same stable-first policy: stable releases first (newest
/// first), then pre-releases when allowed.
pub fn list_applicable(
    releases: &[DespotesRelease],
    loader_slug: &str,
    mc_version: &str,
    allow_prerelease: bool,
) -> Vec<(DespotesRelease, DespotesAsset)> {
    let mut out = Vec::new();
    for release in releases.iter().filter(|r| !r.prerelease) {
        for asset in &release.assets {
            if is_applicable(asset, loader_slug, mc_version) {
                out.push((release.clone(), asset.clone()));
            }
        }
    }
    if allow_prerelease {
        for release in releases.iter().filter(|r| r.prerelease) {
            for asset in &release.assets {
                if is_applicable(asset, loader_slug, mc_version) {
                    out.push((release.clone(), asset.clone()));
                }
            }
        }
    }
    out
}

fn applicable_asset<'a>(
    release: &'a DespotesRelease,
    loader_slug: &str,
    mc_version: &str,
) -> Option<&'a DespotesAsset> {
    release
        .assets
        .iter()
        .find(|a| is_applicable(a, loader_slug, mc_version))
}

fn is_applicable(asset: &DespotesAsset, loader_slug: &str, mc_version: &str) -> bool {
    let Some(parsed) = parse_asset_name(&asset.name) else {
        return false;
    };
    parsed.loader == loader_slug && asset_covers_mc(mc_version, &parsed.mc_version, loader_slug)
}

/// Directory where downloaded Despotes artifacts are cached.
pub fn cache_dir() -> Result<PathBuf> {
    let dir = crate::util::paths::get_data_dir()?.join("despotes");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create cache dir {}", dir.display()))?;
    Ok(dir)
}

/// Download a Despotes asset into the cache (skipping when a matching file
/// is already present) and verify its sha256 digest when one is provided.
/// Returns the cached file path.
pub async fn download_asset(asset: &DespotesAsset) -> Result<PathBuf> {
    let dir = cache_dir()?;
    let dest = dir.join(&asset.name);
    if dest.exists() {
        if digest_matches(&dest, asset.digest.as_deref()).await.unwrap_or(false) {
            return Ok(dest);
        }
        // Cached file is corrupt or mismatched: re-download.
        let _ = tokio::fs::remove_file(&dest).await;
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

    if let Some(digest) = asset.digest.as_deref() {
        if !digest_matches(&dest, Some(digest)).await? {
            let _ = tokio::fs::remove_file(&dest).await;
            anyhow::bail!(
                "Checksum mismatch for {} (expected {})",
                asset.name,
                digest
            );
        }
    }
    Ok(dest)
}

/// Install a cached Despotes JAR into the instance's mods directory.
/// Replaces any previously installed Despotes build. Returns the installed
/// filename.
pub async fn install_into(instance_dir: &Path, source: &Path) -> Result<String> {
    let mods_dir = instance_dir.join("mods");
    tokio::fs::create_dir_all(&mods_dir).await?;

    // Remove older Despotes builds so only one version is loaded.
    for old in installed_despotes(instance_dir) {
        if old.file_name() != source.file_name() {
            let _ = tokio::fs::remove_file(&old).await;
        }
    }

    let dest = mods_dir.join(source.file_name().context("Invalid Despotes jar path")?);
    tokio::fs::copy(source, &dest).await.with_context(|| {
        format!("Failed to copy Despotes into {}", mods_dir.display())
    })?;

    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    tracing::info!("Installed Despotes mod: {}", name);
    Ok(name)
}

/// List Despotes JARs currently installed in the instance's mods dir.
pub fn installed_despotes(instance_dir: &Path) -> Vec<PathBuf> {
    let mods_dir = instance_dir.join("mods");
    let entries = match std::fs::read_dir(&mods_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str());
            (ext == Some("jar") || ext == Some("aje"))
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.starts_with(DESPOTES_JAR_PREFIX))
                    .unwrap_or(false)
        })
        .collect()
}

/// Filename used for the native (javaagent) Despotes variant inside the
/// instance root. Unlike loader mods it must NOT live in mods/ (vanilla has
/// no loader); it is attached as a JVM `-javaagent` at launch.
pub const NATIVE_AGENT_FILE: &str = "despotes-agent.jar";

/// Install a cached native (javaagent) Despotes jar at the instance root.
/// Replaces any previously installed native build. Returns the file name.
pub async fn install_native(instance_dir: &Path, source: &Path) -> Result<String> {
    let dest = instance_dir.join(NATIVE_AGENT_FILE);
    tokio::fs::copy(source, &dest).await.with_context(|| {
        format!("Failed to copy native Despotes agent into {}", instance_dir.display())
    })?;
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    tracing::info!("Installed Despotes native agent: {}", name);
    Ok(name)
}

/// Path to the native (javaagent) Despotes jar for this instance, if present.
pub fn native_agent_jar(instance_dir: &Path) -> Option<PathBuf> {
    let p = instance_dir.join(NATIVE_AGENT_FILE);
    if p.exists() { Some(p) } else { None }
}

/// Whether a Despotes control mod (loader variant) or native agent is present.
pub fn is_installed(instance_dir: &Path) -> bool {
    !installed_despotes(instance_dir).is_empty() || native_agent_jar(instance_dir).is_some()
}

async fn digest_matches(path: &Path, digest: Option<&str>) -> Result<bool> {
    let Some(digest) = digest else { return Ok(true) };
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    let bytes = tokio::fs::read(path).await?;
    use sha2::{Digest, Sha256};
    let actual = format!("{:x}", Sha256::digest(&bytes));
    Ok(actual.eq_ignore_ascii_case(hex))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_asset_name() {
        let p = parse_asset_name("Despotes-v26.0-Alpha.2-fabric-1.21.1.jar").unwrap();
        assert_eq!(p.tag, "v26.0-Alpha.2");
        assert_eq!(p.loader, "fabric");
        assert_eq!(p.mc_version, "1.21.1");

        let p = parse_asset_name("Despotes-v26.0-fabric-26.2.jar").unwrap();
        assert_eq!(p.tag, "v26.0");
        assert_eq!(p.mc_version, "26.2");

        assert!(parse_asset_name("fabric-api.jar").is_none());
        assert!(parse_asset_name("Despotes-v26.0-fabric.txt").is_none());
    }

    #[test]
    fn test_asset_covers_mc() {
        // exact match
        assert!(asset_covers_mc("1.21.1", "1.21.1", "fabric"));
        // modern fabric artifact covers 1.20-1.21.11
        assert!(asset_covers_mc("1.21.4", "1.21.1", "fabric"));
        assert!(asset_covers_mc("1.20.4", "1.21.1", "fabric"));
        assert!(!asset_covers_mc("1.22.1", "1.21.1", "fabric"));
        // legacy fabric artifact covers 1.20-1.20.6
        assert!(asset_covers_mc("1.20.3", "1.20.1", "fabric"));
        assert!(!asset_covers_mc("1.21.1", "1.20.1", "fabric"));
        // year-based versions only exact
        assert!(asset_covers_mc("26.2", "26.2", "fabric"));
        assert!(!asset_covers_mc("26.1", "26.2", "fabric"));
        assert!(!asset_covers_mc("1.21.4", "26.2", "fabric"));
    }

    fn sample_releases() -> Vec<DespotesRelease> {
        vec![
            DespotesRelease {
                tag: "v26.1".to_string(),
                prerelease: false,
                html_url: "u1".into(),
                published_at: "2026-09-01".into(),
                assets: vec![DespotesAsset {
                    name: "Despotes-v26.1-fabric-26.2.jar".into(),
                    url: "u".into(),
                    digest: None,
                    size: 1,
                }],
            },
            DespotesRelease {
                tag: "v26.0-Alpha.2".to_string(),
                prerelease: true,
                html_url: "u2".into(),
                published_at: "2026-08-08".into(),
                assets: vec![
                    DespotesAsset {
                        name: "Despotes-v26.0-Alpha.2-fabric-1.21.1.jar".into(),
                        url: "u".into(),
                        digest: None,
                        size: 1,
                    },
                    DespotesAsset {
                        name: "Despotes-v26.0-Alpha.2-fabric-26.2.jar".into(),
                        url: "u".into(),
                        digest: None,
                        size: 1,
                    },
                ],
            },
        ]
    }

    #[test]
    fn test_select_prefers_stable() {
        let releases = sample_releases();
        // applicable stable exists -> stable wins even when pre also applies
        let (release, asset) =
            select_release(&releases, "fabric", "26.2", true).expect("select");
        assert!(!release.prerelease);
        assert!(asset.name.contains("v26.1"));
    }

    #[test]
    fn test_select_falls_back_to_prerelease_when_allowed() {
        let releases = sample_releases();
        // 1.21.1 is not covered by the stable v26.1 (26.2 only)
        let (release, _) = select_release(&releases, "fabric", "1.21.1", true).expect("select");
        assert!(release.prerelease);
        assert_eq!(release.tag, "v26.0-Alpha.2");
    }

    #[test]
    fn test_select_excludes_prerelease_when_not_allowed() {
        let releases = sample_releases();
        assert!(select_release(&releases, "fabric", "1.21.1", false).is_none());
    }

    // Network-dependent integration test: verifies real download,
    // sha256 digest verification and copy-install into an instance.
    // Disabled by default; run explicitly with `--ignored`.
    #[tokio::test]
    #[ignore]
    async fn test_real_download_and_install() {
        let releases = fetch_releases().await.expect("fetch releases");
        let (rel, asset) = select_release(&releases, "fabric", "1.21.1", true)
            .expect("applicable asset");
        assert!(rel.prerelease, "Despotes currently has no stable release");

        let cached = download_asset(&asset).await.expect("download");
        assert!(cached.exists());

        // install into a temp instance and verify
        let instance = tempfile::tempdir().unwrap();
        let installed = install_into(instance.path(), &cached).await.expect("install");
        assert!(installed.starts_with("Despotes-"));
        assert!(is_installed(instance.path()));
        // re-install same file is idempotent
        let again = install_into(instance.path(), &cached).await.expect("reinstall");
        assert_eq!(again, installed);
        assert_eq!(installed_despotes(instance.path()).len(), 1);
    }

    #[test]
    fn test_vanilla_maps_to_native() {
        // Vanilla instances (None / "none" / "vanilla") use the native branch.
        assert_eq!(despotes_loader_for(None), Some(DespotesLoader::Native));
        assert_eq!(despotes_loader_for(Some("vanilla")), Some(DespotesLoader::Native));
        assert_eq!(despotes_loader_for(Some("none")), Some(DespotesLoader::Native));
        // Real loaders map to themselves.
        assert_eq!(despotes_loader_for(Some("fabric")), Some(DespotesLoader::Fabric));
        assert_eq!(despotes_loader_for(Some("FABRIC")), Some(DespotesLoader::Fabric));
        assert_eq!(despotes_loader_for(Some("aprism")), Some(DespotesLoader::Aprism));
        // Unsupported loaders stay unsupported.
        assert_eq!(despotes_loader_for(Some("optifine")), None);
    }

    #[test]
    fn test_javaagent_variant() {
        assert!(is_javaagent_variant("native"));
        assert!(!is_javaagent_variant("fabric"));
        assert!(!is_javaagent_variant("neoforge"));
    }

    // Network-dependent integration test: the reported bug was that vanilla
    // instances (loader "none") could not find a Despotes build. Verify the
    // native branch is selected for a vanilla instance and installs as a
    // javaagent at the instance root. Disabled by default; run with --ignored.
    #[tokio::test]
    #[ignore]
    async fn test_real_native_variant_for_vanilla() {
        // Vanilla instance -> native branch.
        assert_eq!(despotes_loader_for(None), Some(DespotesLoader::Native));
        assert!(is_javaagent_variant(DespotesLoader::Native.slug()));

        let releases = fetch_releases().await.expect("fetch releases");
        // Pick a MC version the native branch actually ships (probe from the
        // real release list so the test does not hardcode a brittle version).
        let mc = releases
            .iter()
            .flat_map(|r| r.assets.iter())
            .filter_map(|a| parse_asset_name(&a.name))
            .find(|p| p.loader == "native")
            .map(|p| p.mc_version)
            .expect("a native asset exists");

        let (rel, asset) = select_release(&releases, "native", &mc, true)
            .expect("native asset selectable");
        assert_eq!(parse_asset_name(&asset.name).unwrap().loader, "native");

        let cached = download_asset(&asset).await.expect("download native");
        assert!(cached.exists());

        let instance = tempfile::tempdir().unwrap();
        let installed = install_native(instance.path(), &cached).await.expect("install native");
        // install_native renames the agent to the fixed NATIVE_AGENT_FILE name.
        assert_eq!(installed, NATIVE_AGENT_FILE);
        // Native agent lives at the instance root, not mods/.
        assert!(native_agent_jar(instance.path()).is_some());
        assert!(installed_despotes(instance.path()).is_empty());
        // is_installed must report true via the native agent.
        assert!(is_installed(instance.path()));
        let _ = rel;
    }

    #[test]
    fn test_parse_aje_asset() {
        // Aprism variant ships .aje, e.g. Despotes-v26.1-Alpha.8-aprism-26.2.aje
        let p = parse_asset_name("Despotes-v26.1-Alpha.8-aprism-26.2.aje").unwrap();
        assert_eq!(p.tag, "v26.1-Alpha.8");
        assert_eq!(p.loader, "aprism");
        assert_eq!(p.mc_version, "26.2");
        // Non-despotes .aje is rejected.
        assert!(parse_asset_name("fabric-api.aje").is_none());
    }

    #[test]
    fn test_list_applicable_order() {
        let releases = sample_releases();
        let list = list_applicable(&releases, "fabric", "26.2", true);
        // stable first, then prerelease
        assert_eq!(list.len(), 2);
        assert!(!list[0].0.prerelease);
        assert!(list[1].0.prerelease);
    }
}
