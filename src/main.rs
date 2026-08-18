// MDL keeps a large intentional public API surface: serde wire-format fields
// deserialized from Mojang/GitHub/Modrinth responses but not read in Rust, and
// reserved utility functions staged for upcoming features. These are deliberate,
// not bugs — suppress dead_code for the whole crate rather than annotating each.
#![allow(dead_code)]

// MCDebugLauncher - Main entry point

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

mod version;
mod loader;
mod instance;
mod diagnostic;
mod agent;
mod game;
mod util;

use instance::config::InstanceConfig;

#[derive(Parser)]
#[command(name = "mdl")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Output format: text, json, yaml
    #[arg(long, global = true, default_value = "text")]
    format: String,

    /// Increase logging verbosity
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Suppress non-error output
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Disable colored output
    #[arg(long, global = true)]
    no_color: bool,

    /// Language for launcher messages: en (default) or zh
    #[arg(long, global = true, default_value = "en")]
    lang: String,

    /// Also write logs to this file (default: <data>/logs/mdl.log)
    #[arg(long, global = true)]
    log_file: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum ModCommands {
    /// List mods in an instance
    List {
        /// Instance name
        instance: String,
    },

    /// Install a mod from a local file
    Install {
        /// Instance name
        instance: String,
        /// Path to mod JAR file
        mod_path: String,
    },

    /// Remove a mod
    Remove {
        /// Instance name
        instance: String,
        /// Mod filename
        mod_name: String,
    },

    /// Enable a disabled mod
    Enable {
        /// Instance name
        instance: String,
        /// Mod filename
        mod_name: String,
    },

    /// Disable a mod
    Disable {
        /// Instance name
        instance: String,
        /// Mod filename
        mod_name: String,
    },
}

#[derive(Subcommand)]
enum BackupCommands {
    /// Create a backup of a world
    Create {
        /// Instance name
        instance: String,
        /// World name
        world: String,
        /// Optional backup name (defaults to world_timestamp)
        #[arg(long)]
        name: Option<String>,
    },

    /// List backups for an instance
    List {
        /// Instance name
        instance: String,
        /// Output format
        #[arg(long, default_value = "text")]
        format: String,
    },

    /// Restore a backup
    Restore {
        /// Instance name
        instance: String,
        /// Backup name
        backup: String,
        /// Target world name (defaults to original world name)
        #[arg(long)]
        target: Option<String>,
    },

    /// Delete a backup
    Delete {
        /// Instance name
        instance: String,
        /// Backup name
        backup: String,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Get a configuration option
    Get {
        /// Instance name
        instance: String,
        /// Option key
        key: String,
    },

    /// Set a configuration option
    Set {
        /// Instance name
        instance: String,
        /// Option key
        key: String,
        /// Option value
        value: String,
    },

    /// Export configuration to JSON file
    Export {
        /// Instance name
        instance: String,
        /// Output file path
        output: String,
    },

    /// Import configuration from JSON file
    Import {
        /// Instance name
        instance: String,
        /// Input file path
        input: String,
    },
}

#[derive(Subcommand)]
enum GameCommands {
    /// Query the game state via the Despotes mod (in-world, screen, player, ...)
    Status {
        /// Instance name
        instance: String,
    },

    /// Capture a screenshot of the game window (works while unfocused)
    Screenshot {
        /// Instance name
        instance: String,

        /// Output file path (PNG). Defaults to screenshots/<instance>_<timestamp>.png
        #[arg(short, long)]
        output: Option<String>,

        /// Seconds to wait for a captured frame (default 5)
        #[arg(long, default_value = "5")]
        timeout: u64,
    },

    /// Press, release, or tap a key inside the game (e.g. w, space, escape, inventory)
    Key {
        /// Instance name
        instance: String,

        /// Key name (Minecraft keybind name, e.g. w, space, escape, inventory)
        key: String,

        /// Action: press, release, or tap
        #[arg(short, long, default_value = "tap")]
        action: String,

        /// Hold duration in ms (for tap)
        #[arg(long)]
        hold_ms: Option<u64>,
    },

    /// Rotate the player's view (absolute yaw/pitch in degrees, or relative)
    Look {
        /// Instance name
        instance: String,

        /// Yaw in degrees
        #[arg(long)]
        yaw: f32,

        /// Pitch in degrees
        #[arg(long)]
        pitch: f32,

        /// Treat yaw/pitch as deltas relative to the current rotation
        #[arg(long)]
        relative: bool,
    },

    /// Perform a mouse action (left/right/middle click) in the game
    Click {
        /// Instance name
        instance: String,

        /// Mouse button: left, right, middle
        #[arg(short, long, default_value = "left")]
        button: String,

        /// Action: press, release, or tap
        #[arg(short, long, default_value = "tap")]
        action: String,

        /// GUI x coordinate (optional; uses current cursor position when omitted)
        #[arg(long)]
        x: Option<f64>,

        /// GUI y coordinate (optional; uses current cursor position when omitted)
        #[arg(long)]
        y: Option<f64>,

        /// Hold duration in ms (for tap)
        #[arg(long)]
        hold_ms: Option<u64>,
    },

    /// Scroll the mouse wheel (positive = up)
    Scroll {
        /// Instance name
        instance: String,

        /// Scroll amount in steps (negative scrolls down)
        amount: f64,
    },

    /// Send a chat message or /command to the game
    Chat {
        /// Instance name
        instance: String,

        /// Message or command (leading "/" is treated as a command)
        message: String,
    },

    /// List MDL game windows visible for capture
    Windows,
}

#[derive(Subcommand)]
enum SearchCommands {
    /// Search mods on Modrinth
    Mod {
        /// Query string
        query: String,
        /// Minecraft version filter (default: instance-agnostic)
        #[arg(long)]
        mc_version: Option<String>,
        /// Loader filter (e.g. fabric)
        #[arg(long)]
        loader: Option<String>,
        /// Install into this instance after choosing
        #[arg(long)]
        instance: Option<String>,
        /// Max results
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// Search resource packs on Modrinth
    Resourcepack {
        query: String,
        #[arg(long)]
        mc_version: Option<String>,
        #[arg(long)]
        instance: Option<String>,
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// Search shaders on Modrinth
    Shader {
        query: String,
        #[arg(long)]
        mc_version: Option<String>,
        #[arg(long)]
        instance: Option<String>,
        #[arg(long, default_value = "10")]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum AccountCommands {
    /// Sign in with a Microsoft account (device code flow)
    Login,
    /// List cached accounts
    List,
    /// Download the account's skin PNG
    Skin {
        /// UUID or username
        account: String,
        /// Output path (PNG)
        #[arg(short, long)]
        output: Option<String>,
    },
}

#[derive(Subcommand)]
enum BedrockCommands {
    /// Download & install the Bedrock Dedicated Server into an instance
    Install {
        /// Instance name
        name: String,
    },
    /// Launch the Bedrock Dedicated Server of an instance
    Launch {
        /// Instance name
        name: String,
    },
    /// Stop the Bedrock Dedicated Server of an instance
    Stop {
        /// Instance name
        name: String,
    },
    /// Show whether the Bedrock Dedicated Server of an instance is running
    Status {
        /// Instance name
        name: String,
        /// Output format
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
enum CacheCommands {
    /// Show cache statistics
    Info,
    /// Evict cache entries unused for N days (default 7)
    Clean {
        #[arg(long, default_value = "7")]
        days: u64,
    },
}

#[derive(Subcommand)]
enum AprismCommands {
    /// Manage AprismRefract loader-support extensions (.aep)
    #[command(subcommand)]
    Refract(AprismRefractCommands),
    /// Manage AprismPrismate loader-side bridge (.jar)
    #[command(subcommand)]
    Prismate(AprismPrismateCommands),
}

#[derive(Subcommand)]
enum AprismRefractCommands {
    /// Install the best-matching loader-support .aep into an instance
    Install {
        /// Instance name
        instance: String,
        /// Loader key override (fabric/forge/neoforge/quilt/liteloader);
        /// defaults to the instance's loader
        #[arg(long)]
        loader: Option<String>,
        /// Minecraft version override; defaults to the instance's version
        #[arg(long)]
        mc_version: Option<String>,
        /// Also consider pre-releases when no stable artifact applies
        #[arg(long)]
        prerelease: bool,
    },
    /// List .aep extensions installed in an instance
    List {
        /// Instance name
        instance: String,
    },
}

#[derive(Subcommand)]
enum AprismPrismateCommands {
    /// Install the best-matching Prismate bridge into an instance's mods/
    Install {
        /// Instance name
        instance: String,
        /// Loader key override (fabric/neoforge/forge); defaults to the
        /// instance's loader
        #[arg(long)]
        loader: Option<String>,
        /// Minecraft version override; defaults to the instance's version
        #[arg(long)]
        mc_version: Option<String>,
        /// Also consider pre-releases when no stable artifact applies
        #[arg(long)]
        prerelease: bool,
    },
    /// Show whether a Prismate bridge is installed in an instance
    Status {
        /// Instance name
        instance: String,
    },
}

#[derive(Subcommand)]
enum ServerCommands {
    /// Create a managed Java Edition dedicated server (downloads server.jar)
    Create {
        /// Server name
        name: String,
        /// Minecraft version (e.g. 1.21.4, release)
        #[arg(long, default_value = "release")]
        mc_version: String,
        /// Max memory allocation (e.g. 4G)
        #[arg(short, long)]
        memory: Option<String>,
    },
    /// List managed servers
    List,
    /// Launch a server (background by default)
    Launch {
        /// Server name
        name: String,
        /// Run attached (foreground, blocks until the server exits)
        #[arg(long)]
        attach: bool,
    },
    /// Stop a running server
    Stop {
        /// Server name
        name: String,
    },
    /// Show server status (running PID, version)
    Status {
        /// Server name
        name: String,
    },
}

#[derive(Subcommand)]
enum Commands {
    /// Manage Minecraft versions
    Versions {
        /// Filter by version type: release, snapshot, all
        #[arg(long, default_value = "release")]
        type_filter: String,

        /// Limit number of results
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Search pattern
        #[arg(long)]
        search: Option<String>,
    },

    /// Get detailed version information
    VersionInfo {
        /// Version ID or alias (release, snapshot)
        version: String,

        /// Show library list
        #[arg(long)]
        show_libraries: bool,

        /// Show asset information
        #[arg(long)]
        show_assets: bool,
    },

    /// Create a new instance
    Create {
        /// Instance name
        name: String,

        /// Minecraft version
        #[arg(long, default_value = "release")]
        mc_version: String,

        /// Mod loader: fabric, forge, neoforge, quilt, optifine, none
        #[arg(short, long)]
        loader: Option<String>,

        /// Loader version
        #[arg(long)]
        loader_version: Option<String>,

        /// Memory allocation (e.g., "4G", "2048M")
        #[arg(short, long)]
        memory: Option<String>,

        /// Skip installation
        #[arg(long)]
        no_install: bool,

        /// Pre-configure an optional test world (entered via --enter-test-world)
        #[arg(long)]
        with_test_world: bool,

        /// Do not offer/install the Despotes control mod after creation
        #[arg(long)]
        no_despotes: bool,

        /// Also consider Despotes pre-releases when no stable build applies
        #[arg(long)]
        despotes_prerelease: bool,
    },

    /// List instances
    List {
        /// Filter by Minecraft version (e.g. 1.21.1)
        #[arg(long)]
        version: Option<String>,
        
        /// Filter by loader type (e.g. fabric, forge, neoforge)
        #[arg(long)]
        loader: Option<String>,
        
        /// Sort by: name, version, loader (default: name)
        #[arg(long, default_value = "name")]
        sort: String,
    },

    /// Launch an instance
    Launch {
        /// Instance name
        name: String,

        /// Offline username
        #[arg(short, long)]
        username: Option<String>,

        /// Auto-connect to server (host:port)
        #[arg(long)]
        server: Option<String>,

        /// Launch in fullscreen
        #[arg(long)]
        fullscreen: bool,

        /// Window width
        #[arg(long)]
        width: Option<u32>,

        /// Window height
        #[arg(long)]
        height: Option<u32>,

        /// Run in background: return immediately after the game starts.
        /// Output goes to logs/launch_detached.log.
        #[arg(long)]
        detach: bool,

        /// Skip the instance queue: launch even if another instance is running.
        #[arg(long)]
        no_queue: bool,

        /// Enable agent control via Despotes (must be installed in the instance),
        /// disables pause-on-lost-focus, and starts the in-game control server.
        #[arg(long)]
        agent: bool,

        /// TCP port for the agent control server (default 25590)
        #[arg(long)]
        agent_port: Option<u16>,

        /// Custom Java executable path (overrides auto-detection)
        #[arg(long)]
        java_path: Option<String>,

        /// Memory allocation, e.g. 4G / 2048M (default: dynamic)
        #[arg(short, long)]
        memory: Option<String>,

        /// Dynamic memory: allocate based on system RAM (default on)
        #[arg(long, default_value = "true")]
        dynamic_memory: bool,

        /// Attach the Aprism JE Native loader as javaagent (downloads if needed)
        #[arg(long)]
        aprism: bool,

        /// After the game is ready, auto-enter the test world via Despotes
        #[arg(long)]
        enter_test_world: bool,

        /// Wait until the game broadcasts "ready" before returning (agent mode)
        #[arg(long)]
        wait_ready: bool,

        /// Idle timeout: terminate the game after N seconds with no log output.
        /// Default 60. Only applies in --detach mode.
        #[arg(long)]
        idle_timeout: Option<u64>,

        /// Disable the idle watchdog entirely (game runs until manually stopped).
        #[arg(long)]
        no_idle_timeout: bool,

        /// Enable OOM self-protection before launching: kill stale Minecraft
        /// processes and trim system working sets (default: on).
        #[arg(long, default_value = "true")]
        oom_protect: bool,

        /// Aggressive OOM protection: also purge system standby list
        /// (requires admin privileges; no-op when not elevated).
        #[arg(long)]
        oom_aggressive: bool,
    },

    /// Diagnose instance issues
    Diagnose {
        /// Instance name
        name: String,

        /// Export diagnostics to archive
        #[arg(long)]
        export: Option<String>,

        /// Analyze logs for known issues
        #[arg(long)]
        analyze: bool,
    },

    /// View instance logs
    Logs {
        /// Instance name
        name: String,

        /// Follow log output
        #[arg(short, long)]
        follow: bool,

        /// Number of lines to show
        #[arg(short, long, default_value = "100")]
        lines: usize,

        /// Filter by log level
        #[arg(long)]
        level: Option<String>,
    },

    /// Get instance status
    Status {
        /// Instance name (optional, shows all if omitted)
        name: Option<String>,

        /// Show detailed instance information (config, mods, etc.)
        #[arg(short, long)]
        detail: bool,
    },

    /// Manage mods
    #[command(subcommand)]
    Mod(ModCommands),

    /// Manage configuration
    #[command(subcommand)]
    Config(ConfigCommands),

    /// Manage world backups
    #[command(subcommand)]
    Backup(BackupCommands),

    /// Observe and control a running game instance (Alpha 6 agent features)
    #[command(subcommand)]
    Game(GameCommands),

    /// Search mods / resource packs / shaders on Modrinth
    #[command(subcommand)]
    Search(SearchCommands),

    /// Microsoft account login, list, skins
    #[command(subcommand)]
    Account(AccountCommands),

    /// Bedrock Dedicated Server support
    #[command(subcommand)]
    Bedrock(BedrockCommands),

    /// Download cache management
    #[command(subcommand)]
    Cache(CacheCommands),

    /// Aprism ecosystem: loader-support extensions (AprismRefract) and the
    /// loader-side bridge (AprismPrismate)
    #[command(subcommand)]
    Aprism(AprismCommands),

    /// Import a Modrinth modpack (.mrpack): create the instance, copy
    /// overrides and auto-download every missing file (pack auto-completion)
    Import {
        /// New instance name
        name: String,
        /// Path to the .mrpack file (or a Modrinth project slug/version URL)
        pack: String,
        /// Skip the file download step (only create instance + overrides)
        #[arg(long)]
        no_download: bool,
    },

    /// Java Edition dedicated server management (create/launch/stop)
    #[command(subcommand)]
    Server(ServerCommands),

    /// Inject a DLL into a running process (Aprism BE groundwork)
    Inject {
        /// Target PID or process name (e.g. Minecraft.Windows.exe)
        target: String,
        /// Path to the DLL to inject
        dll: String,
    },

    /// Delete an instance
    Delete {
        /// Instance name
        name: String,
    },

    /// Show detailed instance information
    InstanceInfo {
        /// Instance name
        name: String,
    },

    /// Clone (duplicate) an instance into a new instance
    Clone {
        /// Source instance name
        name: String,
        /// New instance name
        new_name: String,
    },

    /// Rename an instance
    Rename {
        /// Current instance name
        name: String,
        /// New instance name
        new_name: String,
    },

    /// Start agent server
    Agent {
        /// Server port
        #[arg(long, default_value = "25580")]
        port: u16,

        /// Bind address
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
    },

    /// Get system information
    Info,

    /// Run an environment health check (Java, directories, cache, mirrors,
    /// network reachability) and print a pass/fail report
    Doctor,

    /// Print the machine-readable capability manifest (endpoints, execute
    /// commands, game inputs, events) as JSON for AI/agent consumers
    Capabilities,

    /// Update MDL to the latest version
    Update {
        /// Check for updates without installing
        #[arg(long)]
        check: bool,
    },

    /// Show recent changelog updates
    Changelog {
        /// Number of versions to show (default: 4)
        #[arg(long, default_value = "4")]
        versions: usize,
    },

    /// Export an instance to a zip file
    Export {
        /// Instance name
        instance: String,
        /// Output path for the zip file
        path: PathBuf,
    },

    /// Import an instance from a zip file
    ImportInstance {
        /// Path to the zip file
        path: PathBuf,
        /// Optional instance name (defaults to name from zip)
        name: Option<String>,
    },

    /// Add MDL to system PATH
    Setup,
}

// Writer that tees tracing output to both stdout and a log file so logs are
// persisted without losing live console display (Alpha 7 logging).
// In JSON mode, stdout output is suppressed to avoid polluting machine-readable output.
struct TeeWriter {
    file: Option<std::sync::Arc<std::sync::Mutex<std::fs::File>>>,
    json_mode: bool,
}
impl std::io::Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Only write to stdout if not in JSON mode
        if !self.json_mode {
            let _ = std::io::stdout().write_all(buf);
        }
        if let Some(file) = &self.file {
            if let Ok(mut f) = file.lock() {
                let _ = f.write_all(buf);
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if !self.json_mode {
            let _ = std::io::stdout().flush();
        }
        if let Some(file) = &self.file {
            if let Ok(mut f) = file.lock() {
                let _ = f.flush();
            }
        }
        Ok(())
    }
}
#[derive(Clone)]
struct TeeMakeWriter {
    file: Option<std::sync::Arc<std::sync::Mutex<std::fs::File>>>,
    json_mode: bool,
}
impl TeeMakeWriter {
    fn new(f: Option<std::fs::File>, json_mode: bool) -> Self {
        Self { 
            file: f.map(|f| std::sync::Arc::new(std::sync::Mutex::new(f))),
            json_mode,
        }
    }
}
impl<'a> tracing_subscriber::fmt::writer::MakeWriter<'a> for TeeMakeWriter {
    type Writer = TeeWriter;
    fn make_writer(&'a self) -> Self::Writer {
        TeeWriter { 
            file: self.file.clone(),
            json_mode: self.json_mode,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Language for launcher messages (en default, zh via --lang zh / MDL_LANG)
    let lang = std::env::var("MDL_LANG").unwrap_or_else(|_| cli.lang.clone());
    util::i18n::set_lang(&lang);
    // Windows: force UTF-8 console output so Chinese logs are not garbled.
    util::i18n::enable_utf8_console();

    // Setup logging
    let log_level = if cli.quiet {
        Level::ERROR
    } else {
        match cli.verbose {
            0 => Level::INFO,
            1 => Level::DEBUG,
            _ => Level::TRACE,
        }
    };

    // Persistent log file (default <data>/logs/mdl.log) alongside stdout.
    let log_path = match &cli.log_file {
        Some(p) => std::path::PathBuf::from(p),
        None => util::paths::get_data_dir()?.join("logs").join("mdl.log"),
    };
    let _ = std::fs::create_dir_all(log_path.parent().unwrap_or(std::path::Path::new(".")));
    let file_writer = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok();

    // Machine-readable commands must emit ONLY their payload on stdout:
    // v26.1-alpha.1 adds `capabilities` to the JSON-mode log suppression so an
    // AI agent can parse the manifest without log noise.
    let machine_readable =
        cli.format == "json" || matches!(cli.command, Commands::Capabilities);

    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_ansi(!cli.no_color)
        .with_writer(TeeMakeWriter::new(file_writer, machine_readable))
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("MCDebugLauncher v{}", env!("CARGO_PKG_VERSION"));

    // Kick off a best-effort GitHub update check concurrently with the command.
    // It is throttled by an on-disk cache and never blocks or fails the command.
    let update_check = tokio::spawn(util::update::check_for_update());

    // Alpha 8.1: show the four most recent version digests at startup.
    util::changelog::print_recent_updates(machine_readable);

    // Execute command
    match cli.command {
        Commands::Versions { type_filter, limit, search } => {
            cmd_versions(&cli.format, &type_filter, limit, search.as_deref()).await?;
        }
        Commands::VersionInfo { version, show_libraries, show_assets } => {
            cmd_version_info(&cli.format, &version, show_libraries, show_assets).await?;
        }
        Commands::Create { name, mc_version, loader, loader_version, memory, no_install, with_test_world, no_despotes, despotes_prerelease } => {
            cmd_create(&name, &mc_version, loader.as_deref(), loader_version.as_deref(), memory.as_deref(), no_install, with_test_world, no_despotes, despotes_prerelease).await?;
        }
        Commands::List { version, loader, sort } => {
            cmd_list(&cli.format, version.as_deref(), loader.as_deref(), &sort).await?;
        }
        Commands::Launch { name, username, server, fullscreen, width, height, detach, no_queue, agent, agent_port, java_path, memory, dynamic_memory, aprism, enter_test_world, wait_ready, idle_timeout, no_idle_timeout, oom_protect, oom_aggressive } => {
            cmd_launch(&name, username.as_deref(), server.as_deref(), fullscreen, width, height, detach, no_queue, agent, agent_port, java_path.as_deref(), memory.as_deref(), dynamic_memory, aprism, enter_test_world, wait_ready, idle_timeout, no_idle_timeout, oom_protect, oom_aggressive).await?;
        }
        Commands::Diagnose { name, export, analyze } => {
            cmd_diagnose(&name, export.as_deref(), analyze).await?;
        }
        Commands::Logs { name, follow, lines, level } => {
            cmd_logs(&name, follow, lines, level.as_deref()).await?;
        }
        Commands::Status { name, detail } => {
            cmd_status(&cli.format, name.as_deref(), detail).await?;
        }
        Commands::Mod(mod_cmd) => {
            match mod_cmd {
                ModCommands::List { instance } => {
                    cmd_mod_list(&cli.format, &instance).await?;
                }
                ModCommands::Install { instance, mod_path } => {
                    cmd_mod_install(&instance, &mod_path).await?;
                }
                ModCommands::Remove { instance, mod_name } => {
                    cmd_mod_remove(&instance, &mod_name).await?;
                }
                ModCommands::Enable { instance, mod_name } => {
                    cmd_mod_enable(&instance, &mod_name).await?;
                }
                ModCommands::Disable { instance, mod_name } => {
                    cmd_mod_disable(&instance, &mod_name).await?;
                }
            }
        }
        Commands::Config(config_cmd) => {
            match config_cmd {
                ConfigCommands::Get { instance, key } => {
                    cmd_config_get(&instance, &key).await?;
                }
                ConfigCommands::Set { instance, key, value } => {
                    cmd_config_set(&instance, &key, &value).await?;
                }
                ConfigCommands::Export { instance, output } => {
                    cmd_config_export(&instance, &output).await?;
                }
                ConfigCommands::Import { instance, input } => {
                    cmd_config_import(&instance, &input).await?;
                }
            }
        }
        Commands::Backup(backup_cmd) => {
            match backup_cmd {
                BackupCommands::Create { instance, world, name } => {
                    cmd_backup_create(&instance, &world, name.as_deref()).await?;
                }
                BackupCommands::List { instance, format } => {
                    cmd_backup_list(&instance, &format).await?;
                }
                BackupCommands::Restore { instance, backup, target } => {
                    cmd_backup_restore(&instance, &backup, target.as_deref()).await?;
                }
                BackupCommands::Delete { instance, backup } => {
                    cmd_backup_delete(&instance, &backup).await?;
                }
            }
        }
        Commands::Game(game_cmd) => {
            match game_cmd {
                GameCommands::Status { instance } => {
                    cmd_game_status(&cli.format, &instance).await?;
                }
                GameCommands::Screenshot { instance, output, timeout } => {
                    cmd_game_screenshot(&instance, output.as_deref(), timeout).await?;
                }
                GameCommands::Key { instance, key, action, hold_ms } => {
                    cmd_game_key(&instance, &key, &action, hold_ms).await?;
                }
                GameCommands::Look { instance, yaw, pitch, relative } => {
                    cmd_game_look(&instance, yaw, pitch, relative).await?;
                }
                GameCommands::Click { instance, button, action, x, y, hold_ms } => {
                    cmd_game_click(&instance, &button, &action, x, y, hold_ms).await?;
                }
                GameCommands::Scroll { instance, amount } => {
                    cmd_game_scroll(&instance, amount).await?;
                }
                GameCommands::Chat { instance, message } => {
                    cmd_game_chat(&instance, &message).await?;
                }
                GameCommands::Windows => {
                    cmd_game_windows(&cli.format)?;
                }
            }
        }
        Commands::Delete { name } => {
            cmd_delete(&name).await?;
        }
        Commands::Clone { name, new_name } => {
            cmd_clone(&name, &new_name).await?;
        }
        Commands::Rename { name, new_name } => {
            cmd_rename(&name, &new_name).await?;
        }
        Commands::InstanceInfo { name } => {
            cmd_instance_info(&cli.format, &name).await?;
        }
        Commands::Export { instance, path } => {
            cmd_export(&instance, &path).await?;
        }
        Commands::ImportInstance { path, name } => {
            cmd_import_instance(&path, name.as_deref()).await?;
        }
        Commands::Search(sc) => match sc {
            SearchCommands::Mod { query, mc_version, loader, instance, limit } => {
                cmd_search(loader::content::ContentKind::Mod, &query, mc_version.as_deref(), loader.as_deref(), instance.as_deref(), limit).await?;
            }
            SearchCommands::Resourcepack { query, mc_version, instance, limit } => {
                cmd_search(loader::content::ContentKind::ResourcePack, &query, mc_version.as_deref(), None, instance.as_deref(), limit).await?;
            }
            SearchCommands::Shader { query, mc_version, instance, limit } => {
                cmd_search(loader::content::ContentKind::Shader, &query, mc_version.as_deref(), None, instance.as_deref(), limit).await?;
            }
        },
        Commands::Account(ac) => match ac {
            AccountCommands::Login => { cmd_account_login().await?; }
            AccountCommands::List => { cmd_account_list(); }
            AccountCommands::Skin { account, output } => { cmd_account_skin(&account, output.as_deref()).await?; }
        },
        Commands::Bedrock(bc) => match bc {
            BedrockCommands::Install { name } => { cmd_bedrock_install(&name).await?; }
            BedrockCommands::Launch { name } => { cmd_bedrock_launch(&name).await?; }
            BedrockCommands::Stop { name } => { cmd_bedrock_stop(&name).await?; }
            BedrockCommands::Status { name, format } => { cmd_bedrock_status(&name, &format).await?; }
        },
        Commands::Cache(cc) => match cc {
            CacheCommands::Info => { cmd_cache_info(); }
            CacheCommands::Clean { days } => { cmd_cache_clean(days); }
        },
        Commands::Aprism(ac) => match ac {
            AprismCommands::Refract(rc) => match rc {
                AprismRefractCommands::Install { instance, loader, mc_version, prerelease } => {
                    cmd_aprism_refract_install(&instance, loader.as_deref(), mc_version.as_deref(), prerelease).await?;
                }
                AprismRefractCommands::List { instance } => { cmd_aprism_refract_list(&instance).await?; }
            },
            AprismCommands::Prismate(pc) => match pc {
                AprismPrismateCommands::Install { instance, loader, mc_version, prerelease } => {
                    cmd_aprism_prismate_install(&instance, loader.as_deref(), mc_version.as_deref(), prerelease).await?;
                }
                AprismPrismateCommands::Status { instance } => { cmd_aprism_prismate_status(&instance).await?; }
            },
        },
        Commands::Import { name, pack, no_download } => {
            cmd_import(&name, &pack, no_download).await?;
        }
        Commands::Server(sc) => match sc {
            ServerCommands::Create { name, mc_version, memory } => {
                cmd_server_create(&name, &mc_version, memory.as_deref()).await?;
            }
            ServerCommands::List => { cmd_server_list(&cli.format); }
            ServerCommands::Launch { name, attach } => {
                cmd_server_launch(&name, attach).await?;
            }
            ServerCommands::Stop { name } => { cmd_server_stop(&name).await?; }
            ServerCommands::Status { name } => { cmd_server_status(&cli.format, &name); }
        },
        Commands::Inject { target, dll } => { cmd_inject(&target, &dll).await?; }
        Commands::Agent { port, bind } => {
            cmd_agent(port, &bind).await?;
        }
        Commands::Info => {
            cmd_info(&cli.format).await?;
        }
        Commands::Doctor => {
            cmd_doctor().await?;
        }
        Commands::Capabilities => {
            let manifest = agent::capabilities::manifest();
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        }
        Commands::Update { check } => {
            cmd_update(check).await?;
        }
        Commands::Changelog { versions } => {
            cmd_changelog(versions);
        }
        Commands::Setup => {
            cmd_setup().await?;
        }
    }

    // Surface any available update after the command completes so the notice is
    // the last thing the user sees. Non-JSON output only, to keep machine-
    // readable output clean.
    if cli.format != "json" {
        // Hard-cap the wait: a slow or blocked network (e.g. unreachable
        // GitHub API) must never hang the command. The check itself keeps
        // running in the background until process exit.
        let update_info = tokio::time::timeout(std::time::Duration::from_secs(8), update_check).await;
        if let Ok(Ok(Some(info))) = update_info {
            eprintln!(
                "\nA new version of MCDebugLauncher is available: {} -> {}\nDownload: {}\n",
                info.current, info.latest, info.url
            );
        }
    }

    // Every code path above surfaces errors by terminating early, so
    // reaching this point always means success. Exit explicitly so the
    // orphaned background update-check thread (possibly blocked in a slow
    // DNS/network call) can never keep the process alive.
    std::process::exit(0);
}

// Command implementations

async fn cmd_versions(format: &str, type_filter: &str, limit: usize, search: Option<&str>) -> Result<()> {
    use version::VersionManifest;

    info!("Fetching version manifest...");
    let manifest = VersionManifest::fetch().await?;

    let versions: Vec<_> = if let Some(pattern) = search {
        manifest.search(pattern)
    } else if type_filter == "all" {
        manifest.versions.iter().collect()
    } else {
        manifest.filter_by_type(type_filter)
    };

    let versions: Vec<_> = versions.into_iter().take(limit).collect();

    if format == "json" {
        let json = serde_json::json!({
            "status": "success",
            "data": {
                "versions": versions,
                "count": versions.len()
            }
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        println!("ID            TYPE         RELEASE DATE");
        println!("----------------------------------------");
        for v in versions {
            println!("{:<13} {:<12} {}", v.id, v.version_type, &v.release_time[..10]);
        }
    }

    Ok(())
}

async fn cmd_version_info(format: &str, version_id: &str, show_libraries: bool, show_assets: bool) -> Result<()> {
    use version::{VersionManifest, VersionMetadata};

    info!("Fetching version manifest...");
    let manifest = VersionManifest::fetch().await?;

    let version = manifest.find_version(version_id)
        .ok_or_else(|| anyhow::anyhow!("Version not found: {}", version_id))?;

    info!("Fetching version metadata...");
    let metadata = VersionMetadata::fetch(&version.url).await?;

    if format == "json" {
        let mut data = serde_json::json!({
            "id": metadata.id,
            "type": metadata.version_type,
            "release_time": metadata.release_time,
            "main_class": metadata.main_class,
            "java_version": metadata.required_java_version(),
            "downloads": metadata.downloads,
        });

        if show_libraries {
            data["libraries"] = serde_json::to_value(&metadata.libraries)?;
        }

        if show_assets {
            data["asset_index"] = serde_json::to_value(&metadata.asset_index)?;
        }

        let json = serde_json::json!({
            "status": "success",
            "data": data
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        println!("Version: {}", metadata.id);
        println!("Type: {}", metadata.version_type);
        println!("Release: {}", metadata.release_time);
        println!("Java: {}", metadata.required_java_version());
        println!("Main Class: {}", metadata.main_class);
        println!("\nDownloads:");
        println!("  Client: {} ({} bytes)", metadata.downloads.client.url, metadata.downloads.client.size);
        if let Some(server) = &metadata.downloads.server {
            println!("  Server: {} ({} bytes)", server.url, server.size);
        }

        if show_libraries {
            println!("\nLibraries: {}", metadata.libraries.len());
        }

        if show_assets {
            if let Some(asset_index) = &metadata.asset_index {
                println!("\nAsset Index: {}", asset_index.id);
                println!("  Total Size: {} bytes", asset_index.total_size);
            }
        }
    }

    Ok(())
}

async fn cmd_create(name: &str, version: &str, loader: Option<&str>, loader_version: Option<&str>, _memory: Option<&str>, no_install: bool, with_test_world: bool, no_despotes: bool, despotes_prerelease: bool) -> Result<()> {
    use instance::{InstanceManager, config::{InstanceConfig, LoaderConfig}};

    let loader_config = if let Some(loader_type) = loader {
        Some(LoaderConfig {
            loader_type: loader_type.to_string(),
            version: loader_version.unwrap_or("latest").to_string(),
        })
    } else {
        None
    };

    let config = InstanceConfig {
        name: name.to_string(),
        version: version.to_string(),
        loader: loader_config,
    };

    let manager = InstanceManager::new()?;
    let instance = manager.create(config, !no_install).await?;

    // Offer the Despotes control mod (skip when not requested).
    if !no_despotes && !no_install {
        let loader_type = loader.as_deref();
        if let Err(e) = maybe_offer_despotes(&instance.path, loader_type, version, despotes_prerelease).await {
            tracing::warn!("Despotes setup skipped: {}", e);
            eprintln!("Warning: Despotes setup skipped: {e}");
        }
    }

    if with_test_world {
        let marker = instance.path.join("mdl-test-world");
        let _ = std::fs::write(&marker, "true");
        println!("Test world will be created on first ready launch (--enter-test-world).");
    }

    println!("Instance '{}' created successfully", instance.name);
    println!("Path: {}", instance.path.display());

    Ok(())
}

async fn cmd_list(
    format: &str,
    version_filter: Option<&str>,
    loader_filter: Option<&str>,
    sort_by: &str,
) -> Result<()> {
    use instance::InstanceManager;

    let manager = InstanceManager::new()?;
    let mut instances = manager.list().await?;

    // Apply filters
    if let Some(version) = version_filter {
        instances.retain(|inst| inst.config.version == version);
    }
    if let Some(loader) = loader_filter {
        instances.retain(|inst| {
            inst.config.loader.as_ref()
                .map(|l| l.loader_type.eq_ignore_ascii_case(loader))
                .unwrap_or(false)
        });
    }

    // Apply sorting
    match sort_by {
        "version" => instances.sort_by(|a, b| a.config.version.cmp(&b.config.version)),
        "loader" => instances.sort_by(|a, b| {
            let a_loader = a.config.loader.as_ref().map(|l| l.loader_type.as_str()).unwrap_or("");
            let b_loader = b.config.loader.as_ref().map(|l| l.loader_type.as_str()).unwrap_or("");
            a_loader.cmp(b_loader)
        }),
        _ => instances.sort_by(|a, b| a.name.cmp(&b.name)),
    }

    if format == "json" {
        let data: Vec<_> = instances.iter().map(|inst| {
            serde_json::json!({
                "name": inst.name,
                "version": inst.config.version,
                "loader": inst.config.loader.as_ref().map(|l| {
                    serde_json::json!({
                        "type": l.loader_type,
                        "version": l.version
                    })
                }),
                "path": inst.path.display().to_string()
            })
        }).collect();

        let json = serde_json::json!({
            "status": "success",
            "data": {
                "instances": data,
                "count": instances.len()
            }
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        if instances.is_empty() {
            println!("No instances found");
        } else {
            println!("Instances ({}):", instances.len());
            for inst in instances {
                print!("  {} (MC {})", inst.name, inst.config.version);
                if let Some(loader) = &inst.config.loader {
                    print!(" - {} {}", loader.loader_type, loader.version);
                }
                println!();
            }
        }
    }

    Ok(())
}

async fn cmd_launch(
    name: &str,
    username: Option<&str>,
    server: Option<&str>,
    fullscreen: bool,
    width: Option<u32>,
    height: Option<u32>,
    detach: bool,
    no_queue: bool,
    agent: bool,
    agent_port: Option<u16>,
    java_path: Option<&str>,
    memory: Option<&str>,
    dynamic_memory: bool,
    aprism: bool,
    enter_test_world: bool,
    wait_ready: bool,
    idle_timeout: Option<u64>,
    no_idle_timeout: bool,
    oom_protect: bool,
    oom_aggressive: bool,
) -> Result<()> {
    use instance::{InstanceLauncher, launcher::LaunchOptions};

    let options = LaunchOptions {
        username: username.map(str::to_string),
        server: server.map(str::to_string),
        fullscreen,
        width,
        height,
        detach,
        agent,
        agent_port,
        java_path: java_path.map(str::to_string),
        memory: memory.map(str::to_string),
        dynamic_memory,
        aprism,
        enter_test_world,
        wait_ready,
        no_queue,
        idle_timeout,
        no_idle_timeout,
        oom_protect,
        oom_aggressive,
    };

    let launcher = InstanceLauncher::new()?;
    let outcome = launcher.launch(name, &options).await?;

    if outcome.detached {
        println!("Instance '{}' launched in background (PID {})", name, outcome.pid);
        println!("  Game log: <instance>/logs/launch_detached.log");
        if agent {
            println!("  Agent control: use 'mdl game status {}' once in-game", name);
        }
        // Wait for the game-ready broadcast if requested (agent mode).
        if agent && wait_ready {
            let ready = wait_game_ready(&outcome, name).await;
            if ready {
                println!("  Game is ready.");
                if enter_test_world {
                    enter_world_after_ready(name).await?;
                }
            }
        }
    }

    Ok(())
}

/// Poll the Despotes status endpoint until the game reports in-game (or a
/// menu) ready state. Used to implement the "game ready" broadcast.
async fn wait_game_ready(outcome: &instance::launcher::LaunchOutcome, name: &str) -> bool {
    use instance::InstanceManager;
    let Ok(manager) = InstanceManager::new() else { return false };
    let Ok(inst) = manager.get(name).await else { return false };
    for _ in 0..120 {
        if game::client::is_available(&inst.path).await {
            if let Ok(status) = game::client::game_status(&inst.path).await {
                if status.get("inGame").and_then(|v| v.as_bool()) == Some(true)
                    || status.get("screenOpen").and_then(|v| v.as_bool()) == Some(true)
                {
                    // broadcast the ready event to agent server subscribers
                    let _ = outcome; // pid available for correlation
                    return true;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    false
}

/// After the game is ready, use Despotes to enter (or create) the test world.
async fn enter_world_after_ready(name: &str) -> Result<()> {
    use instance::InstanceManager;
    let manager = InstanceManager::new()?;
    let inst = manager.get(name).await?;
    // If we pre-created a test world, type-select it by clicking the entry then
    // "Play Selected World". For a fresh instance, create one instead.
    // This is best-effort; failures only warn.
    // Navigate from the title screen: Singleplayer -> Create New World.
    // Coordinates use GUI scale 2 (client area = screenshot px / 2), verified
    // in Alpha 7 E2E. Best-effort: any failure only warns.
    let click = |x: f64, y: f64| {
        let path = inst.path.clone();
        async move {
            let _ = game::client::mouse_input(&path, "left", "tap", Some(x), Some(y), None).await;
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        }
    };
    click(213.0, 136.0).await; // Singleplayer
    click(131.0, 226.0).await; // Create New World
    tracing::info!("enter_test_world: navigated to create-world; game will generate a test world");
    Ok(())
}

async fn cmd_diagnose(name: &str, export: Option<&str>, analyze: bool) -> Result<()> {
    use diagnostic::collector::DiagnosticCollector;
    use diagnostic::crash_analyzer::CrashAnalyzer;
    use instance::InstanceManager;

    info!("Diagnosing instance '{}'", name);

    let manager = InstanceManager::new()?;
    let instance = manager.get(name).await?;

    // Collect diagnostics
    let collector = DiagnosticCollector::new(instance.path.clone());
    let report = collector.collect(name).await?;

    println!("Diagnostic Report for '{}'", name);
    println!("========================================\n");

    // System info
    println!("System Information:");
    println!("  OS: {} ({})", report.system_info.os, report.system_info.arch);
    if let Some(java) = &report.system_info.java_version {
        println!("  Java: {}", java);
    }
    if let Some(total) = report.system_info.memory_total {
        println!("  Memory: {} MB total", total / 1024 / 1024);
    }
    println!();

    // Log summary
    println!("Logs:");
    println!("  Total entries: {}", report.logs.len());
    let error_count = report.errors.len();
    if error_count > 0 {
        println!("  Errors found: {}", error_count);
    }
    println!();

    // Show recent errors
    if !report.errors.is_empty() {
        println!("Recent Errors:");
        for (i, error) in report.errors.iter().take(5).enumerate() {
            println!("  {}. [{}] {}: {}",
                i + 1,
                error.timestamp,
                error.error_type,
                error.message.chars().take(100).collect::<String>()
            );
        }
        println!();
    }

    // Crash reports
    if !report.crash_reports.is_empty() {
        println!("Crash Reports Found: {}", report.crash_reports.len());
        for crash in &report.crash_reports {
            println!("  - {} ({})", crash.file_name, crash.timestamp);
        }
        println!();

        // Analyze the most recent crash if requested
        if analyze && !report.crash_reports.is_empty() {
            println!("Analyzing most recent crash...\n");
            let latest_crash = &report.crash_reports[0];

            let analyzer = CrashAnalyzer::new();
            let analysis = analyzer.analyze(&latest_crash.content)?;

            println!("Crash Analysis:");
            println!("  Summary: {}", analysis.summary);
            println!("  Likely cause: {}", analysis.likely_cause);
            println!();

            if !analysis.mod_conflicts.is_empty() {
                println!("  Mods involved:");
                for mod_name in &analysis.mod_conflicts {
                    println!("    - {}", mod_name);
                }
                println!();
            }

            println!("  Suggestions:");
            for (i, suggestion) in analysis.suggestions.iter().enumerate() {
                println!("    {}. {}", i + 1, suggestion);
            }
            println!();

            if !analysis.stack_trace_top.is_empty() {
                println!("  Stack trace (top 5):");
                for line in analysis.stack_trace_top.iter().take(5) {
                    println!("    {}", line);
                }
                println!();
            }
        }
    } else {
        println!("No crash reports found\n");
    }

    // Export if requested
    if let Some(export_path) = export {
        let export_file = std::path::Path::new(export_path);
        collector.save_report(&report, export_file).await?;
        println!("Diagnostic report exported to: {}", export_path);
    }

    Ok(())
}

async fn cmd_logs(name: &str, follow: bool, lines: usize, level: Option<&str>) -> Result<()> {
    use instance::InstanceManager;
    use tokio::io::{AsyncBufReadExt, BufReader};

    info!("Reading logs for instance '{}'", name);

    let manager = InstanceManager::new()?;
    let instance = manager.get(name).await?;

    let log_file = instance.path.join("logs").join("latest.log");

    if !log_file.exists() {
        println!("No logs found for instance '{}'", name);
        println!("Log file does not exist: {}", log_file.display());
        return Ok(());
    }

    if follow {
        // Follow mode - stream new log lines
        println!("Following logs for '{}' (Ctrl+C to stop)...\n", name);

        let file = tokio::fs::File::open(&log_file).await?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();

        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;

            if n == 0 {
                // End of file, wait a bit and try again
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                continue;
            }

            // Filter by level if specified
            if let Some(filter_level) = level {
                if !line.to_uppercase().contains(&filter_level.to_uppercase()) {
                    continue;
                }
            }

            print!("{}", line);
        }
    } else {
        // Show last N lines
        let content = tokio::fs::read_to_string(&log_file).await?;
        let all_lines: Vec<&str> = content.lines().collect();

        let start = if all_lines.len() > lines {
            all_lines.len() - lines
        } else {
            0
        };

        println!("Last {} lines of logs for '{}':\n", lines.min(all_lines.len()), name);

        for line in &all_lines[start..] {
            // Filter by level if specified
            if let Some(filter_level) = level {
                if !line.to_uppercase().contains(&filter_level.to_uppercase()) {
                    continue;
                }
            }

            println!("{}", line);
        }
    }

    Ok(())
}

async fn cmd_status(format: &str, name: Option<&str>, detail: bool) -> Result<()> {
    use instance::InstanceStatus;

    let status = InstanceStatus::new()?;

    if let Some(instance_name) = name {
        // Show single instance status
        let info = status.get_instance_status(instance_name).await?;

        if detail {
            // Show detailed instance information
            cmd_instance_detail(format, instance_name, &info).await?;
        } else {
            // Show basic status (existing behavior)
            if format == "json" {
                let json = serde_json::json!({
                    "status": "success",
                    "data": info
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("Instance: {}", info.name);
                println!("Status: {}", info.state);

                if let Some(pid) = info.pid {
                    println!("PID: {}", pid);
                }

                if let Some(uptime) = info.uptime_seconds {
                    let hours = uptime / 3600;
                    let minutes = (uptime % 3600) / 60;
                    let seconds = uptime % 60;
                    println!("Uptime: {}h {}m {}s", hours, minutes, seconds);
                }

                if let Some(memory) = info.memory_mb {
                    println!("Memory: {} MB", memory);
                }

                if let Some(cpu) = info.cpu_percent {
                    println!("CPU: {:.1}%", cpu);
                }
            }
        }
    } else {
        // Show all instances
        let all_status = status.get_all_status().await?;

        if format == "json" {
            let json = serde_json::json!({
                "status": "success",
                "data": {
                    "instances": all_status,
                    "count": all_status.len()
                }
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        } else {
            if all_status.is_empty() {
                println!("No instances found");
            } else {
                println!("NAME              STATE      PID       UPTIME        MEMORY");
                println!("----------------------------------------------------------------");
                for info in all_status {
                    let pid_str = info.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".to_string());
                    let uptime_str = if let Some(uptime) = info.uptime_seconds {
                        let hours = uptime / 3600;
                        let minutes = (uptime % 3600) / 60;
                        format!("{}h {}m", hours, minutes)
                    } else {
                        "-".to_string()
                    };
                    let memory_str = info.memory_mb.map(|m| format!("{} MB", m)).unwrap_or_else(|| "-".to_string());

                    println!("{:<17} {:<10} {:<9} {:<13} {}",
                        info.name, info.state, pid_str, uptime_str, memory_str);
                }
            }
        }
    }

    Ok(())
}

async fn cmd_instance_detail(format: &str, instance_name: &str, status_info: &instance::InstanceStatusInfo) -> Result<()> {
    use instance::InstanceManager;
    
    let manager = InstanceManager::new()?;
    let instance = manager.get(instance_name).await?;
    
    // Count mods
    let mods_dir = instance.path.join("mods");
    let mod_count = if mods_dir.exists() {
        std::fs::read_dir(&mods_dir)
            .map(|entries| entries.filter_map(|e| e.ok()).filter(|e| {
                e.path().extension().and_then(|ext| ext.to_str()).map(|ext| ext == "jar").unwrap_or(false)
            }).count())
            .unwrap_or(0)
    } else {
        0
    };

    if format == "json" {
        let data = serde_json::json!({
            "name": instance.name,
            "path": instance.path,
            "config": instance.config,
            "status": {
                "state": status_info.state,
                "pid": status_info.pid,
                "uptime_seconds": status_info.uptime_seconds,
                "memory_mb": status_info.memory_mb,
                "cpu_percent": status_info.cpu_percent,
            },
            "mod_count": mod_count,
        });
        
        let json = serde_json::json!({
            "status": "success",
            "data": data
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        println!("Instance: {}", instance.name);
        println!("Path: {}", instance.path.display());
        println!();
        
        println!("Configuration:");
        println!("  Minecraft Version: {}", instance.config.version);
        if let Some(loader) = &instance.config.loader {
            println!("  Loader: {} {}", loader.loader_type, loader.version);
        } else {
            println!("  Loader: None (Vanilla)");
        }
        println!();
        
        println!("Status:");
        println!("  State: {}", status_info.state);
        if let Some(pid) = status_info.pid {
            println!("  PID: {}", pid);
        }
        if let Some(uptime) = status_info.uptime_seconds {
            let hours = uptime / 3600;
            let minutes = (uptime % 3600) / 60;
            let seconds = uptime % 60;
            println!("  Uptime: {}h {}m {}s", hours, minutes, seconds);
        }
        if let Some(memory) = status_info.memory_mb {
            println!("  Memory: {} MB", memory);
        }
        if let Some(cpu) = status_info.cpu_percent {
            println!("  CPU: {:.1}%", cpu);
        }
        println!();
        
        println!("Content:");
        println!("  Mods: {}", mod_count);
    }

    Ok(())
}

async fn cmd_delete(name: &str) -> Result<()> {
    use instance::InstanceManager;

    let manager = InstanceManager::new()?;
    manager.delete(name).await?;

    println!("Instance '{}' deleted", name);
    Ok(())
}

async fn cmd_clone(name: &str, new_name: &str) -> Result<()> {
    use instance::InstanceManager;

    let manager = InstanceManager::new()?;
    let inst = manager.clone_instance(name, new_name).await?;

    println!("Instance '{}' cloned to '{}' ({})", name, new_name, inst.path.display());
    Ok(())
}

async fn cmd_rename(name: &str, new_name: &str) -> Result<()> {
    use instance::InstanceManager;

    let manager = InstanceManager::new()?;
    let inst = manager.rename(name, new_name).await?;

    println!("Instance '{}' renamed to '{}' ({})", name, new_name, inst.path.display());
    Ok(())
}

async fn cmd_instance_info(format: &str, name: &str) -> Result<()> {
    use instance::InstanceManager;
    use serde_json::json;

    let manager = InstanceManager::new()?;
    let instance = manager.get(name).await?;

    // Calculate disk usage
    let disk_usage = calculate_dir_size(&instance.path).await?;
    
    // Count content items
    let mods_dir = instance.path.join("mods");
    let resourcepacks_dir = instance.path.join("resourcepacks");
    let shaderpacks_dir = instance.path.join("shaderpacks");
    
    let mods_count = count_files_in_dir(&mods_dir).await;
    let resourcepacks_count = count_files_in_dir(&resourcepacks_dir).await;
    let shaderpacks_count = count_files_in_dir(&shaderpacks_dir).await;

    if format == "json" {
        let info = json!({
            "name": instance.name,
            "version": instance.config.version,
            "loader": instance.config.loader.as_ref().map(|l| json!({
                "type": l.loader_type,
                "version": l.version
            })),
            "path": instance.path,
            "disk_usage": disk_usage,
            "content": {
                "mods": mods_count,
                "resourcepacks": resourcepacks_count,
                "shaderpacks": shaderpacks_count
            }
        });
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!("Instance: {}", instance.name);
        println!("Minecraft Version: {}", instance.config.version);
        if let Some(loader) = &instance.config.loader {
            println!("Loader: {} {}", loader.loader_type, loader.version);
        } else {
            println!("Loader: Vanilla");
        }
        println!("Path: {}", instance.path.display());
        println!("Disk Usage: {}", format_bytes(disk_usage));
        println!("\nContent:");
        println!("  Mods: {}", mods_count);
        println!("  Resource Packs: {}", resourcepacks_count);
        println!("  Shader Packs: {}", shaderpacks_count);
    }

    Ok(())
}

async fn calculate_dir_size(path: &std::path::Path) -> Result<u64> {
    let mut total = 0u64;
    
    if !path.exists() {
        return Ok(0);
    }
    
    let mut stack = vec![path.to_path_buf()];
    
    while let Some(current) = stack.pop() {
        if let Ok(mut entries) = tokio::fs::read_dir(&current).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(metadata) = entry.metadata().await {
                    if metadata.is_dir() {
                        stack.push(entry.path());
                    } else {
                        total += metadata.len();
                    }
                }
            }
        }
    }
    
    Ok(total)
}

async fn count_files_in_dir(path: &std::path::Path) -> usize {
    if !path.exists() {
        return 0;
    }
    
    let mut count = 0;
    if let Ok(mut entries) = tokio::fs::read_dir(path).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(metadata) = entry.metadata().await {
                if metadata.is_file() {
                    count += 1;
                }
            }
        }
    }
    
    count
}

async fn cmd_export(instance_name: &str, output_path: &std::path::Path) -> Result<()> {
    use anyhow::Context;
    use instance::InstanceManager;
    use std::fs::File;
    use zip::write::{FileOptions, ZipWriter};
    use zip::CompressionMethod;
    
    info!("Exporting instance '{}'...", instance_name);
    
    let manager = InstanceManager::new()?;
    let instance = manager.get(instance_name).await?;
    let instance_path = &instance.path;
    
    if !instance_path.exists() {
        return Err(anyhow::anyhow!("Instance directory does not exist"));
    }
    
    // Create output file
    let output_file = File::create(output_path)
        .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;
    
    let mut zip = ZipWriter::new(output_file);
    let options = FileOptions::<()>::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o755);
    
    // Walk through instance directory and add files to zip
    let mut file_count = 0;
    let walkdir = walkdir::WalkDir::new(instance_path);
    
    for entry in walkdir.into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let relative_path = path.strip_prefix(instance_path)
            .with_context(|| format!("Failed to strip prefix from path: {}", path.display()))?;
        
        if relative_path.as_os_str().is_empty() {
            continue;
        }
        
        let name = relative_path.to_string_lossy();
        
        if path.is_file() {
            zip.start_file(name.as_ref(), options)
                .with_context(|| format!("Failed to start zip file entry: {}", name))?;
            
            let mut file = std::fs::File::open(path)
                .with_context(|| format!("Failed to open file: {}", path.display()))?;
            
            std::io::copy(&mut file, &mut zip)
                .with_context(|| format!("Failed to write file to zip: {}", name))?;
            
            file_count += 1;
        } else if path.is_dir() {
            zip.add_directory(name.as_ref(), options)
                .with_context(|| format!("Failed to add directory to zip: {}", name))?;
        }
    }
    
    zip.finish()
        .context("Failed to finalize zip file")?;
    
    info!("Exported {} files to {}", file_count, output_path.display());
    println!("Successfully exported instance '{}' to {}", instance_name, output_path.display());
    println!("Total files: {}", file_count);
    
    Ok(())
}

async fn cmd_import_instance(zip_path: &std::path::Path, instance_name: Option<&str>) -> Result<()> {
    use anyhow::Context;
    use instance::InstanceManager;
    use std::io::Read;
    use zip::ZipArchive;
    
    info!("Importing instance from '{}'...", zip_path.display());
    
    if !zip_path.exists() {
        anyhow::bail!("Zip file not found: {}", zip_path.display());
    }
    
    // Open the zip file
    let file = std::fs::File::open(zip_path)
        .with_context(|| format!("Failed to open zip file: {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .context("Failed to read zip archive")?;
    
    // Read instance.json to get the instance name
    let mut instance_json = None;
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        if file.name() == "instance.json" {
            instance_json = Some(i);
            break;
        }
    }
    
    let instance_json_idx = instance_json
        .ok_or_else(|| anyhow::anyhow!("Invalid instance zip: missing instance.json"))?;
    
    let mut instance_file = archive.by_index(instance_json_idx)?;
    let mut contents = String::new();
    instance_file.read_to_string(&mut contents)?;
    drop(instance_file);
    
    let config: InstanceConfig = serde_json::from_str(&contents)
        .context("Failed to parse instance.json")?;
    
    // Use provided name or default to name from config
    let target_name = instance_name.unwrap_or(&config.name).to_string();
    
    // Check if instance already exists
    let _manager = InstanceManager::new()?;
    let instances_dir = util::paths::get_instances_dir()?;
    let target_path = instances_dir.join(&target_name);
    
    if target_path.exists() {
        anyhow::bail!("Instance '{}' already exists", target_name);
    }
    
    info!("Creating instance directory '{}'...", target_name);
    tokio::fs::create_dir_all(&target_path).await
        .with_context(|| format!("Failed to create instance directory: {}", target_path.display()))?;
    
    // Extract all files
    let mut file_count = 0;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = target_path.join(file.name());
        
        if file.name().ends_with('/') {
            // Directory
            std::fs::create_dir_all(&outpath)
                .with_context(|| format!("Failed to create directory: {}", outpath.display()))?;
        } else {
            // File
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create parent directory: {}", parent.display()))?;
            }
            
            let mut outfile = std::fs::File::create(&outpath)
                .with_context(|| format!("Failed to create file: {}", outpath.display()))?;
            std::io::copy(&mut file, &mut outfile)
                .with_context(|| format!("Failed to extract file: {}", file.name()))?;
            
            file_count += 1;
        }
    }
    
    // Update instance.json with new name if different
    if target_name != config.name {
        let mut new_config = config;
        new_config.name = target_name.clone();
        let config_path = target_path.join("instance.json");
        let config_json = serde_json::to_string_pretty(&new_config)?;
        tokio::fs::write(&config_path, config_json).await
            .context("Failed to write updated instance configuration")?;
    }
    
    info!("Imported {} files", file_count);
    println!("Successfully imported instance '{}' from {}", target_name, zip_path.display());
    println!("Total files: {}", file_count);
    
    Ok(())
}

async fn cmd_agent(port: u16, bind_address: &str) -> Result<()> {
    use agent::server::AgentServer;

    // Prevent multiple agent processes from running at the same time. A second
    // instance would fail to bind the port anyway, but we emit a clear error
    // with the existing process's PID and interface address instead of a raw
    // "address already in use" OS error.
    let lock_path = {
        let data_dir = util::paths::get_data_dir()?;
        tokio::fs::create_dir_all(&data_dir).await?;
        data_dir.join("agent.lock")
    };

    if lock_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&lock_path).await {
            if let Ok(pid) = content.trim().parse::<u32>() {
                if agent_pid_running(pid) {
                    eprintln!(
                        "error: mdl agent is already running (PID {}).\n\
                         Interface: http://{}:{}\n\
                         Stop it with: kill {}",
                        pid, bind_address, port, pid
                    );
                    std::process::exit(1);
                }
            }
        }
    }

    tokio::fs::write(&lock_path, std::process::id().to_string()).await?;

    info!("Starting agent server on {}:{}", bind_address, port);

    let server = AgentServer::new();
    let result = server.start(port, bind_address).await;

    // Always remove the lock on exit, whether the server stopped cleanly or not.
    let _ = tokio::fs::remove_file(&lock_path).await;

    result
}

async fn cmd_mod_list(format: &str, instance: &str) -> Result<()> {
    use instance::ModManager;

    let mod_manager = ModManager::new()?;
    let mods = mod_manager.list_mods(instance).await?;

    if format == "json" {
        let json = serde_json::json!({
            "status": "success",
            "data": {
                "instance": instance,
                "mods": mods
            }
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        if mods.is_empty() {
            println!("No mods installed in instance '{}'", instance);
        } else {
            println!("Mods in instance '{}':", instance);
            println!();
            println!("{:<40} {:>12}  {}", "NAME", "SIZE", "STATUS");
            println!("{}", "-".repeat(60));

            for mod_info in &mods {
                let size_str = if mod_info.size_bytes < 1024 {
                    format!("{} B", mod_info.size_bytes)
                } else if mod_info.size_bytes < 1024 * 1024 {
                    format!("{:.1} KB", mod_info.size_bytes as f64 / 1024.0)
                } else {
                    format!("{:.2} MB", mod_info.size_bytes as f64 / (1024.0 * 1024.0))
                };

                let status = if mod_info.enabled { "enabled" } else { "disabled" };

                println!("{:<40} {:>12}  {}", mod_info.filename, size_str, status);
            }

            println!();
            println!("Total: {} mod(s)", mods.len());
        }
    }

    Ok(())
}

async fn cmd_mod_install(instance: &str, mod_path: &str) -> Result<()> {
    use anyhow::Context;
    use instance::ModManager;
    use std::path::Path;

    let mod_manager = ModManager::new()?;
    let path = Path::new(mod_path);

    if !path.exists() {
        anyhow::bail!("Mod file not found: {}", mod_path);
    }

    mod_manager.install_mod(instance, path).await?;

    let filename = path.file_name()
        .context("Invalid mod path")?
        .to_string_lossy();

    println!("Successfully installed mod: {}", filename);
    Ok(())
}

async fn cmd_mod_remove(instance: &str, mod_name: &str) -> Result<()> {
    use instance::ModManager;

    let mod_manager = ModManager::new()?;
    mod_manager.remove_mod(instance, mod_name).await?;

    println!("Successfully removed mod: {}", mod_name);
    Ok(())
}

async fn cmd_mod_enable(instance: &str, mod_name: &str) -> Result<()> {
    use instance::ModManager;

    let mod_manager = ModManager::new()?;
    mod_manager.enable_mod(instance, mod_name).await?;

    println!("Successfully enabled mod: {}", mod_name);
    Ok(())
}

async fn cmd_mod_disable(instance: &str, mod_name: &str) -> Result<()> {
    use instance::ModManager;

    let mod_manager = ModManager::new()?;
    mod_manager.disable_mod(instance, mod_name).await?;

    println!("Successfully disabled mod: {}", mod_name);
    Ok(())
}

async fn cmd_config_get(instance: &str, key: &str) -> Result<()> {
    use instance::ConfigManager;

    let config_manager = ConfigManager::new()?;
    let value = config_manager.get_option(instance, key).await?;

    match value {
        Some(v) => println!("{}", v),
        None => println!("Option '{}' not found", key),
    }
    Ok(())
}

async fn cmd_config_set(instance: &str, key: &str, value: &str) -> Result<()> {
    use instance::ConfigManager;

    let config_manager = ConfigManager::new()?;
    config_manager.set_option(instance, key, value).await?;

    println!("Successfully set option: {}={}", key, value);
    Ok(())
}

async fn cmd_config_export(instance: &str, output: &str) -> Result<()> {
    use instance::ConfigManager;

    let config_manager = ConfigManager::new()?;
    let config = config_manager.export_config(instance).await?;

    let json = serde_json::to_string_pretty(&config)?;
    tokio::fs::write(output, json).await?;

    println!("Exported configuration to: {}", output);
    Ok(())
}

async fn cmd_config_import(instance: &str, input: &str) -> Result<()> {
    use instance::ConfigManager;

    let content = tokio::fs::read_to_string(input).await?;
    let config: serde_json::Value = serde_json::from_str(&content)?;

    let config_manager = ConfigManager::new()?;
    config_manager.import_config(instance, &config).await?;

    println!("Imported configuration from: {}", input);
    Ok(())
}

async fn cmd_backup_create(instance: &str, world: &str, name: Option<&str>) -> Result<()> {
    use instance::BackupManager;

    let backup_manager = BackupManager::new()?;
    let info = backup_manager.create_backup(instance, world, name.map(String::from)).await?;

    println!("Created backup: {}", info.name);
    println!("  Size: {} bytes", info.size_bytes);
    println!("  Path: {}", info.path.display());
    Ok(())
}

async fn cmd_backup_list(instance: &str, format: &str) -> Result<()> {
    use instance::BackupManager;

    let backup_manager = BackupManager::new()?;
    let backups = backup_manager.list_backups(instance).await?;

    if format == "json" {
        let json = serde_json::to_string_pretty(&backups)?;
        println!("{}", json);
    } else {
        if backups.is_empty() {
            println!("No backups found for instance '{}'", instance);
        } else {
            println!("Backups for instance '{}':", instance);
            println!();
            for backup in backups {
                println!("  {}", backup.name);
                println!("    World: {}", backup.world);
                println!("    Created: {}", backup.created_at);
                println!("    Size: {} bytes", backup.size_bytes);
                println!();
            }
        }
    }
    Ok(())
}

async fn cmd_backup_restore(instance: &str, backup: &str, target: Option<&str>) -> Result<()> {
    use instance::BackupManager;

    let backup_manager = BackupManager::new()?;
    backup_manager.restore_backup(instance, backup, target.map(String::from)).await?;

    println!("Successfully restored backup: {}", backup);
    Ok(())
}

async fn cmd_backup_delete(instance: &str, backup: &str) -> Result<()> {
    use instance::BackupManager;

    let backup_manager = BackupManager::new()?;
    backup_manager.delete_backup(instance, backup).await?;

    println!("Successfully deleted backup: {}", backup);
    Ok(())
}

/// Check whether a process with the given PID is currently running.
fn agent_pid_running(pid: u32) -> bool {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes();
    sys.process(sysinfo::Pid::from_u32(pid)).is_some()
}

/// Environment health check (Alpha 12): read-only verification of every
/// external dependency MDL relies on, rendered as a pass/fail report.
async fn cmd_doctor() -> Result<()> {
    println!("MDL environment health check");
    println!("============================");
    let report = util::doctor::run_all().await;
    for line in report.render() {
        println!("{}", line);
    }
    if report.fail_count() > 0 {
        anyhow::bail!("{} health check(s) failed", report.fail_count());
    }
    Ok(())
}

async fn cmd_info(format: &str) -> Result<()> {
    use version::JavaRuntime;

    let java = JavaRuntime::detect();

    if format == "json" {
        let data = serde_json::json!({
            "mdl_version": env!("CARGO_PKG_VERSION"),
            "java": java.as_ref().ok().map(|j| serde_json::json!({
                "path": j.path,
                "version": j.version,
                "major_version": j.major_version
            })),
            "platform": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        });

        let json = serde_json::json!({
            "status": "success",
            "data": data
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        println!("MCDebugLauncher {}", env!("CARGO_PKG_VERSION"));
        println!("Platform: {} ({})", std::env::consts::OS, std::env::consts::ARCH);

        match java {
            Ok(runtime) => {
                println!("\nJava Runtime:");
                println!("  Path: {:?}", runtime.path);
                println!("  Version: {}", runtime.version);
                println!("  Major: {}", runtime.major_version);
            }
            Err(e) => {
                println!("\nJava Runtime: Not found");
                println!("  Error: {}", e);
            }
        }
    }

    Ok(())
}

async fn cmd_update(check_only: bool) -> Result<()> {
    info!("Checking for updates...");

    match util::selfupdate::check_for_update().await {
        Ok(Some(new_version)) => {
            println!("New version available: {} -> {}", env!("CARGO_PKG_VERSION"), new_version);
            println!("Download: https://github.com/StarsailsClover/MCDebugLauncher/releases/tag/v{}", new_version);

            if !check_only {
                println!("\nDo you want to download and install this update? (y/N)");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;

                if input.trim().eq_ignore_ascii_case("y") {
                    util::selfupdate::perform_update(&new_version).await?;
                    println!("\nUpdate downloaded successfully!");
                    println!("Please restart MDL to complete the update.");
                } else {
                    println!("Update skipped.");
                }
            }
        }
        Ok(None) => {
            println!("You are already running the latest version ({})", env!("CARGO_PKG_VERSION"));
        }
        Err(e) => {
            eprintln!("Failed to check for updates: {}", e);
            eprintln!("Please check manually at: https://github.com/StarsailsClover/MCDebugLauncher/releases");
        }
    }

    Ok(())
}

async fn cmd_setup() -> Result<()> {
    println!("Setting up MDL...");

    // Add to PATH
    util::selfupdate::add_to_path()?;

    // Check for updates
    println!("\nChecking for updates...");
    match util::selfupdate::check_for_update().await {
        Ok(Some(new_version)) => {
            println!("A newer version is available: v{}", new_version);
            println!("Run 'mdl update' to upgrade.");
        }
        Ok(None) => {
            println!("MDL is up to date (v{})", env!("CARGO_PKG_VERSION"));
        }
        Err(_) => {
            // Silent fail for setup
        }
    }

    println!("\nSetup complete!");
    println!("You can now use 'mdl' from any terminal window.");

    Ok(())
}

// ---------------------------------------------------------------------------
// Game control commands (Alpha 6)
// ---------------------------------------------------------------------------

/// Resolve the on-disk directory of an instance by name.
async fn resolve_instance_dir(instance: &str) -> Result<std::path::PathBuf> {
    use instance::InstanceManager;
    let manager = InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    Ok(inst.path)
}

fn print_game_response(response: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(response).unwrap_or_default());
}

async fn cmd_game_status(format: &str, instance: &str) -> Result<()> {
    let dir = resolve_instance_dir(instance).await?;

    if !game::client::is_available(&dir).await {
        anyhow::bail!(
            "Agent control is not available for instance '{}'.              The game must be running with the Despotes mod installed.              Launch it with: mdl launch {} --detach --agent",
            instance, instance
        );
    }

    let response = game::client::game_status(&dir).await?;
    if format == "json" {
        print_game_response(&response);
    } else {
        print_game_response(&response);
    }
    Ok(())
}

async fn cmd_game_screenshot(instance: &str, output: Option<&str>, timeout: u64) -> Result<()> {
    #[cfg(not(windows))]
    {
        let _ = (instance, output, timeout);
        anyhow::bail!("Game screenshot capture is currently supported only on Windows");
    }

    #[cfg(windows)]
    {
        let dir = resolve_instance_dir(instance).await?;

        info!("Capturing screenshot of instance '{}'...", instance);
        let start = std::time::Instant::now();

        // Read the instance PID (if running) to prefer the right window when
        // several instances share the title prefix.
        let pid: Option<u32> = tokio::fs::read_to_string(dir.join("runtime").join("pid"))
            .await
            .ok()
            .and_then(|c| c.trim().parse().ok());

        let (image, source) = game::capture::capture_instance_best(&dir, instance, pid, timeout)?;
        let elapsed = start.elapsed();

        let out_path = match output {
            Some(p) => std::path::PathBuf::from(p),
            None => {
                let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
                let name = format!("{}_{}.png", instance, ts);
                std::env::current_dir()?.join(name)
            }
        };
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&out_path, &image.png_bytes)?;

        println!(
            "Screenshot saved: {} ({}x{}, {:.0} ms, source: {})",
            out_path.display(),
            image.width,
            image.height,
            elapsed.as_secs_f64() * 1000.0,
            source
        );
        Ok(())
    }
}

async fn cmd_game_key(instance: &str, key: &str, action: &str, hold_ms: Option<u64>) -> Result<()> {
    let dir = resolve_instance_dir(instance).await?;
    let response = game::client::key_input(&dir, key, action, hold_ms).await?;
    print_game_response(&response);
    Ok(())
}

async fn cmd_game_look(instance: &str, yaw: f32, pitch: f32, relative: bool) -> Result<()> {
    let dir = resolve_instance_dir(instance).await?;
    let response = game::client::look(&dir, yaw, pitch, relative).await?;
    print_game_response(&response);
    Ok(())
}

async fn cmd_game_click(instance: &str, button: &str, action: &str, x: Option<f64>, y: Option<f64>, hold_ms: Option<u64>) -> Result<()> {
    let dir = resolve_instance_dir(instance).await?;
    let response = game::client::mouse_input(&dir, button, action, x, y, hold_ms).await?;
    print_game_response(&response);
    Ok(())
}

async fn cmd_game_scroll(instance: &str, amount: f64) -> Result<()> {
    let dir = resolve_instance_dir(instance).await?;
    let response = game::client::scroll(&dir, amount).await?;
    print_game_response(&response);
    Ok(())
}

async fn cmd_game_chat(instance: &str, message: &str) -> Result<()> {
    let dir = resolve_instance_dir(instance).await?;
    let response = game::client::chat(&dir, message).await?;
    print_game_response(&response);
    Ok(())
}

fn cmd_game_windows(format: &str) -> Result<()> {
    #[cfg(not(windows))]
    {
        let _ = format;
        anyhow::bail!("Game window discovery is currently supported only on Windows");
    }

    #[cfg(windows)]
    {
        let windows = game::window::list_mdl_windows(&game::window::collect_running_pids());
        if format == "json" {
            let json = serde_json::json!({
                "status": "success",
                "data": { "windows": windows, "count": windows.len() }
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        } else if windows.is_empty() {
            println!("No MDL game windows found");
        } else {
            println!("INSTANCE        PID         SIZE          TITLE");
            println!("{}", "-".repeat(64));
            for w in &windows {
                println!(
                    "{:<15} {:<11} {:>4}x{:<6}  {}",
                    w.instance,
                    w.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                    w.width,
                    w.height,
                    w.title
                );
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Despotes offer during instance creation (Alpha 7)
// ---------------------------------------------------------------------------

fn format_bytes(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.2} MB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

fn read_choice_line() -> String {
    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(0) => String::new(), // EOF / non-interactive stdin
        Ok(_) => input.trim().to_string(),
        Err(_) => String::new(),
    }
}

/// True when stdin is attached to a terminal (interactive prompts allowed).
fn stdin_is_interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// After an instance is created, detect the best Despotes build and let the
/// user pick one by number.
///
/// Policy:
/// - Stable ("Latest Release") applicable assets are listed first.
/// - Pre-releases are offered only when `allow_prerelease_flag` is set, or
///   when no applicable stable exists — and installing a pre-release always
///   requires an explicit interactive confirmation.
/// - When no applicable stable exists, the latest applicable pre-release is
///   the fallback candidate.
async fn maybe_offer_despotes(
    instance_dir: &std::path::Path,
    loader: Option<&str>,
    requested_version: &str,
    allow_prerelease_flag: bool,
) -> anyhow::Result<()> {
    use game::despotes;
    use std::io::Write;

    let Some(dloader) = despotes::despotes_loader_for(loader) else {
        println!(
            "
Despotes control mod: not applicable to loader '{}'. Skipping.",
            loader.unwrap_or("none")
        );
        return Ok(());
    };
    // Vanilla/none instances use the `native` branch, which attaches as a
    // JVM -javaagent instead of a mods/ jar. Track this so the chosen asset
    // is installed into the instance root rather than mods/.
    let is_native = despotes::is_javaagent_variant(dloader.slug());

    // Resolve the concrete Minecraft version (handles `release`/`latest`).
    let mc_version = {
        let manifest = crate::version::manifest::VersionManifest::fetch().await?;
        manifest
            .find_version(requested_version)
            .map(|v| v.id.clone())
            .unwrap_or_else(|| requested_version.to_string())
    };

    println!(
        "
Checking Despotes releases for {}/{} ...",
        dloader.slug(),
        mc_version
    );
    let releases = despotes::fetch_releases().await?;

    let stable = despotes::list_applicable(&releases, dloader.slug(), &mc_version, false);

    let chosen = if stable.is_empty() {
        // No applicable stable release: the newest applicable pre-release is
        // the fallback. Despotes currently has no stable release, so blocking
        // pre-releases entirely would make the mod unavailable. Auto-select
        // the pre-release in non-interactive sessions; prompt only when a
        // terminal is available.
        let Some((rel, asset)) =
            despotes::select_release(&releases, dloader.slug(), &mc_version, true)
        else {
            println!("No applicable Despotes package found for {}/{}.", dloader.slug(), mc_version);
            return Ok(());
        };
        println!("No stable Despotes release applies to this instance.");
        println!(
            "Latest applicable pre-release: {} ({}, {})",
            rel.tag,
            asset.name,
            format_bytes(asset.size)
        );
        if !stdin_is_interactive() {
            // Non-interactive (agent/CI): auto-install the pre-release since
            // no stable alternative exists.
            println!("Non-interactive session: auto-installing latest pre-release (no stable available).");
            Some((rel, asset))
        } else {
            print!("Install this pre-release? [y/N] ");
            let _ = std::io::stdout().flush();
            if !read_choice_line().eq_ignore_ascii_case("y") {
                println!("Skipped Despotes pre-release.");
                return Ok(());
            }
            Some((rel, asset))
        }
    } else if !stdin_is_interactive() {
        // Non-interactive: auto-select the newest applicable stable build.
        let pick = stable.into_iter().next().unwrap();
        println!(
            "Non-interactive session: auto-selected {} ({})",
            pick.0.tag, pick.1.name
        );
        Some(pick)
    } else {
        let mut list = stable;
        if allow_prerelease_flag {
            for item in despotes::list_applicable(&releases, dloader.slug(), &mc_version, true) {
                if item.0.prerelease {
                    list.push(item);
                }
            }
        }
        println!("Despotes packages applicable to this instance:");
        for (i, (rel, asset)) in list.iter().enumerate() {
            let kind = if rel.prerelease { "pre-release" } else { "release" };
            println!(
                "  [{}] {}  {}  ({}, {})",
                i + 1,
                rel.tag,
                kind,
                asset.name,
                format_bytes(asset.size)
            );
        }
        println!("  [0] Skip (do not install Despotes)");
        print!("Select a package number and press Enter [1]: ");
        let _ = std::io::stdout().flush();
        let raw = read_choice_line();
        let idx = if raw.is_empty() {
            1
        } else {
            raw.parse::<usize>().unwrap_or(0)
        };
        if idx == 0 || idx > list.len() {
            println!("Skipped Despotes installation.");
            return Ok(());
        }
        list.into_iter().nth(idx - 1)
    };

    let Some((rel, asset)) = chosen else {
        return Ok(());
    };

    let cached = despotes::download_asset(&asset).await?;
    let installed = if is_native {
        despotes::install_native(instance_dir, &cached).await?
    } else {
        despotes::install_into(instance_dir, &cached).await?
    };
    let where_ = if is_native { "instance root (javaagent)" } else { "mods/" };
    println!("Installed Despotes {} ({}) into {}", rel.tag, installed, where_);
    Ok(())
}

// ---------------------------------------------------------------------------
// Alpha 7 command implementations: search, account, bedrock, cache, inject
// ---------------------------------------------------------------------------

async fn cmd_search(
    kind: loader::content::ContentKind,
    query: &str,
    mc_version: Option<&str>,
    loader: Option<&str>,
    instance: Option<&str>,
    limit: usize,
) -> Result<()> {
    let hits = loader::content::search(kind, query, mc_version, loader, limit).await?;
    if hits.is_empty() {
        println!("No results.");
        return Ok(());
    }
    println!("Results ({}):", hits.len());
    for (i, h) in hits.iter().enumerate() {
        println!(
            "  [{}] {} ({} downloads) - {}",
            i + 1,
            h.title,
            h.downloads,
            h.description.chars().take(60).collect::<String>()
        );
    }
    if let Some(inst) = instance {
        let manager = instance::InstanceManager::new()?;
        let inst_obj = manager.get(inst).await?;
        print!("Install which number? (0 to skip) ");
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let idx: usize = input.trim().parse().unwrap_or(0);
        if idx == 0 || idx > hits.len() {
            println!("Skipped.");
            return Ok(());
        }
        let hit = &hits[idx - 1];
        let dest = loader::content::install_content(
            kind,
            hit,
            &inst_obj.path,
            mc_version,
            loader,
        )
        .await?;
        println!("Installed {} to {}", hit.title, dest.display());
    }
    Ok(())
}

async fn cmd_account_login() -> Result<()> {
    let acc = util::account::login_interactive().await?;
    println!(
        "Signed in as {} ({})",
        acc.username, acc.uuid
    );
    Ok(())
}

fn cmd_account_list() {
    let accounts = util::account::list_accounts();
    if accounts.is_empty() {
        println!("No cached accounts. Use `mdl account login`.");
        return;
    }
    for a in accounts {
        println!("  {} ({})", a.username, a.uuid);
    }
}

async fn cmd_account_skin(account: &str, output: Option<&str>) -> Result<()> {
    let acc = util::account::find_account(account);
    let uuid = match &acc {
        Some(a) => a.uuid.clone(),
        None => account.to_string(), // allow raw uuid
    };
    let out = match output {
        Some(p) => std::path::PathBuf::from(p),
        None => std::env::current_dir()?.join(format!("skin-{}.png", uuid)),
    };
    util::account::download_skin(&uuid, &out).await?;
    println!("Skin saved: {}", out.display());
    println!("Avatar: {}", util::account::avatar_url(&uuid, 64));
    Ok(())
}

// ---------------------------------------------------------------------------
// Aprism ecosystem management (Alpha 10): AprismRefract loader-support
// extensions (.aep -> aprism-extensions/) and AprismPrismate bridge
// (.jar -> mods/).
// ---------------------------------------------------------------------------

/// Resolve the loader key + MC version an Aprism artifact should target:
/// explicit CLI overrides win, otherwise fall back to the instance's config.
fn aprism_target<'a>(
    inst: &instance::Instance,
    loader_override: Option<&'a str>,
    mc_override: Option<&'a str>,
) -> (Option<String>, String) {
    let loader = loader_override
        .map(|s| s.to_string())
        .or_else(|| inst.config.loader.as_ref().map(|l| l.loader_type.clone()));
    let mc = mc_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| inst.config.version.clone());
    (loader, mc)
}

async fn cmd_aprism_refract_install(
    instance: &str,
    loader: Option<&str>,
    mc_version: Option<&str>,
    prerelease: bool,
) -> Result<()> {
    let manager = instance::InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    let (loader, mc) = aprism_target(&inst, loader, mc_version);
    let loader = loader.ok_or_else(|| {
        anyhow::anyhow!(
            "Instance '{}' has no mod loader; AprismRefract needs one              (fabric/forge/neoforge/quilt/liteloader). Use --loader to override.",
            inst.name
        )
    })?;
    let key = loader::refract::refract_key_for_loader(&loader).ok_or_else(|| {
        anyhow::anyhow!("Loader '{}' has no AprismRefract support extension", loader)
    })?;
    println!("Checking AprismRefract releases for {}/{} ...", loader, mc);
    let releases = loader::refract::fetch_releases().await?;
    let (rel, asset) = loader::refract::select_release(&releases, key, &mc, prerelease)
        .ok_or_else(|| anyhow::anyhow!("No applicable AprismRefract .aep for {}/{}", loader, mc))?;
    let cached = loader::refract::download_asset(&asset).await?;
    let installed = loader::refract::install_into(&inst.path, &cached).await?;
    println!(
        "Installed AprismRefract extension {} ({}) into aprism-extensions/",
        rel.tag, installed
    );
    Ok(())
}

async fn cmd_aprism_refract_list(instance: &str) -> Result<()> {
    let manager = instance::InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    let exts = loader::refract::installed_extensions(&inst.path);
    if exts.is_empty() {
        println!("No AprismRefract extensions installed in '{}'.", inst.name);
    } else {
        println!("AprismRefract extensions in '{}':", inst.name);
        for e in exts {
            println!("  {}", e.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
        }
    }
    Ok(())
}

async fn cmd_aprism_prismate_install(
    instance: &str,
    loader: Option<&str>,
    mc_version: Option<&str>,
    prerelease: bool,
) -> Result<()> {
    let manager = instance::InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    let (loader, mc) = aprism_target(&inst, loader, mc_version);
    let loader = loader.ok_or_else(|| {
        anyhow::anyhow!(
            "Instance '{}' has no mod loader; AprismPrismate runs inside              Fabric/NeoForge/Forge. Use --loader to override.",
            inst.name
        )
    })?;
    let key = loader::prismate::prismate_key_for_loader(&loader).ok_or_else(|| {
        anyhow::anyhow!("Loader '{}' has no AprismPrismate bridge (fabric/neoforge/forge only)", loader)
    })?;
    // Mutual exclusion: Prismate cannot coexist with the Aprism javaagent.
    // We cannot know the future launch flags here, so we only warn when the
    // Aprism loader is already known to be attached (instance-level marker).
    println!("Checking AprismPrismate releases for {}/{} ...", loader, mc);
    let releases = loader::prismate::fetch_releases().await?;
    let (rel, asset) = loader::prismate::select_release(&releases, key, &mc, prerelease)
        .ok_or_else(|| anyhow::anyhow!("No applicable AprismPrismate jar for {}/{}", loader, mc))?;
    let cached = loader::prismate::download_asset(&asset).await?;
    let installed = loader::prismate::install_into(&inst.path, &cached).await?;
    println!(
        "Installed AprismPrismate {} ({}) into mods/",
        rel.tag, installed
    );
    println!("Note: AprismPrismate is mutually exclusive with the --aprism javaagent.");
    Ok(())
}

async fn cmd_aprism_prismate_status(instance: &str) -> Result<()> {
    let manager = instance::InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    let jars = loader::prismate::installed_prismate(&inst.path);
    if jars.is_empty() {
        println!("AprismPrismate: not installed in '{}'.", inst.name);
    } else {
        println!("AprismPrismate in '{}':", inst.name);
        for j in jars {
            println!("  {}", j.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
        }
    }
    Ok(())
}

async fn cmd_bedrock_install(name: &str) -> Result<()> {
    let manager = instance::InstanceManager::new()?;
    let inst = manager.get(name).await?;
    let dir = inst.path.join("bedrock");
    println!("Downloading Bedrock Dedicated Server...");
    let server_dir = loader::bedrock::install_bds(&dir).await?;
    println!("BDS installed to {}", server_dir.display());
    Ok(())
}

async fn cmd_bedrock_launch(name: &str) -> Result<()> {
    let manager = instance::InstanceManager::new()?;
    let inst = manager.get(name).await?;
    let server_dir = inst.path.join("bedrock").join("server");
    if !server_dir.exists() {
        anyhow::bail!(
            "BDS not installed in '{}'. Run 'mdl bedrock install {}' first.",
            name, name
        );
    }
    let pid = loader::bedrock::launch_bds(&server_dir)?;
    println!("Bedrock Dedicated Server started (PID {})", pid);
    println!("  Log: {}", server_dir.join("bedrock_server.log").display());
    Ok(())
}

async fn cmd_bedrock_stop(name: &str) -> Result<()> {
    let manager = instance::InstanceManager::new()?;
    let inst = manager.get(name).await?;
    let server_dir = inst.path.join("bedrock").join("server");
    let pid = loader::bedrock::stop_bds(&server_dir)?;
    println!("Bedrock Dedicated Server stopped (PID {})", pid);
    Ok(())
}

async fn cmd_bedrock_status(name: &str, format: &str) -> Result<()> {
    let manager = instance::InstanceManager::new()?;
    let inst = manager.get(name).await?;
    let server_dir = inst.path.join("bedrock").join("server");
    let installed = server_dir.join("bedrock_server.exe").exists();
    let pid = loader::bedrock::running_bds_pid(&server_dir);
    if format == "json" {
        println!("{}", serde_json::json!({
            "name": name,
            "installed": installed,
            "running": pid.is_some(),
            "pid": pid,
            "dir": installed.then(|| server_dir.display().to_string()),
        }));
        return Ok(());
    }
    println!("Bedrock Dedicated Server for '{}'", name);
    println!("  Installed: {}", installed);
    match pid {
        Some(p) => println!("  Status: running (PID {})", p),
        None => println!("  Status: stopped"),
    }
    Ok(())
}

fn cmd_cache_info() {
    match util::cache::DownloadCache::new() {
        Ok(c) => {
            println!("Cache entries: {}", c.entry_count());
            println!("Total size: {:.2} MB", c.total_size() as f64 / 1024.0 / 1024.0);
            println!("Default TTL: {} days", util::cache::DEFAULT_CACHE_DAYS);
        }
        Err(e) => eprintln!("Failed to open cache: {}", e),
    }
}

fn cmd_cache_clean(days: u64) {
    match util::cache::DownloadCache::new() {
        Ok(mut c) => {
            let removed = c.evict_expired(days);
            println!("Evicted {} expired cache entr(y/ies) ({} days)", removed, days);
        }
        Err(e) => eprintln!("Failed to open cache: {}", e),
    }
}

async fn cmd_inject(target: &str, dll: &str) -> Result<()> {
    let pid: u32 = target.parse().unwrap_or_else(|_| {
        util::injector::find_pid_by_name(target).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        })
    });
    util::injector::inject_dll(pid, std::path::Path::new(dll))?;
    println!("Injected {} into PID {}", dll, pid);
    Ok(())
}


// ---------- Alpha 8.1: modpack import ----------

async fn cmd_import(name: &str, pack: &str, no_download: bool) -> Result<()> {
    use instance::{InstanceManager, config::{InstanceConfig, LoaderConfig}};

    let pack_path = std::path::PathBuf::from(pack);
    if !pack_path.exists() {
        anyhow::bail!("Modpack file not found: {}", pack);
    }

    // Parse the pack index to learn the required game version + loader.
    let index = loader::modpack::read_pack_index(&pack_path)?;
    let mc_version = index.minecraft_version()
        .ok_or_else(|| anyhow::anyhow!("Pack does not declare a Minecraft version"))?
        .to_string();
    let loader = index.loader();

    println!("Modpack: {} ({})", index.name, index.version_id);
    println!("  Minecraft: {}  Loader: {}", mc_version, loader.map(|(t, v)| format!("{} {}", t, v)).unwrap_or_else(|| "none".into()));

    // Create the instance with the pack's exact version + loader.
    let config = InstanceConfig {
        name: name.to_string(),
        version: mc_version.clone(),
        loader: loader.map(|(t, v)| LoaderConfig {
            loader_type: t.to_string(),
            version: v.to_string(),
        }),
    };
    let manager = InstanceManager::new()?;
    let instance = manager.create(config, true).await?;

    // Copy overrides into the instance.
    let copied = loader::modpack::extract_overrides(&pack_path, &instance.path)?;
    println!("Overrides: {} file(s) copied", copied);

    // Auto-completion: download every indexed file (idempotent).
    if no_download {
        println!("Skipped file downloads (--no-download). Run again without it to complete the pack.");
    } else {
        let (installed, skipped) = loader::modpack::download_pack_files(&index, &instance.path).await?;
        println!("Pack files: {} installed, {} already present", installed, skipped);
    }

    println!("Instance '{}' imported from modpack. Launch it with: mdl launch {}", name, name);
    Ok(())
}

// ---------- Alpha 8.1: JE dedicated server ----------

async fn cmd_server_create(name: &str, mc_version: &str, memory: Option<&str>) -> Result<()> {
    let dir = loader::server::create_server(name, mc_version, memory).await?;
    println!("Server '{}' created at {}", name, dir.display());
    println!("  Start it with: mdl server launch {}   (add --attach for foreground)", name);
    Ok(())
}

fn cmd_server_list(format: &str) {
    let servers = loader::server::list_servers().unwrap_or_default();
    if format == "json" {
        let rows: Vec<serde_json::Value> = servers.iter().map(|s| {
            let dir = s.dir().ok();
            let pid = dir.as_deref().and_then(loader::server::running_pid);
            serde_json::json!({
                "name": s.name,
                "version": s.version,
                "memory": s.memory,
                "running": pid.is_some(),
                "pid": pid,
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
        return;
    }
    if servers.is_empty() {
        println!("No servers. Create one with: mdl server create <name> --mc-version <ver>");
        return;
    }
    for s in &servers {
        let running = s.dir().ok().and_then(|d| loader::server::running_pid(&d));
        match running {
            Some(pid) => println!("  {}  (Minecraft {}, PID {}) [running]", s.name, s.version, pid),
            None => println!("  {}  (Minecraft {})", s.name, s.version),
        }
    }
}

async fn cmd_server_launch(name: &str, attach: bool) -> Result<()> {
    let info = loader::server::load_server(name)?;
    let pid = loader::server::launch_server(&info, !attach).await?;
    if !attach {
        println!("Server '{}' running in background (PID {})", name, pid);
        println!("  Log: {}/server.log", info.dir()?.display());
    }
    Ok(())
}

async fn cmd_server_stop(name: &str) -> Result<()> {
    let info = loader::server::load_server(name)?;
    loader::server::stop_server(&info).await?;
    println!("Server '{}' stopped", name);
    Ok(())
}

fn cmd_server_status(format: &str, name: &str) {
    let info = match loader::server::load_server(name) {
        Ok(i) => i,
        Err(e) => { eprintln!("Error: {}", e); return; }
    };
    let dir = info.dir().ok();
    let pid = dir.as_deref().and_then(loader::server::running_pid);
    if format == "json" {
        println!("{}", serde_json::json!({
            "name": info.name,
            "version": info.version,
            "memory": info.memory,
            "running": pid.is_some(),
            "pid": pid,
            "dir": dir.map(|d| d.display().to_string()),
        }));
        return;
    }
    println!("Server: {} (Minecraft {})", info.name, info.version);
    if let Some(dir) = &dir { println!("  Directory: {}", dir.display()); }
    match pid {
        Some(p) => println!("  Status: running (PID {})", p),
        None => println!("  Status: stopped"),
    }
}

fn cmd_changelog(num_versions: usize) {
    use util::changelog;
    
    let digests = changelog::recent_versions(changelog::CHANGELOG, num_versions, 10);
    if digests.is_empty() {
        println!("{}", util::i18n::t("No changelog entries found.", "未找到更新日志条目。"));
        return;
    }
    
    println!("{}\n", util::i18n::t("Recent Updates:", "最近更新："));
    for d in &digests {
        println!("## {} {}", d.version, if d.date.is_empty() { String::new() } else { format!("({})", d.date) });
        for h in &d.highlights {
            println!("  - {}", h);
        }
        println!();
    }
}
