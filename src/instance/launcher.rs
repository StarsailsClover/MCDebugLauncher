// Instance launcher

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tokio::fs;

use crate::instance::config::InstanceConfig;
use crate::version::manifest::VersionMetadata;

// ---------------------------------------------------------------------------
// Instance launch lock — cross-process queue via a PID lock file.
//
// Only one Minecraft instance should run at a time under MDL (the game is
// memory-heavy and many of its resources, like the log file, are not designed
// for concurrent access from multiple game processes). When a second `mdl
// launch` is issued while another is still running the launcher waits,
// printing a one-time notice, rather than refusing or silently overwriting.
// ---------------------------------------------------------------------------

/// Acquire the global launch lock, blocking (polling) if another MDL-managed
/// instance is already running. Returns the path to the lock file so the
/// caller can release it with `release_launch_lock`.
async fn acquire_launch_lock(no_queue: bool) -> Result<Option<PathBuf>> {
    let lock_path = crate::util::paths::get_data_dir()?.join("launching.lock");
    let mut waiting_logged = false;

    loop {
        // Check whether an existing lock belongs to a live process.
        if lock_path.exists() {
            if let Ok(content) = fs::read_to_string(&lock_path).await {
                if let Ok(pid) = content.trim().parse::<u32>() {
                    if is_pid_running(pid) {
                        if no_queue {
                            // Queueing disabled: launch in parallel without
                            // touching the existing lock.
                            tracing::info!(
                                "Another instance is running (PID {}), but --no-queue was                                  given - launching in parallel without queueing.",
                                pid
                            );
                            return Ok(None);
                        }
                        if !waiting_logged {
                            tracing::info!(
                                "Another instance is already running (PID {}).                                  Waiting for it to close...",
                                pid
                            );
                            waiting_logged = true;
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                    // Stale lock from a crashed process - remove it.
                    tracing::debug!("Removing stale launch lock (PID {} not running)", pid);
                }
            }
        }

        // Write our PID and proceed.
        let our_pid = std::process::id();
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&lock_path, our_pid.to_string()).await
            .context("Failed to write launch lock file")?;
        return Ok(Some(lock_path));
    }
}

/// Release the launch lock acquired by `acquire_launch_lock`. Silently ignores
/// errors (e.g. file already removed by an external cleanup).
async fn release_launch_lock(lock_path: &Path) {
    let _ = fs::remove_file(lock_path).await;
}

/// Check whether a process with `pid` is currently alive using sysinfo.
fn is_pid_running(pid: u32) -> bool {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes();
    sys.process(sysinfo::Pid::from_u32(pid)).is_some()
}

// ---------------------------------------------------------------------------
// Mod enumeration — read installed mod display names from jar manifests.
// ---------------------------------------------------------------------------

/// Metadata extracted from a mod's `fabric.mod.json` (or similar) manifest.
#[derive(Debug)]
struct ModInfo {
    name: String,
    version: String,
}

/// Read the `fabric.mod.json` manifest embedded in a mod JAR and return
/// display metadata. Returns `None` when the file is absent or unparseable.
fn read_fabric_mod_info(jar_path: &Path) -> Option<ModInfo> {
    #[derive(serde::Deserialize)]
    struct FabricModJson {
        name: Option<String>,
        id: String,
        version: String,
    }

    let file = std::fs::File::open(jar_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name("fabric.mod.json").ok()?;
    let mut contents = String::new();
    std::io::Read::read_to_string(&mut entry, &mut contents).ok()?;
    let meta: FabricModJson = serde_json::from_str(&contents).ok()?;
    Some(ModInfo {
        name: meta.name.unwrap_or(meta.id),
        version: meta.version,
    })
}

/// Enumerate all mod JARs in `mods_dir` and return display metadata for each.
/// Non-Fabric JARs (lacking `fabric.mod.json`) are listed by filename only.
fn enumerate_mods(mods_dir: &Path) -> Vec<ModInfo> {
    let mut mods = Vec::new();
    let entries = match std::fs::read_dir(mods_dir) {
        Ok(e) => e,
        Err(_) => return mods,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jar") {
            continue;
        }
        if let Some(info) = read_fabric_mod_info(&path) {
            mods.push(info);
        } else {
            // Non-Fabric JAR or unreadable: use the filename without extension.
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            mods.push(ModInfo { name: stem, version: String::new() });
        }
    }
    mods.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    mods
}

/// Print a formatted mod list to the terminal and (on Windows) set the console
/// window title so the operator can see the active test context at a glance.
fn display_mod_list(instance_name: &str, mods: &[ModInfo]) {
    if mods.is_empty() {
        tracing::info!("No mods installed in instance '{}'", instance_name);
        return;
    }
    tracing::info!("Loaded {} mod(s) in '{}':", mods.len(), instance_name);
    for m in mods {
        if m.version.is_empty() {
            tracing::info!("  - {}", m.name);
        } else {
            tracing::info!("  - {} ({})", m.name, m.version);
        }
    }

    // Set the OS console window title so the test context is visible at a glance.
    let mod_names: Vec<&str> = mods.iter().map(|m| m.name.as_str()).collect();
    let title = format!("MDL: {} [{}]", instance_name, mod_names.join(", "));
    set_console_title(&title);
}

/// Set the console window title on Windows; no-op on other platforms.
fn set_console_title(title: &str) {
    #[cfg(target_os = "windows")]
    {
        // Use the `cmd /C title` approach so we don't need winapi bindings.
        let _ = Command::new("cmd")
            .args(["/C", &format!("title {}", title)])
            .status();
    }
    #[cfg(not(target_os = "windows"))]
    {
        // ANSI escape sequence for terminal emulators that support it.
        print!("\x1b]0;{}\x07", title);
    }
}

#[derive(Debug, Deserialize)]
struct VersionJson {
    id: String,
    #[serde(rename = "inheritsFrom")]
    inherits_from: Option<String>,
    // Option: some installer-produced version JSONs (e.g. certain NeoForge
    // releases) omit mainClass at the top level and rely on the inheritsFrom
    // chain to supply it. Deserializing as Option<String> with a default of
    // None prevents a hard parse error in those cases.
    #[serde(rename = "mainClass", default)]
    main_class: Option<String>,
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

/// Options controlling how an instance is launched. All fields are optional
/// and fall back to sensible defaults when absent.
#[derive(Debug, Default)]
pub struct LaunchOptions {
    /// In-game display name (default: "Player")
    pub username: Option<String>,
    /// Auto-connect to a server on launch ("host:port" or "host")
    pub server: Option<String>,
    /// Launch in fullscreen mode
    pub fullscreen: bool,
    /// Window width in pixels
    pub width: Option<u32>,
    /// Window height in pixels
    pub height: Option<u32>,
    /// Return immediately after spawning the game process instead of waiting
    /// for it to exit. Required for agent workflows and the agent API.
    pub detach: bool,
    /// Enable agent control via the Despotes mod (must be installed in the
    /// passes the control-server port to the game via a JVM property, and
    /// disables pause-on-lost-focus so the game keeps running while the
    /// user focuses other applications.
    pub agent: bool,
    /// TCP port for the Despotes control server (default 25585).
    pub agent_port: Option<u16>,
    /// Custom Java executable path (overrides auto-detection).
    pub java_path: Option<String>,
    /// Explicit memory allocation like "4G"/"2048M".
    pub memory: Option<String>,
    /// Allocate memory dynamically from system RAM when `memory` is unset.
    pub dynamic_memory: bool,
    /// Attach the Aprism JE Native loader as a javaagent.
    pub aprism: bool,
    /// After the game is ready, auto-enter (or create) the test world via Despotes.
    pub enter_test_world: bool,
    /// Block until the game broadcasts ready (agent mode).
    pub wait_ready: bool,
    /// Skip the instance queue: launch even if another instance is running.
    pub no_queue: bool,
}

/// Result of a successful launch.
#[derive(Debug)]
pub struct LaunchOutcome {
    /// Process ID of the spawned game (java) process.
    pub pid: u32,
    /// True when the launcher returned without waiting for the game to exit.
    pub detached: bool,
}

pub struct InstanceLauncher {
    java_path: PathBuf,
}

impl InstanceLauncher {
    pub fn new() -> Result<Self> {
        // Java is resolved (and auto-downloaded if necessary) inside launch()
        // via JavaRuntime::ensure_version, so construction must not fail when the
        // system has no Java yet. Probe for one opportunistically; fall back to a
        // placeholder path that launch() overrides.
        let java_path = crate::version::java::JavaRuntime::detect()
            .map(|r| r.path)
            .unwrap_or_else(|_| PathBuf::from("java"));
        Ok(Self { java_path })
    }

    /// Create launcher with specific Java version requirement
    pub fn with_java_version(required_version: u8) -> Result<Self> {
        let java_runtime = crate::version::java::JavaRuntime::detect()
            .context("Failed to detect Java installation")?;

        if !java_runtime.meets_requirement(required_version) {
            anyhow::bail!(
                "Minecraft requires Java {} or later, but found Java {}.\n\
                Please install Java {} from: https://adoptium.net/temurin/releases/?version={}",
                required_version,
                java_runtime.major_version,
                required_version,
                required_version
            );
        }

        Ok(Self { java_path: java_runtime.path })
    }

    pub async fn launch(&self, name: &str, options: &LaunchOptions) -> Result<LaunchOutcome> {
        let instances_dir = crate::util::paths::get_instances_dir()?;
        let instance_dir = instances_dir.join(name);

        if !instance_dir.exists() {
            anyhow::bail!("Instance '{}' does not exist", name);
        }

        let lock_path = acquire_launch_lock(options.no_queue).await?;

        // Run the actual launch; then handle the lock depending on outcome.
        let result = self.do_launch(name, &instance_dir, options).await;

        if let Some(lp) = &lock_path {
            match &result {
                Ok(outcome) if outcome.detached => {
                    // Detached: transfer the lock to the game process so the
                    // single-instance guarantee holds after this launcher
                    // returns. The lock becomes stale once the game exits.
                    if let Err(e) = fs::write(lp, outcome.pid.to_string()).await {
                        tracing::warn!("Failed to transfer launch lock to game process: {}", e);
                    }
                }
                _ => release_launch_lock(lp).await,
            }
        }
        result
    }

    async fn do_launch(&self, name: &str, instance_dir: &Path, options: &LaunchOptions) -> Result<LaunchOutcome> {
        let config_path = instance_dir.join("instance.json");
        let config_data = fs::read_to_string(&config_path).await?;
        let config: InstanceConfig = serde_json::from_str(&config_data)?;

        // Check Java version requirement
        let version_dir = instance_dir.join("versions").join(&config.version);
        let version_metadata = self.load_version_metadata(&version_dir, &config.version).await?;
        let required_java = version_metadata.required_java_version();

        // Resolve a suitable Java runtime. A user-supplied --java-path wins;
        // otherwise auto-download one from Adoptium when the system lacks a
        // runtime meeting the version requirement.
        let java_path: std::path::PathBuf;
        if let Some(custom) = &options.java_path {
            java_path = std::path::PathBuf::from(custom);
            tracing::info!("Using custom Java: {}", java_path.display());
        } else {
            let java_runtime = crate::version::java::JavaRuntime::ensure_version(required_java)
                .await
                .context("Failed to obtain a suitable Java runtime")?;
            java_path = java_runtime.path.clone();
            tracing::info!("Using Java {} (required: Java {})", java_runtime.major_version, required_java);
        }

        tracing::info!("Launching instance '{}'...", name);
        tracing::info!("Building classpath and downloading libraries...");

        // Build classpath and get main class
        let (classpath, main_class, module_path_entries) = self.build_classpath(&version_dir, &config).await?;

        // Prepare game arguments
        let game_dir = instance_dir.clone();
        let assets_dir = crate::util::paths::get_data_dir()?.join("assets");
        let natives_dir = version_dir.join("natives");

        fs::create_dir_all(&game_dir).await?;
        fs::create_dir_all(&assets_dir).await?;
        fs::create_dir_all(&natives_dir).await?;

        // Download game assets (textures, sounds, language files). Minecraft
        // reads these from <assets>/indexes/<id>.json + <assets>/objects/...; if
        // they are missing the game launches with no textures and logs
        // "Can't open the resource index file". This is idempotent and only
        // fetches what is missing.
        if let Some(asset_index) = &version_metadata.asset_index {
            tracing::info!("Verifying game assets...");
            crate::version::assets::download_assets(asset_index, &assets_dir).await?;
        }

        // Load loader-specific JVM and game args from version.json
        let libraries_dir = crate::util::paths::get_libraries_cache_dir()?;
        let (loader_jvm_args, loader_game_args) = self.load_loader_args(&version_dir, &config, &libraries_dir).await?;

        // Agent mode (Alpha 6): prepare the instance for programmatic control.
        // 1. Install the companion mod (in-process input injection) for Fabric.
        // 2. Force pauseOnLostFocus:false so the game keeps running while the
        //    user focuses other apps — a hard requirement for agent control.
        // 3. Record the requested control port for post-launch discovery.
        // Agent mode (Alpha 7): prepare the instance for programmatic control
        // via the Despotes mod (https://github.com/NDBlockConnect/Despotes),
        // which replaced MDL's old bundled companion:
        // 1. Verify Despotes is present (it is offered at `create` time).
        // 2. Force pauseOnLostFocus:false so the game keeps running while the
        //    user focuses other apps — a hard requirement for agent control.
        // 3. Record the requested control port for post-launch discovery.
        let agent_port = options.agent_port.unwrap_or(crate::game::DEFAULT_DESPOTES_PORT);
        if options.agent {
            if crate::game::despotes::is_installed(instance_dir) {
                tracing::info!("Agent control enabled via Despotes");
            } else {
                tracing::warn!(
                    "Agent control requested but Despotes is not installed in this instance. \
                     Create the instance with Despotes (see `mdl create`) or install its JAR \
                     into mods/. The game will launch without in-game control support."
                );
            }
            match crate::game::options::ensure_no_pause_on_lost_focus(instance_dir) {
                Ok(true) => tracing::info!("Set pauseOnLostFocus:false (game keeps running when unfocused)"),
                Ok(false) => tracing::debug!("pauseOnLostFocus already disabled"),
                Err(e) => tracing::warn!("Failed to set pauseOnLostFocus: {}", e),
            }
            // Record the requested Despotes port so `mdl game ...` commands can
            // locate the control server after launch.
            let runtime_dir = instance_dir.join("runtime");
            fs::create_dir_all(&runtime_dir).await?;
            fs::write(runtime_dir.join(crate::game::DESPOTES_PORT_FILE), agent_port.to_string()).await?;
        }

        // Fabric instances: ensure Fabric API is present before launch. The
        // create-time install is best-effort (warns on failure), so a failure
        // there — or a user removing the file — leaves the instance without
        // it, and most Fabric mods then refuse to load ("Fabric API not
        // installed"). Detect and repair it here so launches are resilient.
        if config.loader.as_ref().map(|l| l.loader_type.as_str()) == Some("fabric") {
            let mods_dir_probe = instance_dir.join("mods");
            let has_fabric_api = std::fs::read_dir(&mods_dir_probe)
                .map(|entries| {
                    entries.flatten().any(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .to_lowercase()
                            .contains("fabric-api")
                    })
                })
                .unwrap_or(false);
            if !has_fabric_api {
                tracing::warn!("Fabric API missing from mods/ - downloading it before launch...");
                match crate::loader::fabric::FabricInstaller::install_fabric_api(
                    &version_metadata.id,
                    &mods_dir_probe,
                )
                .await
                {
                    Ok(()) => tracing::info!("Fabric API installed"),
                    Err(e) => tracing::warn!(
                        "Could not auto-install Fabric API: {} (mods may fail to load)",
                        e
                    ),
                }
            }
        }

        // Enumerate installed mods and display them before launch so the
        // operator can confirm the test context. Also sets the console window
        // title to "MDL: <instance> [mod1, mod2, ...]" for easy identification.
        let mods_dir = instance_dir.join("mods");
        let mods = enumerate_mods(&mods_dir);
        display_mod_list(name, &mods);

        // Build the Minecraft window title from the instance name and mod list.
        // Passed to the game via --title (supported since 1.14). Lets the user
        // identify which test session a game window belongs to at a glance.
        let window_title = if mods.is_empty() {
            format!("MDL: {}", name)
        } else {
            let names: Vec<&str> = mods.iter().map(|m| m.name.as_str()).collect();
            format!("MDL: {} [{}]", name, names.join(", "))
        };

        // Build launch command
        let mut cmd = Command::new(&java_path);

        // JVM arguments: memory. An explicit --memory wins; otherwise a
        // dynamic allocation derived from system RAM (half of total, capped
        // 8G) is used; final fallback is 2G.
        let (xmx, xms) = match &options.memory {
            Some(m) => (m.clone(), "512M".to_string()),
            None if options.dynamic_memory => {
                use sysinfo::System;
                let mut sys = System::new();
                sys.refresh_memory();
                let total_mb = sys.total_memory() / 1024 / 1024;
                let alloc = ((total_mb / 2).min(8192)).max(2048);
                (format!("{}M", alloc), "512M".to_string())
            }
            None => ("2G".to_string(), "512M".to_string()),
        };
        cmd.arg(format!("-Xmx{}", xmx));
        cmd.arg(format!("-Xms{}", xms));
        // Dynamic performance tuning: pick GC by allocation tier.
        if options.dynamic_memory {
            let mb: u64 = if let Some(v) = xmx.strip_suffix('G') {
                v.parse::<u64>().unwrap_or(2) * 1024
            } else if let Some(v) = xmx.strip_suffix('M') {
                v.parse::<u64>().unwrap_or(2048)
            } else {
                2048
            };
            if mb >= 4096 {
                cmd.arg("-XX:+UseG1GC");
                cmd.arg("-XX:MaxGCPauseMillis=50");
            } else {
                cmd.arg("-XX:+UseSerialGC");
            }
        }
        cmd.arg(format!("-Djava.library.path={}", natives_dir.display()));
        if options.agent {
            // Hand the control-server port to Despotes via its documented
            // system property override (-Ddespotes.port=NNNN).
            cmd.arg(format!("-D{}={}", crate::game::DESPOTES_PORT_PROPERTY, agent_port));
        }

        if !loader_jvm_args.is_empty() {
            // Use loader-provided JVM args (NeoForge: includes correct -p, --add-opens, DlibraryDirectory, etc.)
            for arg in &loader_jvm_args {
                cmd.arg(arg);
            }
        } else if !module_path_entries.is_empty() {
            // Forge (legacy): use our constructed module path
            let sep = if cfg!(windows) { ";" } else { ":" };
            cmd.arg("-p");
            cmd.arg(module_path_entries.join(sep));
            cmd.arg("--add-modules");
            cmd.arg("ALL-MODULE-PATH");
            cmd.arg("--add-opens");
            cmd.arg("java.base/java.util.jar=cpw.mods.securejarhandler");
            cmd.arg("--add-opens");
            cmd.arg("java.base/java.lang.invoke=cpw.mods.securejarhandler");
            cmd.arg("--add-exports");
            cmd.arg("java.base/sun.security.util=cpw.mods.securejarhandler");
            cmd.arg("--add-exports");
            cmd.arg("jdk.naming.dns/com.sun.jndi.dns=java.naming");
        }

        cmd.arg("-cp");
        cmd.arg(&classpath);

        // Main class
        cmd.arg(&main_class);

        // Game arguments (standard + loader-specific)
        self.add_game_arguments(&mut cmd, &config, &version_metadata, &game_dir, &assets_dir, &main_class, &version_dir, &loader_game_args, &window_title, options)?;

        cmd.current_dir(&game_dir);
        // In detached mode the game outlives this launcher process, so its
        // output cannot inherit the launcher console. Redirect stdout/stderr
        // into a launch log file instead. In attached mode keep inheriting so
        // the operator sees live game output as before.
        let log_file = if options.detach {
            let log_dir = instance_dir.join("logs");
            tokio::fs::create_dir_all(&log_dir).await?;
            let path = log_dir.join("launch_detached.log");
            let file = std::fs::File::create(&path)
                .with_context(|| format!("Failed to create launch log {}", path.display()))?;
            cmd.stdout(file.try_clone()?);
            cmd.stderr(file);
            Some(path)
        } else {
            cmd.stdout(Stdio::inherit());
            cmd.stderr(Stdio::inherit());
            None
        };

        tracing::info!("Starting Minecraft...");
        tracing::debug!("Command: {:?}", cmd);

        let mut child = cmd.spawn().context("Failed to spawn Minecraft process")?;
        let pid = child.id();

        // Write PID file for status tracking
        let pid_dir = instance_dir.join("runtime");
        tokio::fs::create_dir_all(&pid_dir).await?;
        let pid_file = pid_dir.join("pid");
        tokio::fs::write(&pid_file, pid.to_string()).await?;

        if options.detach {
            // Return immediately; the game keeps running in the background.
            // The launch lock was transferred to the game process in launch().
            if let Some(log) = log_file {
                tracing::info!("Game output is being written to {}", log.display());
            }
            tracing::info!("Instance '{}' launched in background (PID {})", name, pid);
            return Ok(LaunchOutcome { pid, detached: true });
        }

        let status = child.wait().context("Failed to wait for Minecraft process")?;

        // Clean up PID file when process exits
        let pid_file = instance_dir.join("runtime").join("pid");
        let _ = tokio::fs::remove_file(&pid_file).await;

        if !status.success() {
            anyhow::bail!("Minecraft exited with code: {:?}", status.code());
        }

        tracing::info!("Minecraft closed successfully");
        Ok(LaunchOutcome { pid, detached: false })
    }

    async fn build_classpath(&self, version_dir: &Path, config: &InstanceConfig) -> Result<(String, String, Vec<String>)> {
        let libraries_dir = crate::util::paths::get_libraries_cache_dir()?;
        let mut classpath_entries = Vec::new();
        // Special NeoForge JARs (patched client, extra resources, universal) that
        // must bypass coordinate-level deduplication. The patched client and the
        // universal JAR share the same Maven coordinate directory
        // (net/neoforged/neoforge/<version>/) and differ only by classifier, so
        // the coordinate-based dedup below would otherwise collapse them into one.
        let mut deferred_jars: Vec<String> = Vec::new();
        let module_path_entries = Vec::new();
        let mut main_class = String::new();
        let mut is_neoforge = false;

        let loader_type = config.loader.as_ref().map(|l| l.loader_type.as_str());
        let is_neoforge_loader = loader_type == Some("neoforge");

        // Add Minecraft client jar FIRST (required by Forge ModLauncher).
        // NeoForge is the exception: it does NOT use the raw obfuscated vanilla
        // client. Instead it uses a deobfuscated + binary-patched client produced
        // by the installer (neoforge-<version>-client.jar), added below.
        let client_jar = version_dir.join(format!("{}.jar", config.version));
        if !is_neoforge_loader {
            classpath_entries.push(client_jar.display().to_string());
        }

        // Load version.json if loader is present
        if config.loader.is_some() {
            let loader_json_path = version_dir.join("version.json");
            if loader_json_path.exists() {
                let loader_json_data = fs::read_to_string(&loader_json_path).await?;
                let loader_json: VersionJson = serde_json::from_str(&loader_json_data)?;

                // Honour the loader's mainClass only when present. Some
                // installer-produced JSONs omit the field and rely on
                // inheritsFrom; in that case we fall through to the base-JSON
                // fallback below.
                if let Some(cls) = loader_json.main_class.clone() {
                    main_class = cls;
                }

                // Check if this is NeoForge (needs module path instead of classpath)
                is_neoforge = main_class.contains("bootstraplauncher") || main_class.contains("neoforge");

                // NeoForge: add the installer-produced game JARs. These come from
                // the installer's processor pipeline, not from version.json's
                // library list, so they are resolved by convention here.
                if is_neoforge {
                    // Extract NeoForge version from loader_json.id (e.g., "neoforge-21.1.1" -> "21.1.1")
                    let neoforge_version = loader_json.id.strip_prefix("neoforge-")
                        .unwrap_or(&loader_json.id);

                    // CRITICAL FIX: Check if the installed NeoForge version matches
                    // the version specified in instance.json. If they differ, the
                    // user may have manually edited instance.json expecting an upgrade,
                    // or the installer selected a different version (e.g., beta instead
                    // of stable due to incorrect sorting).
                    if let Some(expected_version) = config.loader.as_ref().and_then(|l| {
                        if l.version == "latest" {
                            None // "latest" is always valid
                        } else {
                            Some(l.version.as_str())
                        }
                    }) {
                        if expected_version != neoforge_version {
                            tracing::error!(
                                "NeoForge version mismatch! Expected {} (from instance.json) but found {} (from version.json)",
                                expected_version,
                                neoforge_version
                            );
                            tracing::error!(
                                "This usually happens when:\n\
                                1. You manually edited instance.json to upgrade NeoForge\n\
                                2. The initial installation selected a beta version instead of stable\n\
                                \n\
                                To fix this, delete the instance and recreate it with the correct version:\n\
                                  mdl delete {} && mdl create {} --mc-version {} --loader neoforge --loader-version {}",
                                config.name,
                                config.name,
                                config.version,
                                expected_version
                            );
                            anyhow::bail!(
                                "NeoForge version mismatch: expected {} but found {}. \
                                Please recreate the instance with the correct version.",
                                expected_version,
                                neoforge_version
                            );
                        }
                    }

                    let neoforge_dir = libraries_dir
                        .join("net")
                        .join("neoforged")
                        .join("neoforge")
                        .join(neoforge_version);

                    // Patched client: the deobfuscated + binary-patched Minecraft
                    // client. Contains the Mojang-mapped classes (LoadingOverlay,
                    // BlockEntityType, ...) that FML mixins and the game require.
                    // NeoForge installer places it at net/neoforged/minecraft-client-patched/<version>/
                    let patched_client = libraries_dir
                        .join("net")
                        .join("neoforged")
                        .join("minecraft-client-patched")
                        .join(neoforge_version)
                        .join(format!("minecraft-client-patched-{}.jar", neoforge_version));
                    if patched_client.exists() {
                        tracing::debug!("Adding NeoForge patched client: {:?}", patched_client);
                        deferred_jars.push(patched_client.display().to_string());
                    } else {
                        tracing::warn!("NeoForge patched client not found: {:?}", patched_client);
                        // CRITICAL FIX: If patched client is missing, fall back to vanilla
                        // client JAR to ensure net.minecraft.client.main.Main is available
                        tracing::warn!("Falling back to vanilla client JAR as emergency classpath entry");
                        classpath_entries.insert(0, client_jar.display().to_string());
                    }

                    // Universal JAR: NeoForge API + mod-loading classes.
                    let universal = neoforge_dir.join(format!("neoforge-{}-universal.jar", neoforge_version));
                    if universal.exists() {
                        tracing::debug!("Adding NeoForge universal JAR: {:?}", universal);
                        deferred_jars.push(universal.display().to_string());
                    } else {
                        tracing::warn!("NeoForge universal JAR not found: {:?}", universal);
                    }

                    // Extra JAR: Minecraft assets/resources split out of the client
                    // by the installer. The coordinate is
                    // net.minecraft:client:<mcVersion>-<neoFormVersion>:extra and is
                    // reconstructed from the --fml.mcVersion / --fml.neoFormVersion
                    // game arguments in version.json.
                    if let Some(neoform) = Self::neoform_version_from_args(&loader_json) {
                        let extra = libraries_dir
                            .join("net")
                            .join("minecraft")
                            .join("client")
                            .join(&neoform)
                            .join(format!("client-{}-extra.jar", neoform));
                        if extra.exists() {
                            tracing::debug!("Adding Minecraft extra JAR: {:?}", extra);
                            deferred_jars.push(extra.display().to_string());
                        } else {
                            tracing::warn!("Minecraft extra JAR not found: {:?}", extra);
                        }
                    } else {
                        tracing::warn!("Could not determine neoForm version for extra JAR");
                    }
                }

                // Add loader libraries. For NeoForge, ALL libraries go on the classpath;
                // the module path is fully specified by version.json's `-p` JVM argument.
                //
                // Libraries in loader version.json come in three flavours:
                //   1. Forge-style with `downloads.artifact` carrying url + sha1.
                //   2. Fabric/Quilt-style with only `name` + `url` (Maven repo base).
                //   3. Forge client placeholder: `downloads.artifact.url` is empty —
                //      copy the vanilla client JAR to the Maven-resolved path instead.
                //
                // All three must be downloaded when absent; previously only flavour 3
                // was handled, causing Fabric's intermediary, ASM 9.7.x, and other
                // loader-exclusive libraries to be silently skipped when not cached.
                for library in &loader_json.libraries {
                    // --- Flavour 3: Forge client placeholder (empty artifact URL) ---
                    if library.name.contains(":client") {
                        if let Some(downloads) = &library.downloads {
                            if let Some(artifact) = &downloads.artifact {
                                if artifact.url.is_empty() {
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

                    // --- Flavour 1: full artifact record with url + sha1 ---
                    if let Some(downloads) = &library.downloads {
                        if let Some(artifact) = &downloads.artifact {
                            if !artifact.url.is_empty() {
                                let lib_path = self.get_library_path_from_name(&library.name, &libraries_dir);
                                if !lib_path.exists() {
                                    tracing::info!("Downloading loader library: {}", library.name);
                                    self.download_library(&artifact.url, &lib_path, &artifact.sha1).await?;
                                }
                                classpath_entries.push(lib_path.display().to_string());
                                continue;
                            }
                        }
                    }

                    // --- Flavour 2: Fabric/Quilt-style — only Maven repo base URL ---
                    if let Some(base_url) = &library.url {
                        let lib_path = self.get_library_path_from_name(&library.name, &libraries_dir);
                        if !lib_path.exists() {
                            let maven_url = Self::maven_artifact_url(&library.name, base_url);
                            if !maven_url.is_empty() {
                                tracing::info!("Downloading loader library: {}", library.name);
                                // No sha1 in this format; pass empty to skip checksum.
                                self.download_library(&maven_url, &lib_path, "").await?;
                            }
                        }
                        if lib_path.exists() {
                            classpath_entries.push(lib_path.display().to_string());
                            continue;
                        }
                    }

                    // --- Fallback: already cached, resolve by coordinate only ---
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

        // Use base main class if loader did not supply one.
        if main_class.is_empty() {
            main_class = base_json.main_class.clone().unwrap_or_default();
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
                        // All Minecraft base libraries go on the classpath.
                        // For NeoForge, BootstrapLauncher reads the classpath and
                        // constructs its module layers from these entries.
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

        // Deduplicate classpath entries by Maven coordinate (group:artifact),
        // keeping the first occurrence. Loader libraries are added before the
        // base Minecraft libraries, so the loader's version wins when both list
        // the same artifact at different versions (e.g. log4j-slf4j2-impl 2.19.0
        // from NeoForge vs 2.22.1 from Minecraft). Without coordinate-level
        // deduplication, two different versions of the same automatic module end
        // up on the classpath and crash BootstrapLauncher with either a
        // "Duplicate key" error or a module "exports package ... to" conflict.
        //
        // A library path has the shape:
        //   <libraries_dir>/<group_path>/<artifact>/<version>/<artifact>-<version>.jar
        // so the parent of the version directory (the grandparent of the JAR
        // file) uniquely identifies group:artifact.
        let mut seen = std::collections::HashSet::new();
        classpath_entries.retain(|entry| {
            let path = Path::new(entry);
            let coord_key = path
                .parent()
                .and_then(|version_dir| version_dir.parent())
                .map(|artifact_dir| artifact_dir.to_string_lossy().to_string())
                .unwrap_or_else(|| entry.clone());
            seen.insert(coord_key)
        });

        // Append the deferred NeoForge JARs AFTER deduplication so the patched
        // client and the universal JAR (which share a Maven coordinate directory
        // and differ only by classifier) are both preserved on the classpath.
        classpath_entries.extend(deferred_jars);

        let classpath = if cfg!(windows) {
            classpath_entries.join(";")
        } else {
            classpath_entries.join(":")
        };

        let _module_path = if cfg!(windows) {
            module_path_entries.join(";")
        } else {
            module_path_entries.join(":")
        };

        tracing::info!("Classpath built with {} entries", classpath_entries.len());
        if !module_path_entries.is_empty() {
            tracing::info!("Module path built with {} entries", module_path_entries.len());
        }
        tracing::info!("Main class: {}", main_class);

        Ok((classpath, main_class, module_path_entries))
    }

    async fn download_library(&self, url: &str, path: &Path, expected_sha1: &str) -> Result<()> {
        use crate::version::downloader::download_file;

        // Create parent directory
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // An empty sha1 string means no checksum is available (e.g. Fabric-style
        // library entries that only carry a Maven base URL). Pass None so the
        // downloader skips verification instead of comparing against a blank hash.
        let sha1_opt: Option<&str> = if expected_sha1.is_empty() {
            None
        } else {
            Some(expected_sha1)
        };

        // Retry up to 3 times with exponential backoff
        let mut last_error = None;
        for attempt in 0..3 {
            if attempt > 0 {
                let delay = std::time::Duration::from_millis(500 * 2_u64.pow(attempt - 1));
                tracing::debug!("Retrying download after {:?}", delay);
                tokio::time::sleep(delay).await;
            }

            match download_file(url, path, sha1_opt).await {
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

    /// Construct a Maven artifact download URL from a coordinate string and a
    /// repository base URL. Used for Fabric/Quilt-style library entries that
    /// carry a `url` (repo root) but no per-artifact download record.
    ///
    /// Example:
    ///   name:     "net.fabricmc:intermediary:1.21.4"
    ///   base_url: "https://maven.fabricmc.net/"
    ///   result:   "https://maven.fabricmc.net/net/fabricmc/intermediary/1.21.4/intermediary-1.21.4.jar"
    fn maven_artifact_url(name: &str, base_url: &str) -> String {
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() < 3 {
            return String::new();
        }
        let group_path = parts[0].replace('.', "/");
        let artifact = parts[1];
        let version = parts[2].split('@').next().unwrap_or(parts[2]);
        let jar_name = if parts.len() > 3 {
            let classifier = parts[3].split('@').next().unwrap_or(parts[3]);
            format!("{}-{}-{}.jar", artifact, version, classifier)
        } else {
            format!("{}-{}.jar", artifact, version)
        };
        format!(
            "{}/{}/{}/{}/{}",
            base_url.trim_end_matches('/'),
            group_path,
            artifact,
            version,
            jar_name
        )
    }

    fn get_library_path_from_name(&self, name: &str, libraries_dir: &Path) -> PathBuf {
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() < 3 {
            return libraries_dir.join(name);
        }

        let group = parts[0].replace('.', "/");
        let artifact = parts[1];
        // Strip @jar or @type suffix from version (Maven packaging type notation)
        let version = parts[2].split('@').next().unwrap_or(parts[2]);

        // Handle natives (e.g., "org.lwjgl:lwjgl:3.3.1:natives-windows")
        // or classifiers with @type (e.g., "artifact:version:classifier@jar")
        let jar_name = if parts.len() > 3 {
            // Strip @type from classifier if present
            let classifier = parts[3].split('@').next().unwrap_or(parts[3]);
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
        // Strip @jar or @type suffix from version
        let version = parts[2].split('@').next().unwrap_or(parts[2]);

        // Handle classifier (e.g., "net.neoforged:mergetool:2.0.0:api@jar" ->
        // mergetool-2.0.0-api.jar). Without this, classified artifacts like the
        // mergetool `api` JAR (which contains net.neoforged.api.distmarker.OnlyIn)
        // resolve to a non-existent path and never reach the classpath, causing
        // NoClassDefFoundError at launch.
        let jar_name = if parts.len() > 3 {
            let classifier = parts[3].split('@').next().unwrap_or(parts[3]);
            format!("{}-{}-{}.jar", artifact, version, classifier)
        } else {
            format!("{}-{}.jar", artifact, version)
        };

        let path = libraries_dir
            .join(&group)
            .join(artifact)
            .join(version)
            .join(jar_name);

        if path.exists() {
            Some(path.display().to_string())
        } else {
            None
        }
    }

    /// Reconstruct the neoForm-qualified Minecraft version used for the extra
    /// resources JAR (net.minecraft:client:<mcVersion>-<neoFormVersion>:extra).
    ///
    /// NeoForge's version.json carries the two halves as separate game
    /// arguments (`--fml.mcVersion 1.21.1` and `--fml.neoFormVersion
    /// 20240808.144430`); the installer names the extracted resources JAR
    /// `client-<mcVersion>-<neoFormVersion>-extra.jar`. We scan the raw game
    /// argument list for both flags and join their values.
    fn neoform_version_from_args(loader_json: &VersionJson) -> Option<String> {
        let args = loader_json.arguments.as_ref()?;
        let game = args.game.as_ref()?;

        // Flatten the game arguments into a plain string sequence so we can look
        // for a flag followed by its value.
        let mut flat: Vec<String> = Vec::new();
        for arg in game {
            match arg {
                ArgumentValue::String(s) => flat.push(s.clone()),
                ArgumentValue::Object { value, .. } => match value {
                    ArgumentValueInner::String(s) => flat.push(s.clone()),
                    ArgumentValueInner::Array(arr) => flat.extend(arr.iter().cloned()),
                },
            }
        }

        let find_value = |flag: &str| -> Option<String> {
            flat.iter()
                .position(|a| a == flag)
                .and_then(|i| flat.get(i + 1))
                .cloned()
        };

        let mc_version = find_value("--fml.mcVersion")?;
        let neoform_version = find_value("--fml.neoFormVersion")?;

        Some(format!("{}-{}", mc_version, neoform_version))
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

    async fn load_loader_args(
        &self,
        version_dir: &Path,
        config: &InstanceConfig,
        libraries_dir: &Path,
    ) -> Result<(Vec<String>, Vec<String>)> {
        if config.loader.is_none() {
            return Ok((Vec::new(), Vec::new()));
        }
        let loader_json_path = version_dir.join("version.json");
        if !loader_json_path.exists() {
            return Ok((Vec::new(), Vec::new()));
        }
        let loader_json_data = fs::read_to_string(&loader_json_path).await?;
        let loader_json: VersionJson = serde_json::from_str(&loader_json_data)?;

        let library_dir = libraries_dir.display().to_string();
        let sep = if cfg!(windows) { ";" } else { ":" };
        let version_name = &config.version;

        let substitute = |s: &str| -> String {
            s.replace("${library_directory}", &library_dir)
             .replace("${classpath_separator}", sep)
             .replace("${version_name}", version_name)
        };

        let mut jvm_args = if let Some(args) = &loader_json.arguments {
            let mut result = Vec::new();
            if let Some(jvm) = &args.jvm {
                for arg in jvm {
                    match arg {
                        ArgumentValue::String(s) => result.push(substitute(s)),
                        ArgumentValue::Object { rules, value } => {
                            if self.check_rules(rules) {
                                match value {
                                    ArgumentValueInner::String(s) => result.push(substitute(s)),
                                    ArgumentValueInner::Array(arr) => {
                                        result.extend(arr.iter().map(|s| substitute(s)));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            result
        } else {
            Vec::new()
        };

        // NeoForge 21.4+ fix: the installer-produced version.json may omit
        // "neoforge-" from -DignoreList, causing BootstrapLauncher to load the
        // patched client/universal JARs as named modules. When two JARs with
        // overlapping packages are treated as named modules, the JVM raises a
        // mixin_synthetic duplicate-module error at startup. Appending
        // "neoforge-" to the ignore list tells BootstrapLauncher to keep those
        // JARs on the unnamed module path (classpath) instead.
        let loader_type = config.loader.as_ref().map(|l| l.loader_type.as_str());
        if loader_type == Some("neoforge") {
            let mut found = false;
            for arg in &mut jvm_args {
                if let Some(list) = arg.strip_prefix("-DignoreList=") {
                    if !list.split(',').any(|e| e.trim() == "neoforge-") {
                        arg.push_str(",neoforge-");
                    }
                    found = true;
                    break;
                }
            }
            // If the installer omitted -DignoreList entirely, inject it so the
            // JVM flag is always present for NeoForge instances.
            if !found {
                jvm_args.push("-DignoreList=neoforge-".to_string());
            }
        }

        let game_args = if let Some(args) = &loader_json.arguments {
            let mut result = Vec::new();
            if let Some(game) = &args.game {
                for arg in game {
                    match arg {
                        ArgumentValue::String(s) => result.push(substitute(s)),
                        ArgumentValue::Object { rules, value } => {
                            if self.check_rules(rules) {
                                match value {
                                    ArgumentValueInner::String(s) => result.push(substitute(s)),
                                    ArgumentValueInner::Array(arr) => {
                                        result.extend(arr.iter().map(|s| substitute(s)));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            result
        } else {
            Vec::new()
        };

        Ok((jvm_args, game_args))
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
        loader_game_args: &[String],
        window_title: &str,
        options: &LaunchOptions,
    ) -> Result<()> {
        // Standard arguments
        cmd.arg("--username");
        cmd.arg(options.username.as_deref().unwrap_or("Player"));
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

        // Optional window / display options
        if options.fullscreen {
            cmd.arg("--fullscreen");
        }
        if let Some(w) = options.width {
            cmd.arg("--width");
            cmd.arg(w.to_string());
        }
        if let Some(h) = options.height {
            cmd.arg("--height");
            cmd.arg(h.to_string());
        }
        // Auto-connect to server: parse "host:port" or treat as host-only
        if let Some(server) = &options.server {
            let (host, port) = if let Some((h, p)) = server.rsplit_once(':') {
                (h, p.parse::<u16>().unwrap_or(25565))
            } else {
                (server.as_str(), 25565u16)
            };
            cmd.arg("--server");
            cmd.arg(host);
            cmd.arg("--port");
            cmd.arg(port.to_string());
        }

        let is_neoforge = main_class.contains("bootstraplauncher") || main_class.contains("neoforge");
        let is_modded = main_class.contains("forge") || is_neoforge;

        // Set the Minecraft window title to the instance name + loaded mods so
        // the operator can identify the test session at a glance. Supported
        // since Minecraft 1.14; silently ignored by older versions.
        if !window_title.is_empty() {
            cmd.arg("--title");
            cmd.arg(window_title);
        }

        if is_neoforge && !loader_game_args.is_empty() {
            // NeoForge: use the game args verbatim from version.json (they carry
            // --launchTarget forgeclient, --fml.neoForgeVersion, --fml.mcVersion,
            // --fml.neoFormVersion, --fml.fmlVersion). Do NOT inject --gameJar:
            // NeoForge locates the patched client from the classpath through
            // BootstrapLauncher's -DignoreList, and pointing --gameJar at the raw
            // obfuscated vanilla jar (which isn't even on the classpath) makes
            // ModLauncher fail to find the Mojang-mapped game classes.
            for arg in loader_game_args {
                cmd.arg(arg);
            }
        } else if is_modded && !loader_game_args.is_empty() {
            // Newer Forge: use game args from version.json, then force --gameJar
            // to the vanilla client (Forge's ModLauncher still consumes it).
            for arg in loader_game_args {
                cmd.arg(arg);
            }
            let client_jar = version_dir.join(format!("{}.jar", config.version));
            cmd.arg("--gameJar");
            cmd.arg(client_jar.display().to_string());
        } else if is_modded {
            // Legacy Forge: add launchTarget and gameJar manually
            cmd.arg("--launchTarget");
            cmd.arg("fmlclient");
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

    async fn write_pid_file(&self, instance_path: &Path, pid: u32) -> Result<()> {
        let runtime_dir = instance_path.join("runtime");
        tokio::fs::create_dir_all(&runtime_dir).await?;

        let pid_file = runtime_dir.join("pid");
        tokio::fs::write(&pid_file, pid.to_string()).await?;

        Ok(())
    }
}

/// Download (cache) and attach the Aprism JE javaagent for a MC version.
async fn attach_aprism(
    cmd: &mut Command,
    mc_version: &str,
    instance_dir: &std::path::Path,
) -> Result<String> {
    let releases = crate::loader::aprism::fetch_releases().await?;
    let (release, asset) = crate::loader::aprism::select_release(&releases, mc_version, true)
        .context("No applicable Aprism JE artifact for this Minecraft version")?;
    let jar = crate::loader::aprism::download_asset(&asset).await?;
    let arg = crate::loader::aprism::javaagent_arg(&jar, &release.tag, mc_version, instance_dir);
    cmd.arg(&arg);
    Ok(format!("{} ({})", release.tag, jar.display()))
}
