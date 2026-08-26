// AprismJDK (AJR - Aprism Java Runtime) provisioning.
//
// AprismJDK is the AprismLab JDK distribution, published as GitHub releases
// on AprismLab/AprismJDK. Stable archives follow the naming scheme
//
//     AprismJDK-<version>-<os>-<arch>-jdk.<zip|tar.gz>
//
// e.g. `AprismJDK-26.2-windows-x64-jdk.zip` (~265MB), accompanied by a
// SHA256SUMS.txt manifest. Older pre-release tags carried unrelated agent
// jars; only the `-jdk.<ext>` shape is accepted here.
//
// Layout in the MDL java cache:
//   <java_cache>/aprism-jdk/<tag>/...        extracted runtimes
//   <java_cache>/aprism-jdk/_archives/       downloaded archives (kept for
//                                            offline reinstall / audit)
//
// v26.4-alpha.6.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use tracing::info;

pub const APRISM_JDK_OWNER: &str = "AprismLab";
pub const APRISM_JDK_REPO: &str = "AprismJDK";

#[derive(Debug, Clone)]
pub struct AprismJdkAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct AprismJdkRelease {
    pub tag: String,
    pub prerelease: bool,
    pub html_url: String,
    pub assets: Vec<AprismJdkAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedJdkAsset {
    /// Release version segment, e.g. "26.2".
    pub version: String,
    /// OS segment: windows | linux | macos.
    pub os: &'static str,
    /// Arch segment: x64 | aarch64.
    pub arch: &'static str,
    /// Archive extension: zip | tar.gz.
    pub ext: &'static str,
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
    #[serde(default)]
    size: Option<u64>,
}

/// Fetch AprismJDK releases (newest first).
pub async fn fetch_releases() -> Result<Vec<AprismJdkRelease>> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases?per_page=30",
        APRISM_JDK_OWNER, APRISM_JDK_REPO
    );
    let response = crate::util::http::create_http_client()?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Failed to query AprismJDK releases")?;
    if !response.status().is_success() {
        anyhow::bail!(
            "GitHub returned HTTP {} for AprismJDK releases",
            response.status()
        );
    }
    let raw: Vec<GhRelease> =
        response.json().await.context("Failed to parse AprismJDK releases")?;
    Ok(raw
        .into_iter()
        .map(|r| AprismJdkRelease {
            tag: r.tag_name,
            prerelease: r.prerelease,
            html_url: r.html_url,
            assets: r
                .assets
                .into_iter()
                .map(|a| AprismJdkAsset {
                    name: a.name,
                    url: a.browser_download_url,
                    size: a.size.unwrap_or(0),
                })
                .collect(),
        })
        .collect())
}

/// Parse an AprismJDK runtime archive name. Returns None for non-runtime
/// assets (agent/sources jars, checksum lists) and unknown platforms.
///
/// `AprismJDK-26.2-windows-x64-jdk.zip`
///   -> version 26.2, os windows, arch x64, ext zip
pub fn parse_asset_name(name: &str) -> Option<ParsedJdkAsset> {
    let (stem, ext) = if let Some(s) = name.strip_suffix("-jdk.tar.gz") {
        (s, "tar.gz")
    } else if let Some(s) = name.strip_suffix("-jdk.zip") {
        (s, "zip")
    } else {
        return None;
    };

    // stem = AprismJDK-<ver>-<os>-<arch>; ver may contain dashes (v26.1-Alpha.4),
    // so anchor on the LAST two dash-separated segments for os/arch.
    let mut parts: Vec<&str> = stem.split('-').collect();
    if parts.len() < 4 || parts[0] != "AprismJDK" {
        return None;
    }
    let arch_raw = parts.pop()?;
    let os_raw = parts.pop()?;
    // Tags carry a "v" prefix; versions are reported bare for consistency.
    let version = parts[1..].join("-").trim_start_matches('v').to_string();

    let os = match os_raw.to_ascii_lowercase().as_str() {
        "windows" => "windows",
        "linux" => "linux",
        "macos" | "darwin" | "mac" => "macos",
        _ => return None,
    };
    let arch = match arch_raw.to_ascii_lowercase().as_str() {
        "x64" | "x86_64" | "amd64" => "x64",
        "aarch64" | "arm64" => "aarch64",
        _ => return None,
    };

    Some(ParsedJdkAsset { version, os, arch, ext })
}

/// This host's platform segments in AprismJDK asset naming.
pub fn current_platform() -> (&'static str, &'static str) {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    { ("windows", "x64") }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    { ("windows", "aarch64") }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { ("linux", "x64") }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    { ("linux", "aarch64") }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { ("macos", "aarch64") }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { ("macos", "x64") }
}

/// Preferred archive extension for this platform (matches Adoptium policy).
fn preferred_ext() -> &'static str {
    if cfg!(target_os = "windows") { "zip" } else { "tar.gz" }
}

/// Pick the best runtime archive for this host from one release.
fn pick_asset<'a>(
    release: &'a AprismJdkRelease,
) -> Option<(&'a AprismJdkAsset, ParsedJdkAsset)> {
    let (want_os, want_arch) = current_platform();
    let want_ext = preferred_ext();
    let mut best: Option<(&'a AprismJdkAsset, ParsedJdkAsset)> = None;
    for asset in &release.assets {
        let Some(parsed) = parse_asset_name(&asset.name) else { continue };
        if parsed.os != want_os || parsed.arch != want_arch {
            continue;
        }
        let better = match &best {
            None => true,
            Some((_, b)) => {
                // Prefer the platform-native archive format over a fallback.
                parsed.ext == want_ext && b.ext != want_ext
            }
        };
        if better {
            best = Some((asset, parsed));
        }
    }
    best
}

/// Select (release, asset) for this host. `version_hint` matches a tag
/// ("v26.2") or version ("26.2"); without a hint the newest stable release
/// wins, falling back to prereleases only when `prerelease` is set.
pub fn select_release<'a>(
    releases: &'a [AprismJdkRelease],
    version_hint: Option<&str>,
    prerelease: bool,
) -> Option<(&'a AprismJdkRelease, (&'a AprismJdkAsset, ParsedJdkAsset))> {
    for rel in releases {
        if let Some(hint) = version_hint {
            let h = hint.trim_start_matches('v');
            if rel.tag.trim_start_matches('v') != h && !rel.tag.contains(h) {
                continue;
            }
        } else if rel.prerelease && !prerelease {
            continue;
        }
        if let Some(picked) = pick_asset(rel) {
            return Some((rel, picked));
        }
    }
    None
}

/// Directory holding extracted AprismJDK runtimes, keyed by tag.
fn jdk_base_dir() -> Result<PathBuf> {
    Ok(crate::util::paths::get_java_cache_dir()?.join("aprism-jdk"))
}

/// List installed AprismJDK runtimes as (tag, java executable path), newest
/// directory first. Entries whose java binary vanished are skipped.
pub fn installed() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let Ok(base) = jdk_base_dir() else { return out };
    let Ok(entries) = std::fs::read_dir(&base) else { return out };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let tag = entry.file_name().to_string_lossy().to_string();
        if tag == "_archives" {
            continue;
        }
        if let Ok(java) =
            crate::version::java::JavaRuntime::find_java_binary(&entry.path())
        {
            out.push((tag, java));
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out
}

/// Resolve an installed runtime by tag or version substring (e.g. "aprism"
/// alone resolves the newest; "aprism@26.2" or "26.2" matches a tag).
pub fn resolve(hint: Option<&str>) -> Result<(String, PathBuf)> {
    let all = installed();
    match hint {
        None => all.first().cloned().ok_or_else(|| {
            anyhow::anyhow!("No AprismJDK installed. Run 'mdl jdk install' first.")
        }),
        Some(h) => {
            let needle = h.trim_start_matches("aprism@").trim();
            if needle.is_empty() {
                return all.first().cloned().ok_or_else(|| {
                    anyhow::anyhow!("No AprismJDK installed. Run 'mdl jdk install' first.")
                });
            }
            let n = needle.trim_start_matches('v');
            all.into_iter()
                .find(|(tag, _)| tag.trim_start_matches('v') == n || tag.contains(n))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No AprismJDK matching '{h}'. Installed: {}.",
                        if installed().is_empty() {
                            "none".to_string()
                        } else {
                            installed().iter().map(|(t, _)| t.clone()).collect::<Vec<_>>().join(", ")
                        }
                    )
                })
        }
    }
}

/// Remove an installed runtime by tag. Returns whether anything was deleted.
pub fn remove(tag: &str) -> Result<bool> {
    let base = jdk_base_dir()?;
    let dir = base.join(tag);
    if !dir.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(&dir)
        .with_context(|| format!("Failed to remove AprismJDK dir {:?}", dir))?;
    Ok(true)
}

/// Verify a streamed archive against the release's SHA256SUMS.txt.
async fn verify_sha256(
    client: &reqwest::Client,
    release: &AprismJdkRelease,
    asset_name: &str,
    archive_path: &std::path::Path,
) -> Result<()> {
    use sha2::Digest as _;

    let sums_asset = release
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("SHA256SUMS.txt"));
    let Some(sums) = sums_asset else {
        info!("SHA256SUMS.txt absent for this release; skipping verification");
        return Ok(());
    };
    let text = client
        .get(&sums.url)
        .send()
        .await
        .context("Failed to fetch SHA256SUMS.txt")?
        .error_for_status()
        .context("SHA256SUMS.txt download failed")?
        .text()
        .await?;

    let expected = text
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let hash = it.next()?;
            let file = it.next()?.trim_start_matches('*');
            (file == asset_name).then(|| hash.to_lowercase())
        })
        .next();
    let Some(expected) = expected else {
        info!("No entry for {asset_name} in SHA256SUMS.txt; skipping verification");
        return Ok(());
    };

    let bytes = std::fs::read(archive_path)?;
    let actual = format!("{:x}", sha2::Sha256::digest(&bytes));
    if actual != expected {
        anyhow::bail!(
        "AprismJDK archive failed SHA-256 verification: expected {expected}, got {actual}"
        );
    }
    info!("SHA-256 verified ({})", asset_name);
    Ok(())
}

/// Download (streamed to disk), verify and extract the selected runtime.
/// Returns the resolved java executable path.
pub async fn download_and_install(
    release: &AprismJdkRelease,
    asset: &AprismJdkAsset,
) -> Result<PathBuf> {
    let base = jdk_base_dir()?;
    let archives = base.join("_archives");
    let target_dir = base.join(&release.tag);
    std::fs::create_dir_all(&archives)
        .with_context(|| format!("Failed to create {:?}", archives))?;
    std::fs::create_dir_all(&target_dir)
        .with_context(|| format!("Failed to create {:?}", target_dir))?;

    let ext = parse_asset_name(&asset.name).map(|p| p.ext).unwrap_or("zip");
    let archive_path = archives.join(&asset.name);

    info!(
        "Downloading AprismJDK {} ({} MB)...",
        release.tag,
        asset.size / (1024 * 1024)
    );

    // Streamed: a ~265MB archive must never sit fully in flight buffers.
    {
        let client = crate::util::http::create_download_client()?;
        let response = client
            .get(&asset.url)
            .send()
            .await
            .with_context(|| format!("Failed to start download from {}", asset.url))?
            .error_for_status()
            .with_context(|| format!("Download failed: HTTP from {}", asset.url))?;

        use futures_util::StreamExt;
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::File::create(&archive_path).await.with_context(|| {
            format!("Failed to create archive file {:?}", archive_path)
        })?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            file.write_all(&chunk.context("Failed to read AprismJDK chunk")?)
                .await?;
        }
        file.sync_all().await?;
        drop(file);
    }

    verify_sha256(&crate::util::http::create_http_client()?, release, &asset.name, &archive_path).await?;

    // Extraction reads the archive from disk once (same trade-off as the
    // Adoptium path; extraction APIs are byte-based).
    let bytes = std::fs::read(&archive_path)
        .with_context(|| format!("Failed to re-read archive {:?}", archive_path))?;
    info!("Extracting AprismJDK ({} MB on disk)...", bytes.len() / (1024 * 1024));
    match ext {
        "zip" => crate::version::java::JavaRuntime::extract_zip(&bytes, &target_dir)?,
        "tar.gz" => crate::version::java::JavaRuntime::extract_tar_gz(&bytes, &target_dir)?,
        other => anyhow::bail!("Unsupported AprismJDK archive format: {}", other),
    }

    let java = crate::version::java::JavaRuntime::find_java_binary(&target_dir)
        .context("Extracted AprismJDK has no bin/java executable")?;
    Ok(java)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_asset_name_stable() {
        let p = parse_asset_name("AprismJDK-26.2-windows-x64-jdk.zip").unwrap();
        assert_eq!(p.version, "26.2");
        assert_eq!(p.os, "windows");
        assert_eq!(p.arch, "x64");
        assert_eq!(p.ext, "zip");
    }

    #[test]
    fn test_parse_asset_name_prerelease_and_targz() {
        let p = parse_asset_name("AprismJDK-v26.3-Alpha.1-linux-x64-jdk.tar.gz").unwrap();
        assert_eq!(p.version, "26.3-Alpha.1");
        assert_eq!(p.os, "linux");
        assert_eq!(p.ext, "tar.gz");
    }

    #[test]
    fn test_parse_asset_name_rejects_non_runtime() {
        // Old alpha tags carried agent jars with no platform segments.
        assert!(parse_asset_name("AprismJDK-v26.1-Alpha.4.jar").is_none());
        assert!(parse_asset_name("aprismate-26.2.jar").is_none());
        assert!(parse_asset_name("SHA256SUMS.txt").is_none());
        // Unknown platform must be rejected.
        assert!(parse_asset_name("AprismJDK-26.2-solaris-sparc-jdk.zip").is_none());
    }

    #[test]
    fn test_select_release_prefers_stable_and_platform() {
        // Platform-agnostic: build asset names for whatever host the test
        // runs on (CI covers linux/macos/windows).
        let (os, arch) = current_platform();
        let native_ext = if cfg!(target_os = "windows") { "zip" } else { "tar.gz" };
        let fallback_ext = if cfg!(target_os = "windows") { "tar.gz" } else { "zip" };

        let mk = |tag: &str, pre: bool, names: &[&str]| AprismJdkRelease {
            tag: tag.to_string(),
            prerelease: pre,
            html_url: String::new(),
            assets: names
                .iter()
                .map(|n| AprismJdkAsset {
                    name: n.to_string(),
                    url: format!("https://example/{n}"),
                    size: 0,
                })
                .collect(),
        };

        let releases = vec![
            mk("v26.1-Alpha.5", true, &["AprismJDK-v26.1-Alpha.5.jar"]),
            mk(
                "v26.2",
                false,
                &[
                    "aprismate-26.2.jar",
                    "SHA256SUMS.txt",
                    &format!("AprismJDK-26.2-{os}-{arch}-jdk.{fallback_ext}"),
                    &format!("AprismJDK-26.2-{os}-{arch}-jdk.{native_ext}"),
                ],
            ),
        ];

        let (rel, (_, parsed)) = select_release(&releases, None, false).unwrap();
        assert_eq!(rel.tag, "v26.2");
        assert_eq!(parsed.version, "26.2");
        assert_eq!(parsed.os, os);
        assert_eq!(parsed.arch, arch);
        // Platform-native format preferred over the fallback archive.
        assert_eq!(parsed.ext, native_ext);

        // Version hint hits across tags.
        let (rel2, _) = select_release(&releases, Some("26.2"), false).unwrap();
        assert_eq!(rel2.tag, "v26.2");
        assert!(select_release(&releases, Some("99.9"), false).is_none());

        // Stable-first: prereleases skipped unless requested.
        let pre_only = vec![mk(
            "v27.0-beta",
            true,
            &[format!("AprismJDK-27.0-{os}-{arch}-jdk.{native_ext}").as_str()],
        )];
        assert!(select_release(&pre_only, None, false).is_none());
        assert!(select_release(&pre_only, None, true).is_some());
    }

    #[test]
    fn test_current_platform_is_known() {
        let (os, _arch) = current_platform();
        assert!(matches!(os, "windows" | "linux" | "macos"));
    }
}
