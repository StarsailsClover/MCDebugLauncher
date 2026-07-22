// NeoForge mod loader installer

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

const NEOFORGE_MAVEN_URL: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const NEOFORGE_METADATA_URL: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";

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

        let versions: Vec<String> = metadata.versioning.versions.versions
            .into_iter()
            .filter(|v| v.starts_with(&neoforge_prefix))
            .collect();

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
