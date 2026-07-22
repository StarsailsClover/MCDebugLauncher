// Instance launcher

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tokio::fs;

use crate::instance::config::InstanceConfig;
use crate::version::manifest::VersionMetadata;

#[derive(Debug, Deserialize)]
struct VersionJson {
    id: String,
    #[serde(rename = "inheritsFrom")]
    inherits_from: Option<String>,
    #[serde(rename = "mainClass")]
    main_class: String,
    #[serde(rename = "minecraftArguments")]
    minecraft_arguments: Option<String>,
    arguments: Option<Arguments>,
    libraries: Vec<Library>,
}

#[derive(Debug, Deserialize)]
struct Arguments {
    game: Option<Vec<ArgumentValue>>,
    jvm: Option<Vec<ArgumentValue>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ArgumentValue {
    String(String),
    Object {
        rules: Vec<Rule>,
        value: ArgumentValueInner,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ArgumentValueInner {
    String(String),
    Array(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct Rule {
    action: String,
    os: Option<OsRule>,
    features: Option<HashMap<String, bool>>,
}

#[derive(Debug, Deserialize)]
struct OsRule {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Library {
    name: String,
    downloads: Option<LibraryDownloads>,
    url: Option<String>,
    rules: Option<Vec<Rule>>,
}

#[derive(Debug, Deserialize)]
struct LibraryDownloads {
    artifact: Option<Artifact>,
}

#[derive(Debug, Deserialize)]
struct Artifact {
    url: String,
    sha1: String,
    size: u64,
}

pub struct InstanceLauncher {
    java_path: PathBuf,
}

impl InstanceLauncher {
    pub fn new() -> Result<Self> {
        let java_runtime = crate::version::java::JavaRuntime::detect()
            .context("Failed to detect Java installation")?;
        Ok(Self { java_path: java_runtime.path })
    }

    pub async fn launch(&self, name: &str) -> Result<()> {
        let instances_dir = crate::util::paths::get_instances_dir()?;
        let instance_dir = instances_dir.join(name);

        if !instance_dir.exists() {
            anyhow::bail!("Instance '{}' does not exist", name);
        }

        // Load instance config
        let config_path = instance_dir.join("instance.json");
        let config_data = fs::read_to_string(&config_path).await?;
        let config: InstanceConfig = serde_json::from_str(&config_data)?;

        tracing::info!("Launching instance '{}'...", name);
        tracing::info!("Building classpath and downloading libraries...");

        // Build classpath and get main class
        let version_dir = instance_dir.join("versions").join(&config.version);
        let (classpath, main_class) = self.build_classpath(&version_dir, &config).await?;

        // Prepare game arguments
        let game_dir = instance_dir.clone();
        let assets_dir = crate::util::paths::get_data_dir()?.join("assets");
        let natives_dir = version_dir.join("natives");

        fs::create_dir_all(&game_dir).await?;
        fs::create_dir_all(&assets_dir).await?;
        fs::create_dir_all(&natives_dir).await?;

        let version_metadata = self.load_version_metadata(&version_dir, &config.version).await?;

        // Build launch command
        let mut cmd = Command::new(&self.java_path);

        // JVM arguments
        cmd.arg("-Xmx2G");
        cmd.arg("-Xms512M");
        cmd.arg(format!("-Djava.library.path={}", natives_dir.display()));
        cmd.arg("-cp");
        cmd.arg(&classpath);

        // Main class
        cmd.arg(&main_class);

        // Game arguments
        self.add_game_arguments(&mut cmd, &config, &version_metadata, &game_dir, &assets_dir, &main_class, &version_dir)?;

        cmd.current_dir(&game_dir);
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());

        tracing::info!("Starting Minecraft...");
        tracing::debug!("Command: {:?}", cmd);

        let mut child = cmd.spawn().context("Failed to spawn Minecraft process")?;
        let status = child.wait().context("Failed to wait for Minecraft process")?;

        if !status.success() {
            anyhow::bail!("Minecraft exited with code: {:?}", status.code());
        }

        tracing::info!("Minecraft closed successfully");
        Ok(())
    }

    async fn build_classpath(&self, version_dir: &Path, config: &InstanceConfig) -> Result<(String, String)> {
        let libraries_dir = crate::util::paths::get_libraries_cache_dir()?;
        let mut classpath_entries = Vec::new();
        let mut main_class = String::new();

        // Add Minecraft client jar FIRST (required by Forge ModLauncher)
        let client_jar = version_dir.join(format!("{}.jar", config.version));
        classpath_entries.push(client_jar.display().to_string());

        // Load version.json if loader is present
        if config.loader.is_some() {
            let loader_json_path = version_dir.join("version.json");
            if loader_json_path.exists() {
                let loader_json_data = fs::read_to_string(&loader_json_path).await?;
                let loader_json: VersionJson = serde_json::from_str(&loader_json_data)?;

                main_class = loader_json.main_class.clone();

                // Add loader libraries
                for library in &loader_json.libraries {
                    // Check for Forge client library with empty URL
                    if library.name.contains(":client") {
                        if let Some(downloads) = &library.downloads {
                            if let Some(artifact) = &downloads.artifact {
                                if artifact.url.is_empty() {
                                    // This is Forge's client JAR - copy Minecraft client to Maven path
                                    let target_path = self.get_library_path_from_name(&library.name, &libraries_dir);
                                    if !target_path.exists() {
                                        tracing::info!("Copying Minecraft client JAR to Forge Maven path");
                                        if let Some(parent) = target_path.parent() {
                                            fs::create_dir_all(parent).await?;
                                        }
                                        fs::copy(&client_jar, &target_path).await?;
                                    }
                                    classpath_entries.push(target_path.display().to_string());
                                    continue;
                                }
                            }
                        }
                    }

                    if let Some(lib_path) = self.resolve_library_path(&library.name, &libraries_dir) {
                        classpath_entries.push(lib_path);
                    }
                }
            }
        }

        // Load base Minecraft version json
        let base_json_path = version_dir.join(format!("{}.json", config.version));
        let base_json_data = fs::read_to_string(&base_json_path).await?;
        let base_json: VersionJson = serde_json::from_str(&base_json_data)?;

        // Use base main class if no loader
        if main_class.is_empty() {
            main_class = base_json.main_class.clone();
        }

        // Prepare natives directory
        let natives_dir = version_dir.join("natives");
        fs::create_dir_all(&natives_dir).await?;

        // Add Minecraft libraries
        for library in &base_json.libraries {
            // Check rules
            if let Some(rules) = &library.rules {
                if !self.check_rules(rules) {
                    continue;
                }
            }

            // Check if this is a native library (contains :natives- in name)
            let is_native = library.name.contains(":natives-");

            // Download library if missing
            if let Some(downloads) = &library.downloads {
                if let Some(artifact) = &downloads.artifact {
                    let lib_path = self.get_library_path_from_name(&library.name, &libraries_dir);

                    // Download if not exists
                    if !lib_path.exists() {
                        tracing::info!("Downloading library: {}", library.name);
                        self.download_library(&artifact.url, &lib_path, &artifact.sha1).await?;
                    }

                    // Extract native libraries
                    if is_native {
                        tracing::info!("Extracting native library: {}", library.name);
                        match self.extract_natives(&lib_path, &natives_dir).await {
                            Ok(_) => {},
                            Err(e) => {
                                tracing::warn!("Failed to extract natives from {:?}: {}, retrying download...", lib_path, e);
                                // File was deleted by extract_natives, re-download
                                self.download_library(&artifact.url, &lib_path, &artifact.sha1).await?;
                                // Try extracting again
                                self.extract_natives(&lib_path, &natives_dir).await?;
                            }
                        }
                    } else {
                        classpath_entries.push(lib_path.display().to_string());
                    }
                    continue;
                }
            }

            // Fallback to name-based resolution
            if !is_native {
                if let Some(lib_path) = self.resolve_library_path(&library.name, &libraries_dir) {
                    classpath_entries.push(lib_path);
                }
            }
        }

        let classpath = if cfg!(windows) {
            classpath_entries.join(";")
        } else {
            classpath_entries.join(":")
        };

        tracing::info!("Classpath built with {} entries", classpath_entries.len());
        tracing::info!("Main class: {}", main_class);

        Ok((classpath, main_class))
    }

    async fn download_library(&self, url: &str, path: &Path, expected_sha1: &str) -> Result<()> {
        use crate::version::downloader::download_file;

        // Create parent directory
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Retry up to 3 times with exponential backoff
        let mut last_error = None;
        for attempt in 0..3 {
            if attempt > 0 {
                let delay = std::time::Duration::from_millis(500 * 2_u64.pow(attempt - 1));
                tracing::debug!("Retrying download after {:?}", delay);
                tokio::time::sleep(delay).await;
            }

            match download_file(url, path, Some(expected_sha1)).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    last_error = Some(e);
                    tracing::warn!("Download attempt {} failed for {}: {}", attempt + 1, url, last_error.as_ref().unwrap());
                    // Delete partial file if it exists
                    let _ = fs::remove_file(path).await;
                }
            }
        }

        Err(last_error.unwrap())
    }

    fn get_library_path_from_name(&self, name: &str, libraries_dir: &Path) -> PathBuf {
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() < 3 {
            return libraries_dir.join(name);
        }

        let group = parts[0].replace('.', "/");
        let artifact = parts[1];
        let version = parts[2];

        // Handle natives (e.g., "org.lwjgl:lwjgl:3.3.1:natives-windows")
        let jar_name = if parts.len() > 3 {
            let classifier = parts[3];
            format!("{}-{}-{}.jar", artifact, version, classifier)
        } else {
            format!("{}-{}.jar", artifact, version)
        };

        libraries_dir
            .join(&group)
            .join(artifact)
            .join(version)
            .join(jar_name)
    }

    fn resolve_library_path(&self, name: &str, libraries_dir: &Path) -> Option<String> {
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() < 3 {
            return None;
        }

        let group = parts[0].replace('.', "/");
        let artifact = parts[1];
        let version = parts[2];

        let path = libraries_dir
            .join(&group)
            .join(artifact)
            .join(version)
            .join(format!("{}-{}.jar", artifact, version));

        if path.exists() {
            Some(path.display().to_string())
        } else {
            None
        }
    }

    fn check_rules(&self, rules: &[Rule]) -> bool {
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

    async fn load_version_metadata(&self, version_dir: &Path, version: &str) -> Result<VersionMetadata> {
        let json_path = version_dir.join(format!("{}.json", version));
        let json_data = fs::read_to_string(&json_path).await?;
        let metadata: VersionMetadata = serde_json::from_str(&json_data)?;
        Ok(metadata)
    }

    fn add_game_arguments(
        &self,
        cmd: &mut Command,
        config: &InstanceConfig,
        metadata: &VersionMetadata,
        game_dir: &Path,
        assets_dir: &Path,
        main_class: &str,
        version_dir: &Path,
    ) -> Result<()> {
        // Standard arguments
        cmd.arg("--username");
        cmd.arg("Player");
        cmd.arg("--version");
        cmd.arg(&config.version);
        cmd.arg("--gameDir");
        cmd.arg(game_dir.display().to_string());
        cmd.arg("--assetsDir");
        cmd.arg(assets_dir.display().to_string());

        if let Some(asset_index) = &metadata.asset_index {
            cmd.arg("--assetIndex");
            cmd.arg(&asset_index.id);
        } else if let Some(assets) = &metadata.assets {
            cmd.arg("--assetIndex");
            cmd.arg(assets);
        }

        cmd.arg("--uuid");
        cmd.arg("00000000-0000-0000-0000-000000000000");
        cmd.arg("--accessToken");
        cmd.arg("0");
        cmd.arg("--userType");
        cmd.arg("legacy");
        cmd.arg("--versionType");
        cmd.arg("release");

        // Add launchTarget and gameJar for Forge/NeoForge
        if main_class.contains("forge") || main_class.contains("neoforge") {
            cmd.arg("--launchTarget");
            cmd.arg("forge_client");

            // Forge ModLauncher needs to know where the client JAR is
            let client_jar = version_dir.join(format!("{}.jar", config.version));
            cmd.arg("--gameJar");
            cmd.arg(client_jar.display().to_string());
        }

        Ok(())
    }

    async fn extract_natives(&self, jar_path: &Path, natives_dir: &Path) -> Result<()> {
        use std::io::Read;

        // Try to open the zip file
        let file = match std::fs::File::open(jar_path) {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("Failed to open jar file {:?}: {}", jar_path, e);
                return Err(e.into());
            }
        };

        // Try to read the zip archive
        let mut archive = match zip::ZipArchive::new(file) {
            Ok(a) => a,
            Err(e) => {
                tracing::error!("Invalid zip archive {:?}: {}, deleting...", jar_path, e);
                // Delete the corrupted file so it can be re-downloaded
                let _ = std::fs::remove_file(jar_path);
                return Err(anyhow::anyhow!("Invalid zip archive: {}", e));
            }
        };

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let file_name = file.name().to_string();

            // Only extract .dll, .so, .dylib files
            if file_name.ends_with(".dll")
                || file_name.ends_with(".so")
                || file_name.ends_with(".dylib")
            {
                // Get just the filename without path
                if let Some(name) = std::path::Path::new(&file_name).file_name() {
                    let out_path = natives_dir.join(name);

                    // Skip if already exists
                    if out_path.exists() {
                        continue;
                    }

                    let mut out_file = std::fs::File::create(&out_path)?;
                    let mut buffer = Vec::new();
                    file.read_to_end(&mut buffer)?;
                    std::io::Write::write_all(&mut out_file, &buffer)?;

                    tracing::debug!("Extracted: {}", name.to_string_lossy());
                }
            }
        }

        Ok(())
    }
}
