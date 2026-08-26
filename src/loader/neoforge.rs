// NeoForge mod loader installer

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

const NEOFORGE_MAVEN_URL: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const NEOFORGE_METADATA_URL: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";

/// Merge a single attribute into a JAR MANIFEST.MF main section.
///
/// Returns `Some(new_text)` when the attribute was added, `None` when an
/// attribute with the same name already exists (anywhere in the manifest,
/// case-insensitive line-start match). Preserves named sections verbatim.
///
/// JAR manifest rules honored here: attributes are `Key: value` lines, the
/// main section ends at the first blank line, and every line (including the
/// last) must be newline-terminated. Line wrapping of pre-existing content
/// is left untouched.
pub fn merge_manifest_attribute(text: &str, key: &str, value: &str) -> Option<String> {
    let needle = format!("{key}:");
    if text
        .lines()
        .any(|l| l.trim_end().to_ascii_lowercase().starts_with(&needle.to_ascii_lowercase()))
    {
        return None;
    }

    let attr = format!("{key}: {value}");

    // Find the end of the main section: the first blank line (\r\n\r\n or
    // \n\n). Everything from there on is named sections and must stay put.
    let split_idx = text.find("\r\n\r\n").or_else(|| text.find("\n\n"));
    match split_idx {
        Some(i) => {
            let head = text[..i].trim_end_matches(['\r', '\n']);
            let tail = &text[i..];
            // tail starts with the blank line separator; keep its original
            // newline style intact.
            Some(format!("{head}\r\n{attr}\n{tail}"))
        }
        None => {
            let head = text.trim_end_matches(['\r', '\n']);
            if head.is_empty() {
                // Empty/absent manifest: just emit ours.
                Some(format!("{attr}\r\n"))
            } else {
                Some(format!("{head}\r\n{attr}\r\n"))
            }
        }
    }
}

/// Ensure the patched-client JAR carries `Minecraft-Dists: client` in its
/// MANIFEST.MF main section (v26.4-alpha.6).
///
/// NeoForge 26.x dist resolution expects this attribute on the patched
/// client produced by the official installer pipeline, but the installer
/// does not add it - until now this was a manual JAR-editing step after
/// every install. Rewrites the archive in place (via a temp file + rename)
/// and returns whether a change was made.
pub fn ensure_patched_client_dist_attribute(jar: &Path) -> Result<bool> {
    const ATTR_KEY: &str = "Minecraft-Dists";
    const ATTR_VALUE: &str = "client";

    let src_file = std::fs::File::open(jar)
        .with_context(|| format!("Failed to open patched client JAR {:?}", jar))?;
    let mut archive = zip::ZipArchive::new(src_file)
        .with_context(|| format!("Failed to read zip structure of {:?}", jar))?;

    let original = {
        let mut entry = archive
            .by_name("META-INF/MANIFEST.MF")
            .with_context(|| format!("{:?} has no META-INF/MANIFEST.MF", jar))?;
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut entry, &mut buf)?;
        buf
    };

    let merged = match merge_manifest_attribute(&original, ATTR_KEY, ATTR_VALUE) {
        Some(m) => m,
        None => return Ok(false),
    };

    let tmp_path = jar.with_extension("jar.mdl-tmp");
    {
        let out_file = std::fs::File::create(&tmp_path)
            .with_context(|| format!("Failed to create temp JAR {:?}", tmp_path))?;
        let mut out = zip::ZipWriter::new(out_file);
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            if name == "META-INF/MANIFEST.MF" {
                out.start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )?;
                std::io::Write::write_all(&mut out, merged.as_bytes())?;
            } else {
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(entry.compression());
                out.start_file(name, options)?;
                std::io::copy(&mut entry, &mut out)?;
            }
        }
        out.finish()?;
    }

    std::fs::rename(&tmp_path, jar)
        .with_context(|| format!("Failed to replace {:?} with rewritten JAR", jar))?;
    Ok(true)
}

/// Post-install hook: inject `Minecraft-Dists: client` into the 26.x
/// patched client. Best-effort by design - failures are logged, never
/// propagated, because installs with older pipelines remain launchable.
pub async fn post_install_manifest_fix(base_dir: &Path, neoforge_version: &str) {
    // Scope: 26.x era only, matching the dist-resolver requirement.
    if !neoforge_version.starts_with("26.") {
        return;
    }
    let patched = base_dir
        .join("libraries")
        .join("net")
        .join("neoforged")
        .join("minecraft-client-patched")
        .join(neoforge_version)
        .join(format!("minecraft-client-patched-{neoforge_version}.jar"));
    if !patched.exists() {
        tracing::warn!(
            "Patched client not found at {:?}; skipping Minecraft-Dists injection",
            patched
        );
        return;
    }
    let patched_clone = patched.clone();
    let result = tokio::task::spawn_blocking(move || {
        ensure_patched_client_dist_attribute(&patched_clone)
    })
    .await;
    match result {
        Ok(Ok(true)) => tracing::info!(
            "Injected 'Minecraft-Dists: client' into {}",
            patched.display()
        ),
        Ok(Ok(false)) => tracing::debug!(
            "{} already carries Minecraft-Dists",
            patched.display()
        ),
        Ok(Err(e)) => tracing::warn!(
            "Minecraft-Dists injection failed (install continues): {e:#}"
        ),
        Err(e) => tracing::warn!("Minecraft-Dists injection task failed: {e}"),
    }
}

/// NeoForge-specific version metadata (extends Minecraft's format)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NeoForgeVersionMetadata {
    id: String,
    #[serde(rename = "inheritsFrom")]
    inherits_from: String,
    #[serde(rename = "mainClass")]
    main_class: String,
    libraries: Vec<crate::version::manifest::Library>,
    #[serde(default)]
    arguments: Option<crate::version::manifest::GameArguments>,
    #[serde(default, rename = "minecraftArguments")]
    minecraft_arguments: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Metadata {
    #[serde(rename = "groupId")]
    group_id: String,
    #[serde(rename = "artifactId")]
    artifact_id: String,
    versioning: Versioning,
}

#[derive(Debug, Deserialize, Serialize)]
struct Versioning {
    latest: String,
    release: String,
    versions: Versions,
}

#[derive(Debug, Deserialize, Serialize)]
struct Versions {
    #[serde(rename = "version", default)]
    versions: Vec<String>,
}

pub struct NeoForgeInstaller {
    version: Option<String>,
}

impl NeoForgeInstaller {
    pub fn new(version: Option<String>) -> Self {
        Self { version }
    }

    /// Compare two NeoForge version strings for sorting (semantic versioning).
    /// Returns Ordering for use with sort_by.
    /// Examples: "21.10.64" > "21.10.0-beta", "21.10.10" > "21.10.9"
    fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
        // Split version into numeric parts and pre-release suffix
        let parse_version = |v: &str| -> (Vec<u32>, Option<String>) {
            let (numeric, prerelease) = if let Some(pos) = v.find('-') {
                (&v[..pos], Some(v[pos + 1..].to_string()))
            } else {
                (v, None)
            };

            let parts: Vec<u32> = numeric
                .split('.')
                .filter_map(|s| s.parse::<u32>().ok())
                .collect();

            (parts, prerelease)
        };

        let (a_parts, a_pre) = parse_version(a);
        let (b_parts, b_pre) = parse_version(b);

        // Compare numeric parts
        for (a_num, b_num) in a_parts.iter().zip(b_parts.iter()) {
            match a_num.cmp(b_num) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        }

        // If all common parts are equal, longer version is greater
        match a_parts.len().cmp(&b_parts.len()) {
            std::cmp::Ordering::Equal => {},
            other => return other,
        }

        // Numeric parts are equal; compare pre-release.
        // Stable (no pre-release) > any pre-release
        match (a_pre, b_pre) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Greater, // stable > pre-release
            (Some(_), None) => std::cmp::Ordering::Less,
            (Some(a_pr), Some(b_pr)) => a_pr.cmp(&b_pr), // lexicographic
        }
    }

    pub async fn fetch_versions(mc_version: &str) -> Result<Vec<String>> {
        let response = reqwest::get(NEOFORGE_METADATA_URL)
            .await
            .context("Failed to fetch NeoForge version metadata")?;

        let text = response.text().await?;
        let metadata: Metadata = serde_xml_rs::from_str(&text)
            .context("Failed to parse NeoForge metadata XML")?;

        // Extract MC version from MC format (e.g., "1.21.1" -> "21.1")
        let mc_parts: Vec<&str> = mc_version.split('.').collect();
        if mc_parts.len() < 2 {
            anyhow::bail!("Invalid Minecraft version format: {}", mc_version);
        }

        let neoforge_prefix = if mc_parts.len() == 3 && mc_parts[2] != "0" {
            // MC 1.21.1 -> NeoForge 21.1.x
            format!("{}.{}.", &mc_parts[1], &mc_parts[2])
        } else {
            // MC 1.21 or 1.21.0 -> NeoForge 21.0.x
            format!("{}.0.", &mc_parts[1])
        };

        let mut versions: Vec<String> = metadata.versioning.versions.versions
            .into_iter()
            .filter(|v| v.starts_with(&neoforge_prefix))
            .collect();

        // Sort versions in descending order (newest first) using semantic versioning.
        // This ensures "21.10.64" comes before "21.10.0-beta" when selecting "latest".
        versions.sort_by(|a, b| Self::compare_versions(b, a));

        Ok(versions)
    }

    fn get_installer_url(neoforge_version: &str) -> String {
        format!(
            "{}/{}/neoforge-{}-installer.jar",
            NEOFORGE_MAVEN_URL, neoforge_version, neoforge_version
        )
    }

    async fn download_installer(&self, neoforge_version: &str, cache_dir: &Path) -> Result<PathBuf> {
        let installer_url = Self::get_installer_url(neoforge_version);
        let installer_filename = format!("neoforge-{}-installer.jar", neoforge_version);
        let installer_path = cache_dir.join(&installer_filename);

        if installer_path.exists() {
            tracing::debug!("NeoForge installer already cached: {}", installer_filename);
            return Ok(installer_path);
        }

        fs::create_dir_all(cache_dir).await?;

        tracing::info!("Downloading NeoForge installer: {}", installer_url);
        let response = reqwest::get(&installer_url)
            .await
            .with_context(|| format!("Failed to download NeoForge installer from {}", installer_url))?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to download NeoForge installer: HTTP {}", response.status());
        }

        let bytes = response.bytes().await?;
        fs::write(&installer_path, bytes).await?;

        Ok(installer_path)
    }

    async fn extract_version_json(&self, installer_path: &Path) -> Result<NeoForgeVersionMetadata> {
        let file = std::fs::File::open(installer_path)
            .with_context(|| format!("Failed to open installer: {:?}", installer_path))?;

        let mut archive = zip::ZipArchive::new(file)
            .context("Failed to read installer JAR")?;

        let mut version_json_file = archive.by_name("version.json")
            .context("version.json not found in installer")?;

        let mut contents = String::new();
        std::io::Read::read_to_string(&mut version_json_file, &mut contents)?;

        let version_metadata: NeoForgeVersionMetadata = serde_json::from_str(&contents)
            .context("Failed to parse version.json")?;

        Ok(version_metadata)
    }

    fn library_name_to_path(library_name: &str) -> Result<String> {
        let parts: Vec<&str> = library_name.split(':').collect();
        if parts.len() < 3 {
            anyhow::bail!("Invalid library name format: {}", library_name);
        }

        let (group, artifact) = (parts[0], parts[1]);
        // Strip @jar or @type suffix from version (Maven packaging type notation)
        let version = parts[2].split('@').next().unwrap_or(parts[2]);

        // Handle classifier with @type (e.g., "artifact:version:classifier@jar")
        let classifier = if parts.len() > 3 {
            let classifier_str = parts[3].split('@').next().unwrap_or(parts[3]);
            format!("-{}", classifier_str)
        } else {
            String::new()
        };

        let group_path = group.replace('.', "/");
        let jar_name = format!("{}-{}{}.jar", artifact, version, classifier);

        Ok(format!("{}/{}/{}/{}", group_path, artifact, version, jar_name))
    }

    async fn download_library(&self, library: &crate::version::manifest::Library, libraries_dir: &Path) -> Result<PathBuf> {
        // Skip libraries without downloads
        let downloads = match &library.downloads {
            Some(d) => d,
            None => return Ok(PathBuf::new()),
        };

        // Skip libraries without artifacts
        let artifact = match &downloads.artifact {
            Some(a) => a,
            None => return Ok(PathBuf::new()),
        };

        // Skip empty URLs (these are generated client-side)
        if artifact.url.is_empty() {
            tracing::debug!("Skipping library with empty URL: {}", library.name);
            return Ok(PathBuf::new());
        }

        // Calculate path from library name
        let relative_path = Self::library_name_to_path(&library.name)?;
        let target_path = libraries_dir.join(&relative_path);

        if target_path.exists() {
            tracing::debug!("Library already exists: {}", library.name);
            return Ok(target_path);
        }

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        tracing::info!("Downloading library: {} from {}", library.name, artifact.url);

        let response = reqwest::get(&artifact.url)
            .await
            .with_context(|| format!("Failed to download library: {}", library.name))?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to download library {}: HTTP {}", library.name, response.status());
        }

        let bytes = response.bytes().await?;

        if !crate::util::checksum::verify_sha1(&bytes, &artifact.sha1) {
            anyhow::bail!("SHA1 verification failed for library: {}", library.name);
        }

        fs::write(&target_path, bytes).await?;

        Ok(target_path)
    }

    async fn download_neoforge_jar(&self, neoforge_version: &str, libraries_dir: &Path) -> Result<PathBuf> {
        // NeoForge main JAR follows Maven naming: net.neoforged:neoforge:VERSION:universal
        let neoforge_jar_name = format!("neoforge-{}-universal.jar", neoforge_version);
        let neoforge_jar_url = format!(
            "{}/{}/{}",
            NEOFORGE_MAVEN_URL, neoforge_version, neoforge_jar_name
        );

        // Maven path: net/neoforged/neoforge/VERSION/neoforge-VERSION-universal.jar
        let target_path = libraries_dir
            .join("net")
            .join("neoforged")
            .join("neoforge")
            .join(neoforge_version)
            .join(&neoforge_jar_name);

        if target_path.exists() {
            tracing::debug!("NeoForge main JAR already exists: {}", neoforge_jar_name);
            return Ok(target_path);
        }

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        tracing::info!("Downloading NeoForge main JAR: {}", neoforge_jar_url);

        let response = reqwest::get(&neoforge_jar_url)
            .await
            .with_context(|| format!("Failed to download NeoForge JAR from {}", neoforge_jar_url))?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to download NeoForge JAR: HTTP {}", response.status());
        }

        let bytes = response.bytes().await?;
        fs::write(&target_path, bytes).await?;

        Ok(target_path)
    }

    pub async fn install_loader(&self, _mc_version: &str, neoforge_version: &str, target_dir: &Path) -> Result<String> {
        // The base cache directory. Its `libraries` subdirectory IS our shared
        // library cache (see util::paths). By pointing the official installer's
        // `--install-client` at this directory, everything it produces (patched
        // client, extra jar, all loader/MC libraries) lands directly in our
        // library cache at the correct Maven coordinates.
        let base_dir = crate::util::paths::get_cache_dir()?;
        let installer_cache = base_dir.join("neoforge");

        fs::create_dir_all(&base_dir).await?;

        tracing::info!("Installing NeoForge {}", neoforge_version);

        let installer_path = self.download_installer(neoforge_version, &installer_cache).await?;

        // NeoForge (like Forge) requires a real deobfuscated + binary-patched
        // client JAR that is produced by a multi-step processor pipeline
        // (jarsplitter -> AutoRenamingTool -> binarypatcher). Reimplementing that
        // pipeline is fragile; the official installer already runs it headlessly
        // via `--install-client`. Delegate to it.
        self.run_official_installer(&installer_path, &base_dir).await?;

        // The installer writes the launch profile to:
        //   <base_dir>/versions/neoforge-<version>/neoforge-<version>.json
        let neoforge_id = format!("neoforge-{}", neoforge_version);
        let generated_json = base_dir
            .join("versions")
            .join(&neoforge_id)
            .join(format!("{}.json", neoforge_id));

        if !generated_json.exists() {
            anyhow::bail!(
                "NeoForge installer did not produce the expected profile: {:?}",
                generated_json
            );
        }

        // v26.4-alpha.6: the 26.x patched client needs `Minecraft-Dists:
        // client` in its MANIFEST for dist resolution; the official installer
        // omits it (previously a manual OpenLumin step). Best-effort.
        post_install_manifest_fix(&base_dir, neoforge_version).await;

        // Copy the installer-produced version.json into the instance so the
        // launcher can read the authoritative JVM args, game args and libraries.
        let version_json_content = fs::read_to_string(&generated_json)
            .await
            .with_context(|| format!("Failed to read generated profile: {:?}", generated_json))?;
        let version_json_path = target_dir.join("version.json");
        fs::write(&version_json_path, &version_json_content).await?;

        Ok(neoforge_id)
    }

    /// Run the official NeoForge installer in headless client-install mode.
    /// This executes the full processor pipeline that produces the patched
    /// client and extracts all libraries into the shared library cache.
    async fn run_official_installer(&self, installer_path: &Path, install_dir: &Path) -> Result<()> {
        use std::process::Command;

        // The installer refuses to run without a launcher_profiles.json in the
        // target directory (it injects a profile entry there on success).
        let profiles_path = install_dir.join("launcher_profiles.json");
        if !profiles_path.exists() {
            fs::write(
                &profiles_path,
                r#"{"profiles":{},"selectedProfile":"","clientToken":""}"#,
            )
            .await?;
        }

        // Locate a Java runtime to run the installer with, auto-downloading one
        // if the system has none. Modern NeoForge (MC 1.20.5+) targets Java 21,
        // and the installer itself runs cleanly on 21, so require that.
        let java = crate::version::java::JavaRuntime::ensure_version(21)
            .await
            .context("Failed to obtain a Java runtime for the NeoForge installer")?;

        tracing::info!("Running official NeoForge installer (this may take a minute)...");

        let installer_path = installer_path.to_path_buf();
        let install_dir = install_dir.to_path_buf();
        let java_path = java.path.clone();

        // The installer is a blocking, network-bound Java process. Run it on a
        // blocking thread so we don't stall the async runtime.
        let output = tokio::task::spawn_blocking(move || {
            Command::new(&java_path)
                .arg("-jar")
                .arg(&installer_path)
                .arg("--install-client")
                .arg(&install_dir)
                .output()
        })
        .await?
        .context("Failed to execute NeoForge installer")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            anyhow::bail!(
                "NeoForge installer failed (exit {:?}).\nstdout:\n{}\nstderr:\n{}",
                output.status.code(),
                stdout,
                stderr
            );
        }

        tracing::info!("NeoForge installer completed successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        // Stable versions should be greater than pre-releases
        assert_eq!(
            NeoForgeInstaller::compare_versions("21.10.64", "21.10.0-beta"),
            std::cmp::Ordering::Greater
        );

        // Higher numeric versions
        assert_eq!(
            NeoForgeInstaller::compare_versions("21.10.64", "21.10.10"),
            std::cmp::Ordering::Greater
        );

        assert_eq!(
            NeoForgeInstaller::compare_versions("21.11.0", "21.10.99"),
            std::cmp::Ordering::Greater
        );

        // Equal versions
        assert_eq!(
            NeoForgeInstaller::compare_versions("21.10.0", "21.10.0"),
            std::cmp::Ordering::Equal
        );

        // Pre-release comparison
        assert_eq!(
            NeoForgeInstaller::compare_versions("21.10.0-beta", "21.10.0-alpha"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn test_version_sorting() {
        let mut versions = vec![
            "21.10.0-beta".to_string(),
            "21.10.64".to_string(),
            "21.10.10".to_string(),
            "21.10.0-alpha".to_string(),
            "21.11.0".to_string(),
        ];

        versions.sort_by(|a, b| NeoForgeInstaller::compare_versions(b, a));

        // Should be sorted: 21.11.0, 21.10.64, 21.10.10, 21.10.0-beta, 21.10.0-alpha
        assert_eq!(versions[0], "21.11.0");
        assert_eq!(versions[1], "21.10.64");
        assert_eq!(versions[2], "21.10.10");
        assert_eq!(versions[3], "21.10.0-beta");
        assert_eq!(versions[4], "21.10.0-alpha");
    }

    // ------------------------------------------------------------------
    // v26.4-alpha.6: MANIFEST Minecraft-Dists injection
    // ------------------------------------------------------------------

    #[test]
    fn test_merge_manifest_appends_to_main_section() {
        let src = "Manifest-Version: 1.0\r\nImplementation-Title: patched\r\n";
        let out = merge_manifest_attribute(src, "Minecraft-Dists", "client").unwrap();
        assert!(out.contains("Minecraft-Dists: client"));
        assert!(out.starts_with("Manifest-Version: 1.0\r\nImplementation-Title: patched"));
    }

    #[test]
    fn test_merge_manifest_preserves_named_sections() {
        let src = "Manifest-Version: 1.0\r\n\r\nName: net/minecraft/client/Blah.class\r\nSHA-Digest: abc\r\n";
        let out = merge_manifest_attribute(src, "Minecraft-Dists", "client").unwrap();
        // Attribute must land in the MAIN section (before the blank line).
        let main_end = out.find("\r\n\r\n").unwrap();
        assert!(out[..main_end].contains("Minecraft-Dists: client"));
        // Named section must survive untouched after the separator.
        assert!(out[main_end..].contains("Name: net/minecraft/client/Blah.class"));
    }

    #[test]
    fn test_merge_manifest_skips_when_present() {
        let src = "Manifest-Version: 1.0\r\nMinecraft-Dists: client\r\n";
        assert!(merge_manifest_attribute(src, "Minecraft-Dists", "client").is_none());
        // Case-insensitive detection.
        let src2 = "Manifest-Version: 1.0\r\nminecraft-dists: client\r\n";
        assert!(merge_manifest_attribute(src2, "Minecraft-Dists", "client").is_none());
    }

    #[test]
    fn test_merge_manifest_handles_lf_only_and_empty() {
        let lf = "Manifest-Version: 1.0\n";
        let out = merge_manifest_attribute(lf, "Minecraft-Dists", "client").unwrap();
        assert!(out.contains("Minecraft-Dists: client"));

        let out_empty = merge_manifest_attribute("", "Minecraft-Dists", "client").unwrap();
        assert_eq!(out_empty, "Minecraft-Dists: client\r\n");
    }

    #[test]
    fn test_patched_client_jar_roundtrip() {
        use std::io::Write as _;

        let dir = tempfile::TempDir::new().unwrap();
        let jar_path = dir.path().join("minecraft-client-patched-26.2.0.jar");

        // Build a tiny stand-in JAR with a manifest and two entries.
        {
            let file = std::fs::File::create(&jar_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("META-INF/MANIFEST.MF", opts).unwrap();
            zip.write_all(b"Manifest-Version: 1.0\r\nImplementation-Title: t\r\n").unwrap();
            zip.start_file("a.class", opts).unwrap();
            zip.write_all(b"fake class bytes").unwrap();
            zip.start_file("assets/x.png", opts).unwrap();
            zip.write_all(&[0u8, 1, 2, 3]).unwrap();
            zip.finish().unwrap();
        }

        // First pass injects; second pass is a no-op.
        assert!(ensure_patched_client_dist_attribute(&jar_path).unwrap());
        assert!(!ensure_patched_client_dist_attribute(&jar_path).unwrap());

        // Rewritten JAR stays structurally valid and keeps its entries.
        let f = std::fs::File::open(&jar_path).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        assert_eq!(z.len(), 3);
        let mut mf = String::new();
        use std::io::Read as _;
        z.by_name("META-INF/MANIFEST.MF").unwrap().read_to_string(&mut mf).unwrap();
        assert!(mf.contains("Minecraft-Dists: client"));
        assert!(mf.contains("Implementation-Title: t"));
        let mut bytes = Vec::new();
        z.by_name("a.class").unwrap().read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"fake class bytes");

        // No temp file left behind.
        assert!(!jar_path.with_extension("jar.mdl-tmp").exists());
    }
}
