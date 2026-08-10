// Pre-launch file integrity verification with self-repair (Alpha 8.1).
//
// Corrupted or truncated game files (client JAR, libraries, assets) cause
// obscure launch failures. MDL verifies these files before every launch and
// automatically re-downloads anything that fails its checksum, so a damaged
// cache heals itself instead of breaking the game.
//
// Verification scope:
//   1. client JAR  — always hashed (one file, cheap)
//   2. base libraries with a known sha1 — always hashed
//   3. asset objects — hashed once per asset index id; a marker file records
//      the verified index so subsequent launches skip the expensive full
//      scan (assets number in the thousands). Missing objects are still
//      fetched by the normal asset downloader.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::instance::config::InstanceConfig;
use crate::util::checksum::verify_sha1;
use crate::version::manifest::VersionMetadata;

/// Verify the client JAR and libraries of `version_metadata`; re-download any
/// file that is missing or fails its sha1 check. Returns the number of files
/// repaired (0 means everything was intact).
pub async fn verify_and_repair_core_files(
    launcher: &super::launcher::InstanceLauncher,
    version_dir: &Path,
    version_metadata: &VersionMetadata,
    libraries_dir: &Path,
) -> Result<u32> {
    let mut repaired = 0u32;

    // 1. Client JAR ---------------------------------------------------------
    let client = &version_metadata.downloads.client;
    let client_jar = version_dir.join(format!("{}.jar", version_metadata.id));
    if !file_matches_sha1(&client_jar, &client.sha1).await {
        tracing::warn!(
            "Client JAR missing or corrupted ({}), re-downloading...",
            client_jar.display()
        );
        if let Some(parent) = client_jar.parent() {
            fs::create_dir_all(parent).await?;
        }
        crate::version::downloader::download_file(&client.url, &client_jar, Some(&client.sha1))
            .await
            .context("Failed to re-download client JAR")?;
        repaired += 1;
        tracing::info!("Client JAR repaired");
    }

    // 2. Base libraries with checksums --------------------------------------
    for library in &version_metadata.libraries {
        // Only verify libraries that apply to this platform.
        if let Some(rules) = &library.rules {
            if !rules_allow_current_os(rules) {
                continue;
            }
        }
        let Some(downloads) = &library.downloads else {
            continue;
        };
        let Some(artifact) = &downloads.artifact else {
            continue;
        };
        if artifact.url.is_empty() || artifact.sha1.is_empty() {
            continue; // no checksum available — nothing to verify
        }

        let lib_path = launcher.get_library_path_from_name(&library.name, libraries_dir);
        if !file_matches_sha1(&lib_path, &artifact.sha1).await {
            tracing::warn!("Library corrupted, re-downloading: {}", library.name);
            launcher
                .download_library(&artifact.url, &lib_path, &artifact.sha1)
                .await
                .with_context(|| format!("Failed to re-download library {}", library.name))?;
            repaired += 1;
        }
    }

    if repaired > 0 {
        tracing::info!("Integrity check: repaired {} corrupted file(s)", repaired);
    } else {
        tracing::debug!("Integrity check: client JAR and libraries OK");
    }
    Ok(repaired)
}

/// Verify asset objects for `asset_index_id`. A full sha1 scan runs only once
/// per asset index (result cached in a marker file); afterwards the scan is
/// skipped because asset objects are content-addressed and immutable.
/// Corrupted objects are deleted so the asset downloader re-fetches them.
pub async fn verify_assets(
    assets_dir: &Path,
    asset_index_id: &str,
    asset_index_url: &str,
) -> Result<()> {
    let marker = assets_dir.join(format!(".verified-{}", asset_index_id));
    if marker.exists() {
        tracing::debug!("Asset index '{}' already verified (marker present)", asset_index_id);
        return Ok(());
    }

    // Load the index (download it if absent).
    let indexes_dir = assets_dir.join("indexes");
    let index_path = indexes_dir.join(format!("{}.json", asset_index_id));
    let index_content = if index_path.exists() {
        fs::read_to_string(&index_path).await?
    } else {
        let response = crate::util::http::create_http_client()?
            .get(asset_index_url)
            .send()
            .await
            .context("Failed to download asset index")?;
        if !response.status().is_success() {
            anyhow::bail!("Asset index download failed: HTTP {}", response.status());
        }
        let text = response.text().await?;
        fs::create_dir_all(&indexes_dir).await?;
        fs::write(&index_path, &text).await?;
        text
    };

    let index: crate::version::assets::AssetIndexFile = serde_json::from_str(&index_content)
        .context("Failed to parse asset index JSON")?;

    let objects_dir = assets_dir.join("objects");
    let mut corrupt = 0u32;
    let mut checked = 0u32;
    for object in index.objects.values() {
        if object.hash.len() < 2 {
            continue;
        }
        checked += 1;
        let path = objects_dir.join(&object.hash[0..2]).join(&object.hash);
        if !path.exists() {
            continue; // missing objects are handled by download_assets
        }
        if !file_matches_sha1(&path, &object.hash).await {
            tracing::warn!("Corrupt asset detected, removing for re-download: {}", &object.hash[..8]);
            let _ = fs::remove_file(&path).await;
            corrupt += 1;
        }
    }
    if corrupt > 0 {
        tracing::info!("Asset verification removed {} corrupt object(s); they will be re-downloaded", corrupt);
    } else {
        tracing::debug!("Asset verification: {} objects OK", checked);
    }

    // Mark this index as verified.
    fs::write(&marker, format!("verified {} objects at {}\n", checked, chrono_now())).await
        .context("Failed to write asset verification marker")?;
    Ok(())
}

/// Minimal platform rule evaluation for `manifest::Rule` entries (mirrors
/// the launcher's `check_rules` but operates on the manifest types). Only
/// OS-based rules are relevant for library verification on this machine.
fn rules_allow_current_os(rules: &[crate::version::manifest::Rule]) -> bool {
    let os_name = std::env::consts::OS;
    for rule in rules {
        let matches = if let Some(os_rule) = &rule.os {
            if let Some(name) = &os_rule.name {
                match name.as_str() {
                    "windows" => os_name == "windows",
                    "linux" => os_name == "linux",
                    "osx" => os_name == "macos",
                    _ => false,
                }
            } else {
                true
            }
        } else {
            true
        };
        if matches && rule.action == "disallow" {
            return false;
        }
    }
    true
}

/// Return true when `path` exists and its content matches `expected_sha1`.
async fn file_matches_sha1(path: &Path, expected_sha1: &str) -> bool {
    let Ok(bytes) = fs::read(path).await else {
        return false;
    };
    verify_sha1(&bytes, expected_sha1)
}

fn chrono_now() -> String {
    // Avoid pulling chrono: use SystemTime since UNIX epoch as a simple stamp.
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_file_matches_sha1_missing() {
        assert!(!file_matches_sha1(Path::new("/nonexistent/xyz"), "abc").await);
    }

    #[tokio::test]
    async fn test_file_matches_sha1_ok() {
        let dir = std::env::temp_dir().join("mdl-verify-test");
        fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("hello.txt");
        fs::write(&f, b"hello world").await.unwrap();
        assert!(file_matches_sha1(&f, "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed").await);
        assert!(!file_matches_sha1(&f, "deadbeef").await);
        let _ = fs::remove_dir_all(&dir).await;
    }
}
