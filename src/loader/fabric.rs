// Fabric mod loader installer

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

const FABRIC_META_URL: &str = "https://meta.fabricmc.net/v2/versions";
const MODRINTH_API_URL: &str = "https://api.modrinth.com/v2";
// Modrinth project ID for Fabric API
const FABRIC_API_PROJECT_ID: &str = "P7dR8mSH";

#[derive(Debug, Deserialize, Serialize)]
pub struct FabricLoaderVersion {
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct FabricLoaderProfile {
    #[serde(rename = "launcherMeta")]
    launcher_meta: LauncherMeta,
    /// Intermediary mappings library required at runtime. The Fabric meta API
    /// includes this separately from `launcherMeta.libraries`; omitting it
    /// causes `net.fabricmc:intermediary` to be absent from the classpath and
    /// the game to crash with ClassNotFoundException at startup.
    intermediary: FabricIntermediary,
}

/// Describes the intermediary mappings artifact returned by the Fabric meta API.
#[derive(Debug, Deserialize, Serialize)]
struct FabricIntermediary {
    /// Maven coordinate, e.g. "net.fabricmc:intermediary:1.21.4"
    maven: String,
    #[serde(default)]
    stable: bool,
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

#[derive(Debug, Deserialize)]
struct ModrinthVersion {
    version_number: String,
    files: Vec<ModrinthFile>,
}

#[derive(Debug, Deserialize)]
struct ModrinthFile {
    url: String,
    filename: String,
    primary: bool,
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

        // Download intermediary mappings. This library is listed separately in
        // the Fabric meta API response and is NOT included in launcherMeta.libraries.
        // Without it the game crashes with ClassNotFoundException for Fabric's
        // obfuscation-aware class loader at startup.
        let intermediary_library = Library {
            name: profile.intermediary.maven.clone(),
            url: String::from("https://maven.fabricmc.net/"),
            sha1: None,
            size: None,
        };
        self.download_library(&intermediary_library, &libraries_dir).await?;

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

        // Add intermediary mappings (required at runtime, not in launcherMeta.libraries)
        libraries.push(serde_json::json!({
            "name": profile.intermediary.maven.clone(),
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

    /// Download and install the latest Fabric API mod for `mc_version` into `mods_dir`.
    /// Queries Modrinth for the latest compatible release, downloads the primary JAR,
    /// and skips the download if the file is already present. Non-fatal: logs a warning
    /// on any failure rather than aborting the install.
    pub async fn install_fabric_api(mc_version: &str, mods_dir: &std::path::Path) -> Result<()> {
        // Modrinth versioned API: filter by game version and loader
        let url = format!(
            "{}/project/{}/version?game_versions=[\"{}\"]&loaders=[\"fabric\"]",
            MODRINTH_API_URL, FABRIC_API_PROJECT_ID, mc_version
        );

        let response = reqwest::get(&url)
            .await
            .context("Failed to query Modrinth for Fabric API versions")?;

        if !response.status().is_success() {
            anyhow::bail!("Modrinth API returned HTTP {}", response.status());
        }

        let versions: Vec<ModrinthVersion> = response
            .json()
            .await
            .context("Failed to parse Modrinth Fabric API versions")?;

        let latest = versions
            .first()
            .context("No Fabric API version found for this Minecraft version on Modrinth")?;

        // Prefer the primary file; fall back to the first file in the list.
        let file = latest
            .files
            .iter()
            .find(|f| f.primary)
            .or_else(|| latest.files.first())
            .context("Fabric API version has no downloadable files")?;

        fs::create_dir_all(mods_dir).await?;
        let target = mods_dir.join(&file.filename);

        if target.exists() {
            tracing::debug!("Fabric API already present: {}", file.filename);
            return Ok(());
        }

        tracing::info!("Downloading Fabric API {}...", latest.version_number);

        let dl = reqwest::get(&file.url)
            .await
            .with_context(|| format!("Failed to download Fabric API from {}", file.url))?;

        if !dl.status().is_success() {
            anyhow::bail!("Failed to download Fabric API: HTTP {}", dl.status());
        }

        let bytes = dl.bytes().await.context("Failed to read Fabric API response body")?;
        fs::write(&target, &bytes).await?;

        tracing::info!("Fabric API {} installed ({} bytes)", latest.version_number, bytes.len());
        Ok(())
    }
}
