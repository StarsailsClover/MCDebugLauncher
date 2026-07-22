// Fabric mod loader installer

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

const FABRIC_META_URL: &str = "https://meta.fabricmc.net/v2/versions";

#[derive(Debug, Deserialize, Serialize)]
pub struct FabricLoaderVersion {
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct FabricLoaderProfile {
    #[serde(rename = "launcherMeta")]
    launcher_meta: LauncherMeta,
}

#[derive(Debug, Deserialize, Serialize)]
struct LauncherMeta {
    version: u32,
    libraries: Libraries,
    #[serde(rename = "mainClass")]
    main_class: MainClass,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum MainClass {
    String(String),
    Object {
        client: String,
        server: String,
    },
}

#[derive(Debug, Deserialize, Serialize)]
struct Libraries {
    client: Vec<Library>,
    common: Vec<Library>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Library {
    name: String,
    url: String,
    sha1: Option<String>,
    size: Option<u64>,
}

pub struct FabricInstaller {
    version: Option<String>,
}

impl FabricInstaller {
    pub fn new(version: Option<String>) -> Self {
        Self { version }
    }

    pub async fn fetch_versions() -> Result<Vec<FabricLoaderVersion>> {
        let url = format!("{}/loader", FABRIC_META_URL);
        let response = reqwest::get(&url)
            .await
            .context("Failed to fetch Fabric loader versions")?;
        let versions = response
            .json::<Vec<FabricLoaderVersion>>()
            .await
            .context("Failed to parse Fabric loader versions")?;
        Ok(versions)
    }

    pub async fn fetch_profile(mc_version: &str, loader_version: &str) -> Result<FabricLoaderProfile> {
        let url = format!(
            "{}/loader/{}/{}",
            FABRIC_META_URL, mc_version, loader_version
        );
        let response = reqwest::get(&url)
            .await
            .with_context(|| format!("Failed to fetch Fabric profile for MC {} loader {}", mc_version, loader_version))?;
        let profile = response
            .json::<FabricLoaderProfile>()
            .await
            .context("Failed to parse Fabric profile")?;
        Ok(profile)
    }

    async fn download_library(&self, library: &Library, libraries_dir: &Path) -> Result<PathBuf> {
        let parts: Vec<&str> = library.name.split(':').collect();
        if parts.len() != 3 {
            anyhow::bail!("Invalid library name format: {}", library.name);
        }

        let (group, artifact, version) = (parts[0], parts[1], parts[2]);
        let group_path = group.replace('.', "/");
        let jar_name = format!("{}-{}.jar", artifact, version);
        let relative_path = format!("{}/{}/{}/{}", group_path, artifact, version, jar_name);

        let target_path = libraries_dir.join(&relative_path);

        if target_path.exists() {
            tracing::debug!("Library already exists: {}", library.name);
            return Ok(target_path);
        }

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let download_url = if library.url.is_empty() {
            format!("https://maven.fabricmc.net/{}", relative_path)
        } else {
            format!("{}/{}", library.url.trim_end_matches('/'), relative_path)
        };

        tracing::info!("Downloading library: {} from {}", library.name, download_url);

        let response = reqwest::get(&download_url)
            .await
            .with_context(|| format!("Failed to download library: {}", library.name))?;

        let bytes = response.bytes().await?;

        if let Some(expected_sha1) = &library.sha1 {
            if !crate::util::checksum::verify_sha1(&bytes, expected_sha1) {
                anyhow::bail!("SHA1 verification failed for library: {}", library.name);
            }
        }

        fs::write(&target_path, bytes).await?;

        Ok(target_path)
    }

    pub async fn install_loader(&self, mc_version: &str, loader_version: &str, target_dir: &Path) -> Result<String> {
        let profile = Self::fetch_profile(mc_version, loader_version).await?;

        let libraries_dir = crate::util::paths::get_libraries_cache_dir()?;
        fs::create_dir_all(&libraries_dir).await?;

        tracing::info!("Downloading Fabric libraries...");
        for library in profile.launcher_meta.libraries.common.iter() {
            self.download_library(library, &libraries_dir).await?;
        }

        for library in profile.launcher_meta.libraries.client.iter() {
            self.download_library(library, &libraries_dir).await?;
        }

        // Download fabric-loader JAR itself
        let loader_library = Library {
            name: format!("net.fabricmc:fabric-loader:{}", loader_version),
            url: String::from("https://maven.fabricmc.net/"),
            sha1: None,
            size: None,
        };
        self.download_library(&loader_library, &libraries_dir).await?;

        let version_json_path = target_dir.join("version.json");

        let main_class_str = match &profile.launcher_meta.main_class {
            MainClass::String(s) => s.clone(),
            MainClass::Object { client, .. } => client.clone(),
        };

        // Build libraries list: common + client + fabric-loader itself
        let mut libraries = profile.launcher_meta.libraries.common.iter()
            .chain(profile.launcher_meta.libraries.client.iter())
            .map(|lib| {
                serde_json::json!({
                    "name": lib.name,
                    "url": if lib.url.is_empty() { "https://maven.fabricmc.net/" } else { &lib.url }
                })
            })
            .collect::<Vec<_>>();

        // Add fabric-loader JAR
        libraries.push(serde_json::json!({
            "name": format!("net.fabricmc:fabric-loader:{}", loader_version),
            "url": "https://maven.fabricmc.net/"
        }));

        let version_json = serde_json::json!({
            "id": format!("fabric-loader-{}-{}", loader_version, mc_version),
            "inheritsFrom": mc_version,
            "type": "release",
            "mainClass": main_class_str,
            "libraries": libraries
        });

        fs::write(&version_json_path, serde_json::to_string_pretty(&version_json)?).await?;

        Ok(format!("fabric-loader-{}-{}", loader_version, mc_version))
    }
}

impl crate::loader::LoaderInstaller for FabricInstaller {
    fn install(&self, mc_version: &str, target_dir: &str) -> Result<String> {
        let target_path = Path::new(target_dir);

        let loader_version = if let Some(v) = &self.version {
            v.clone()
        } else {
            let runtime = tokio::runtime::Handle::current();
            runtime.block_on(async {
                let versions = Self::fetch_versions().await?;
                versions
                    .iter()
                    .find(|v| v.stable)
                    .map(|v| v.version.clone())
                    .context("No stable Fabric loader version found")
            })?
        };

        let runtime = tokio::runtime::Handle::current();
        runtime.block_on(async {
            self.install_loader(mc_version, &loader_version, target_path).await
        })
    }

    fn version(&self) -> &str {
        self.version.as_deref().unwrap_or("latest")
    }

    fn loader_type(&self) -> &str {
        "fabric"
    }
}
