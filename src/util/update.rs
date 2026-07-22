// GitHub release update checker
//
// Queries the GitHub Releases API for the latest published release and compares
// its tag against the compiled-in crate version. The check is best-effort: any
// network or parsing failure is swallowed so it can never block or fail a
// command. Results are cached on disk for a short window to avoid hitting the
// API on every invocation.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

const GITHUB_OWNER: &str = "StarsailsClover";
const GITHUB_REPO: &str = "MCDebugLauncher";

/// Minimum interval between remote checks, in seconds (6 hours).
const CHECK_INTERVAL_SECS: u64 = 6 * 60 * 60;

/// Subset of the GitHub release object we care about.
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    prerelease: bool,
}

/// On-disk cache of the last successful check, so we don't query GitHub on
/// every command invocation.
#[derive(Debug, Serialize, Deserialize)]
struct UpdateCache {
    last_check: u64,
    latest_tag: String,
    html_url: String,
}

/// A newer release is available.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub url: String,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> Result<std::path::PathBuf> {
    Ok(crate::util::paths::get_cache_dir()?.join("update-check.json"))
}

fn read_cache() -> Option<UpdateCache> {
    let path = cache_path().ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_cache(cache: &UpdateCache) {
    if let Ok(path) = cache_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(cache) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// Normalize a version/tag string for comparison by stripping a leading `v`.
fn normalize(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// Compare two semver-ish version strings, returning true if `candidate` is
/// strictly newer than `current`. Handles the `MAJOR.MINOR.PATCH[-pre]` form
/// used by this crate (e.g. `26.0.0-alpha.2`). A release build (no pre-release
/// suffix) is always considered newer than a pre-release of the same numeric
/// version.
fn is_newer(candidate: &str, current: &str) -> bool {
    let (c_nums, c_pre) = split_version(normalize(candidate));
    let (u_nums, u_pre) = split_version(normalize(current));

    // Compare numeric release components first.
    for i in 0..c_nums.len().max(u_nums.len()) {
        let cn = c_nums.get(i).copied().unwrap_or(0);
        let un = u_nums.get(i).copied().unwrap_or(0);
        if cn != un {
            return cn > un;
        }
    }

    // Numeric versions equal: a non-pre-release outranks a pre-release.
    match (c_pre.as_deref(), u_pre.as_deref()) {
        (None, None) => false,
        (None, Some(_)) => true,   // candidate is a full release, current is pre
        (Some(_), None) => false,  // candidate is pre, current is full release
        (Some(cp), Some(up)) => cp > up, // lexical comparison of pre-release tags
    }
}

/// Split `1.2.3-alpha.2` into (`[1,2,3]`, Some("alpha.2")).
fn split_version(v: &str) -> (Vec<u64>, Option<String>) {
    let (core, pre) = match v.split_once('-') {
        Some((core, pre)) => (core, Some(pre.to_string())),
        None => (v, None),
    };
    let nums = core
        .split('.')
        .map(|p| p.parse::<u64>().unwrap_or(0))
        .collect();
    (nums, pre)
}

/// Query the GitHub Releases API for the latest release tag. Returns the tag
/// and its release page URL.
async fn fetch_latest_release() -> Result<(String, String)> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        GITHUB_OWNER, GITHUB_REPO
    );

    let client = crate::util::http::create_http_client()?;
    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Failed to query GitHub releases API")?;

    if !response.status().is_success() {
        anyhow::bail!("GitHub releases API returned HTTP {}", response.status());
    }

    let release: GitHubRelease = response
        .json()
        .await
        .context("Failed to parse GitHub release JSON")?;

    let _ = release.prerelease; // reserved for future filtering
    Ok((release.tag_name, release.html_url))
}

/// Check whether a newer release than the running version is available.
///
/// Best-effort and side-effect-free from the caller's perspective: uses a
/// disk-backed cache to throttle API calls to at most once per
/// [`CHECK_INTERVAL_SECS`], and returns `None` on any error. Never panics.
pub async fn check_for_update() -> Option<UpdateInfo> {
    let current = env!("CARGO_PKG_VERSION");
    let now = now_secs();

    // Fast path: reuse a recent cached result.
    if let Some(cache) = read_cache() {
        if now.saturating_sub(cache.last_check) < CHECK_INTERVAL_SECS {
            if is_newer(&cache.latest_tag, current) {
                return Some(UpdateInfo {
                    current: current.to_string(),
                    latest: normalize(&cache.latest_tag).to_string(),
                    url: cache.html_url,
                });
            }
            return None;
        }
    }

    // Slow path: hit the API and refresh the cache.
    match fetch_latest_release().await {
        Ok((tag, url)) => {
            write_cache(&UpdateCache {
                last_check: now,
                latest_tag: tag.clone(),
                html_url: url.clone(),
            });

            if is_newer(&tag, current) {
                Some(UpdateInfo {
                    current: current.to_string(),
                    latest: normalize(&tag).to_string(),
                    url,
                })
            } else {
                None
            }
        }
        Err(e) => {
            tracing::debug!("Update check skipped: {}", e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize() {
        assert_eq!(normalize("v1.2.3"), "1.2.3");
        assert_eq!(normalize("1.2.3"), "1.2.3");
    }

    #[test]
    fn test_split_version() {
        assert_eq!(split_version("1.2.3"), (vec![1, 2, 3], None));
        assert_eq!(
            split_version("26.0.0-alpha.2"),
            (vec![26, 0, 0], Some("alpha.2".to_string()))
        );
    }

    #[test]
    fn test_is_newer_numeric() {
        assert!(is_newer("1.2.4", "1.2.3"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(!is_newer("1.2.3", "1.2.3"));
        assert!(!is_newer("1.2.2", "1.2.3"));
    }

    #[test]
    fn test_is_newer_prerelease() {
        // Full release outranks pre-release of same numeric version.
        assert!(is_newer("26.0.0", "26.0.0-alpha.2"));
        // Pre-release does not outrank the full release.
        assert!(!is_newer("26.0.0-alpha.2", "26.0.0"));
        // Later pre-release outranks earlier one.
        assert!(is_newer("26.0.0-alpha.3", "26.0.0-alpha.2"));
        assert!(!is_newer("26.0.0-alpha.1", "26.0.0-alpha.2"));
    }

    #[test]
    fn test_is_newer_with_v_prefix() {
        assert!(is_newer("v26.1.0", "26.0.0"));
        assert!(is_newer("v1.0.0", "v0.9.0"));
    }
}
