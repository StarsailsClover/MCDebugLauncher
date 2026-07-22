// Forge mod loader installer

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

const FORGE_MAVEN_URL: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
const FORGE_METADATA_URL: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";

/// Forge-specific version metadata (extends Minecraft's format)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ForgeVersionMetadata {
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

pub struct ForgeInstaller {
    version: Option<String>,
}

impl ForgeInstaller {
    pub fn new(version: Option<String>) -> Self {
        Self { version }
    }

    pub async fn fetch_versions(mc_version: &str) -> Result<Vec<String>> {
        let response = reqwest::get(FORGE_METADATA_URL)
            .await
            .context("Failed to fetch Forge version metadata")?;

        let text = response.text().await?;
        let metadata: Metadata = serde_xml_rs::from_str(&text)
            .context("Failed to parse Forge metadata XML")?;

        let versions: Vec<String> = metadata.versioning.versions.versions
            .into_iter()
            .filter(|v| v.starts_with(&format!("{}-", mc_version)))
            .collect();

        Ok(versions)
    }

    fn get_installer_url(mc_version: &str, forge_version: &str) -> String {
        format!(
            "{}/{}-{}/forge-{}-{}-installer.jar",
            FORGE_MAVEN_URL, mc_version, forge_version, mc_version, forge_version
        )
    }

    async fn download_installer(&self, mc_version: &str, forge_version: &str, cache_dir: &Path) -> Result<PathBuf> {
        let installer_url = Self::get_installer_url(mc_version, forge_version);
        let installer_filename = format!("forge-{}-{}-installer.jar", mc_version, forge_version);
        let installer_path = cache_dir.join(&installer_filename);

        if installer_path.exists() {
            tracing::debug!("Forge installer already cached: {}", installer_filename);
            return Ok(installer_path);
        }

        fs::create_dir_all(cache_dir).await?;

        tracing::info!("Downloading Forge installer: {}", installer_url);
        let response = reqwest::get(&installer_url)
            .await
            .with_context(|| format!("Failed to download Forge installer from {}", installer_url))?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to download Forge installer: HTTP {}", response.status());
        }

        let bytes = response.bytes().await?;
        fs::write(&installer_path, bytes).await?;

        Ok(installer_path)
    }

    async fn extract_version_json(&self, installer_path: &Path) -> Result<ForgeVersionMetadata> {
        let file = std::fs::File::open(installer_path)
            .with_context(|| format!("Failed to open installer: {:?}", installer_path))?;

        let mut archive = zip::ZipArchive::new(file)
            .context("Failed to read installer JAR")?;

        let mut version_json_file = archive.by_name("version.json")
            .context("version.json not found in installer")?;

        let mut contents = String::new();
        std::io::Read::read_to_string(&mut version_json_file, &mut contents)?;

        let version_metadata: ForgeVersionMetadata = serde_json::from_str(&contents)
            .context("Failed to parse version.json")?;

        Ok(version_metadata)
    }

    fn library_name_to_path(library_name: &str) -> Result<String> {
        let parts: Vec<&str> = library_name.split(':').collect();
        if parts.len() < 3 {
            anyhow::bail!("Invalid library name format: {}", library_name);
        }

        let (group, artifact, version) = (parts[0], parts[1], parts[2]);
        let classifier = if parts.len() > 3 {
            format!("-{}", parts[3])
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

    pub async fn install_loader(&self, mc_version: &str, forge_version: &str, target_dir: &Path) -> Result<String> {
        let cache_dir = crate::util::paths::get_cache_dir()?.join("forge");
        let libraries_dir = crate::util::paths::get_libraries_cache_dir()?;

        fs::create_dir_all(&libraries_dir).await?;

        tracing::info!("Installing Forge {}-{}", mc_version, forge_version);

        let installer_path = self.download_installer(mc_version, forge_version, &cache_dir).await?;
        let version_metadata = self.extract_version_json(&installer_path).await?;

        tracing::info!("Downloading Forge libraries...");
        for library in &version_metadata.libraries {
            self.download_library(library, &libraries_dir).await?;
        }

        let version_json_path = target_dir.join("version.json");
        let version_json = serde_json::to_string_pretty(&version_metadata)?;
        fs::write(&version_json_path, version_json).await?;

        Ok(version_metadata.id)
    }
}

impl crate::loader::LoaderInstaller for ForgeInstaller {
    fn install(&self, mc_version: &str, target_dir: &str) -> Result<String> {
        let target_path = Path::new(target_dir);

        let forge_version = if let Some(v) = &self.version {
            v.clone()
        } else {
            let runtime = tokio::runtime::Handle::current();
            runtime.block_on(async {
                let versions = Self::fetch_versions(mc_version).await?;
                versions
                    .first()
                    .cloned()
                    .with_context(|| format!("No Forge version found for Minecraft {}", mc_version))
                    .map(|v| {
                        v.strip_prefix(&format!("{}-", mc_version))
                            .unwrap_or(&v)
                            .to_string()
                    })
            })?
        };

        let runtime = tokio::runtime::Handle::current();
        runtime.block_on(async {
            self.install_loader(mc_version, &forge_version, target_path).await
        })
    }

    fn version(&self) -> &str {
        self.version.as_deref().unwrap_or("latest")
    }

    fn loader_type(&self) -> &str {
        "forge"
    }
}
