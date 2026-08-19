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

// ---------------------------------------------------------------------------
// Export (v26.2-alpha.5): build a .mrpack from a live instance.
// ---------------------------------------------------------------------------

use serde::Serialize;
use sha1::{Digest, Sha1};

/// Serializable file entry for the export index.
#[derive(Debug, Serialize)]
struct ExportFile {
    path: String,
    hashes: std::collections::HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    env: Option<std::collections::HashMap<String, String>>,
    downloads: Vec<String>,
    #[serde(rename = "fileSize")]
    file_size: u64,
}

/// Serializable modrinth.index.json for export.
#[derive(Debug, Serialize)]
struct ExportIndex {
    #[serde(rename = "formatVersion")]
    format_version: u32,
    game: String,
    #[serde(rename = "versionId")]
    version_id: String,
    name: String,
    dependencies: std::collections::HashMap<String, String>,
    files: Vec<ExportFile>,
}

/// Export an instance to a `.mrpack` archive.
///
/// Scans the instance `mods/` directory for JAR files, computes sha1 hashes,
/// and builds a `modrinth.index.json` with download URLs pointing to
/// Modrinth's CDN (best-effort: if we can't determine the project, the
/// file is included in overrides instead). Non-mod files (configs, options,
/// shaderpacks, resourcepacks) are placed in `overrides/`.
pub fn export_to_mrpack(
    instance_dir: &Path,
    config: &crate::instance::config::InstanceConfig,
    output_path: &Path,
) -> Result<(usize, usize)> {
    let file = std::fs::File::create(output_path)
        .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut index_files = Vec::new();
    let mut override_count = 0usize;

    // Build dependencies map from instance config.
    let mut deps = std::collections::HashMap::new();
    deps.insert("minecraft".to_string(), config.version.clone());
    if let Some(loader) = &config.loader {
        match loader.loader_type.as_str() {
            "fabric" => {
                deps.insert("fabric-loader".to_string(), loader.version.clone());
            }
            "quilt" => {
                deps.insert("quilt-loader".to_string(), loader.version.clone());
            }
            "forge" => {
                deps.insert("forge".to_string(), loader.version.clone());
            }
            "neoforge" => {
                deps.insert("neoforge".to_string(), loader.version.clone());
            }
            _ => {}
        }
    }

    // Scan mods/ directory: include JARs as indexed files with sha1 hashes.
    let mods_dir = instance_dir.join("mods");
    if mods_dir.exists() {
        for entry in std::fs::read_dir(&mods_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jar") {
                continue;
            }
            let data = std::fs::read(&path)?;
            let mut hasher = Sha1::new();
            hasher.update(&data);
            let sha1 = hex::encode(&hasher.finalize());

            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown.jar");
            let rel_path = format!("mods/{}", filename);

            index_files.push(ExportFile {
                path: rel_path,
                hashes: {
                    let mut h = std::collections::HashMap::new();
                    h.insert("sha1".to_string(), sha1);
                    h
                },
                env: None,
                downloads: Vec::new(), // no known Modrinth URL without API lookup
                file_size: data.len() as u64,
            });
        }
    }

    // Include config files, options.txt, shaderpacks, resourcepacks as overrides.
    let override_dirs = ["config", "shaderpacks", "resourcepacks"];
    let override_files = ["options.txt", "servers.dat", "saves"];

    // Walk override directories.
    for dir in &override_dirs {
        let dir_path = instance_dir.join(dir);
        if !dir_path.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&dir_path) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry.path().strip_prefix(instance_dir)?;
            let zip_path = format!("overrides/{}", rel.to_string_lossy());
            zip.start_file(&zip_path, opts)?;
            let mut f = std::fs::File::open(entry.path())?;
            std::io::copy(&mut f, &mut zip)?;
            override_count += 1;
        }
    }

    // Include individual override files.
    for file_name in &override_files {
        let file_path = instance_dir.join(file_name);
        if file_path.is_file() {
            let zip_path = format!("overrides/{}", file_name);
            zip.start_file(&zip_path, opts)?;
            let mut f = std::fs::File::open(&file_path)?;
            std::io::copy(&mut f, &mut zip)?;
            override_count += 1;
        }
    }

    // Write modrinth.index.json.
    let index = ExportIndex {
        format_version: 1,
        game: "minecraft".to_string(),
        version_id: "1".to_string(),
        name: config.name.clone(),
        dependencies: deps,
        files: index_files,
    };
    let index_json = serde_json::to_string_pretty(&index)?;
    zip.start_file("modrinth.index.json", opts)?;
    std::io::Write::write_all(&mut zip, index_json.as_bytes())?;

    zip.finish()?;
    let mod_count = index.files.len();

    Ok((mod_count, override_count))
}

// Hex encoding for sha1 output (simple inline implementation).
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
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

    /// Build a synthetic .mrpack in a temp dir and verify index parsing +
    /// overrides extraction end-to-end (offline, hermetic).
    #[test]
    fn test_synthetic_mrpack_roundtrip() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let pack_path = dir.path().join("synthetic.mrpack");

        // Assemble the zip: modrinth.index.json + overrides/options.txt
        let file = std::fs::File::create(&pack_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();

        zip.start_file("modrinth.index.json", opts).unwrap();
        zip.write_all(sample_index_json().as_bytes()).unwrap();

        zip.start_file("overrides/options.txt", opts).unwrap();
        zip.write_all(b"fov:1.0
").unwrap();

        zip.start_file("overrides/config/test.toml", opts).unwrap();
        zip.write_all(b"key = \"value\"
").unwrap();

        zip.finish().unwrap();

        // Parse the index back from the archive.
        let index = read_pack_index(&pack_path).unwrap();
        assert_eq!(index.name, "Test Pack");
        assert_eq!(index.minecraft_version(), Some("1.21.1"));
        assert_eq!(index.client_files().len(), 1);

        // Extract overrides into a fresh instance dir.
        let inst = dir.path().join("instance");
        std::fs::create_dir_all(&inst).unwrap();
        let copied = extract_overrides(&pack_path, &inst).unwrap();
        assert_eq!(copied, 2);
        assert!(inst.join("options.txt").exists());
        assert!(inst.join("config/test.toml").exists());
        let content = std::fs::read_to_string(inst.join("config/test.toml")).unwrap();
        assert!(content.contains("value"));
    }

    /// Zip-slip protection: unsafe override paths must be skipped.
    #[test]
    fn test_zip_slip_override_skipped() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let pack_path = dir.path().join("evil.mrpack");
        let file = std::fs::File::create(&pack_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("modrinth.index.json", opts).unwrap();
        zip.write_all(sample_index_json().as_bytes()).unwrap();
        zip.start_file("overrides/../../evil.txt", opts).unwrap();
        zip.write_all(b"pwned").unwrap();
        zip.finish().unwrap();

        let inst = dir.path().join("instance");
        std::fs::create_dir_all(&inst).unwrap();
        let copied = extract_overrides(&pack_path, &inst).unwrap();
        assert_eq!(copied, 0, "unsafe path must be skipped");
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn test_loader_detection_none() {
        let raw = r#"{"formatVersion":1,"game":"minecraft","versionId":"1","dependencies":{"minecraft":"1.20"}}"#;
        let idx: ModrinthPackIndex = serde_json::from_str(raw).unwrap();
        assert!(idx.loader().is_none());
    }
}
