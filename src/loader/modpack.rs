// Modrinth modpack (.mrpack) import with auto-completion (Alpha 8.1).
//
// A Modrinth modpack is a zip archive containing:
//   - `modrinth.index.json`  — pack metadata + file list (path, hashes, urls)
//   - `overrides/`           — files copied verbatim into the instance root
//   - optional `client-overrides/` / `server-overrides/`
//
// MDL "completes" a pack: it creates the instance with the correct Minecraft
// version and loader (read from the index `dependencies`), copies overrides,
// then downloads every indexed file (sha1-verified, skipped when already
// present), so importing a pack yields a ready-to-launch instance.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Top-level `modrinth.index.json` structure.
#[derive(Debug, Deserialize)]
pub struct ModrinthPackIndex {
    #[serde(rename = "formatVersion")]
    pub format_version: u32,
    #[serde(default)]
    pub game: String,
    #[serde(rename = "versionId")]
    pub version_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub dependencies: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub files: Vec<PackFile>,
}

/// One file entry in the pack index.
#[derive(Debug, Deserialize)]
pub struct PackFile {
    pub path: String,
    #[serde(default)]
    pub hashes: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    pub downloads: Vec<String>,
    #[serde(default, rename = "fileSize")]
    pub file_size: u64,
}

impl ModrinthPackIndex {
    /// Minecraft version required by the pack.
    pub fn minecraft_version(&self) -> Option<&str> {
        self.dependencies.get("minecraft").map(|s| s.as_str())
    }

    /// Loader (type, version) required by the pack, if any.
    pub fn loader(&self) -> Option<(&'static str, &str)> {
        if let Some(v) = self.dependencies.get("fabric-loader") {
            return Some(("fabric", v.as_str()));
        }
        if let Some(v) = self.dependencies.get("quilt-loader") {
            return Some(("quilt", v.as_str()));
        }
        if let Some(v) = self.dependencies.get("forge") {
            return Some(("forge", v.as_str()));
        }
        if let Some(v) = self.dependencies.get("neoforge") {
            return Some(("neoforge", v.as_str()));
        }
        None
    }

    /// Files that should be installed on the client side (env.client != "unsupported").
    pub fn client_files(&self) -> Vec<&PackFile> {
        self.files
            .iter()
            .filter(|f| {
                f.env
                    .get("client")
                    .map(|v| v != "unsupported")
                    .unwrap_or(true)
            })
            .collect()
    }
}

/// Read and parse `modrinth.index.json` from an `.mrpack` archive without
/// extracting anything else.
pub fn read_pack_index(mrpack_path: &Path) -> Result<ModrinthPackIndex> {
    let file = std::fs::File::open(mrpack_path)
        .with_context(|| format!("Failed to open modpack {}", mrpack_path.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("Not a valid .mrpack archive")?;
    let mut entry = zip
        .by_name("modrinth.index.json")
        .context("modrinth.index.json not found in modpack")?;
    let mut raw = String::new();
    std::io::Read::read_to_string(&mut entry, &mut raw)?;
    let index: ModrinthPackIndex =
        serde_json::from_str(&raw).context("Failed to parse modrinth.index.json")?;
    if index.game != "minecraft" {
        anyhow::bail!("Unsupported pack game type: '{}'", index.game);
    }
    Ok(index)
}

/// Extract `overrides/` (and `client-overrides/`) into the instance root.
/// Returns the number of files copied.
pub fn extract_overrides(mrpack_path: &Path, instance_dir: &Path) -> Result<usize> {
    let file = std::fs::File::open(mrpack_path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut count = 0usize;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().to_string();
        let rel = if let Some(r) = name.strip_prefix("overrides/") {
            r.to_string()
        } else if let Some(r) = name.strip_prefix("client-overrides/") {
            r.to_string()
        } else {
            continue;
        };
        if rel.is_empty() || entry.is_dir() {
            continue;
        }
        // Prevent path traversal (zip-slip): reject absolute or escaping paths.
        if rel.starts_with('/') || rel.contains("..") {
            tracing::warn!("Skipping unsafe override path in pack: {}", rel);
            continue;
        }
        let dest = instance_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut buf)?;
        std::fs::write(&dest, &buf)?;
        count += 1;
    }
    Ok(count)
}

/// Download every client-side file listed in the pack index into the instance
/// directory. Existing files with a matching sha1 are skipped (completion is
/// idempotent — re-running only fetches what is missing or corrupted).
/// Returns (installed, skipped) counts.
pub async fn download_pack_files(
    index: &ModrinthPackIndex,
    instance_dir: &Path,
) -> Result<(usize, usize)> {
    let files = index.client_files();
    let mut installed = 0usize;
    let mut skipped = 0usize;
    let mut failed: Vec<String> = Vec::new();

    for f in files {
        // Reject path traversal in file paths as well.
        if f.path.starts_with('/') || f.path.contains("..") {
            tracing::warn!("Skipping unsafe file path in pack: {}", f.path);
            failed.push(f.path.clone());
            continue;
        }
        let dest: PathBuf = instance_dir.join(&f.path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Already present and intact? Skip.
        if dest.exists() {
            if let Some(expected_sha1) = f.hashes.get("sha1") {
                if let Ok(bytes) = tokio::fs::read(&dest).await {
                    if crate::util::checksum::verify_sha1(&bytes, expected_sha1) {
                        skipped += 1;
                        continue;
                    }
                }
            } else {
                skipped += 1;
                continue;
            }
        }

        let sha1 = f.hashes.get("sha1").cloned();
        let mut ok = false;
        for url in &f.downloads {
            match crate::version::downloader::download_file(url, &dest, sha1.as_deref()).await {
                Ok(()) => {
                    ok = true;
                    break;
                }
                Err(e) => {
                    tracing::warn!("Download failed for {} from {}: {}", f.path, url, e);
                }
            }
        }
        if ok {
            installed += 1;
            tracing::info!("Installed pack file: {}", f.path);
        } else {
            failed.push(f.path.clone());
        }
    }

    if !failed.is_empty() {
        anyhow::bail!(
            "{} pack file(s) could not be installed: {}",
            failed.len(),
            failed.join(", ")
        );
    }
    Ok((installed, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_index_json() -> &'static str {
        r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "versionId": "1.0.0",
            "name": "Test Pack",
            "summary": "A test pack",
            "dependencies": {
                "minecraft": "1.21.1",
                "fabric-loader": "0.16.9"
            },
            "files": [
                {
                    "path": "mods/example.jar",
                    "hashes": {"sha1": "abc", "sha512": "def"},
                    "env": {"client": "required", "server": "required"},
                    "downloads": ["https://example.com/example.jar"],
                    "fileSize": 1234
                },
                {
                    "path": "mods/serveronly.jar",
                    "hashes": {"sha1": "xyz"},
                    "env": {"client": "unsupported", "server": "required"},
                    "downloads": ["https://example.com/serveronly.jar"],
                    "fileSize": 100
                }
            ]
        }"#
    }

    #[test]
    fn test_parse_pack_index() {
        let idx: ModrinthPackIndex = serde_json::from_str(sample_index_json()).unwrap();
        assert_eq!(idx.minecraft_version(), Some("1.21.1"));
        assert_eq!(idx.loader(), Some(("fabric", "0.16.9")));
        assert_eq!(idx.files.len(), 2);
        // server-only file must be filtered out for client installs
        let client = idx.client_files();
        assert_eq!(client.len(), 1);
        assert_eq!(client[0].path, "mods/example.jar");
    }

    #[test]
    fn test_loader_detection_none() {
        let raw = r#"{"formatVersion":1,"game":"minecraft","versionId":"1","dependencies":{"minecraft":"1.20"}}"#;
        let idx: ModrinthPackIndex = serde_json::from_str(raw).unwrap();
        assert!(idx.loader().is_none());
    }
}
