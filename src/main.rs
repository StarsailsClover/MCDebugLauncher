// MDL keeps a large intentional public API surface: serde wire-format fields
// deserialized from Mojang/GitHub/Modrinth responses but not read in Rust, and
// reserved utility functions staged for upcoming features. These are deliberate,
// not bugs — suppress dead_code for the whole crate rather than annotating each.
#![allow(dead_code)]

// MCDebugLauncher - Main entry point

use anyhow::Result;
use anyhow::Context as _;
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
use util::disk::{dir_size, format_bytes};

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

    /// Log output format: text (default) or json (structured, one JSON
    /// object per line - for log aggregation pipelines)
    #[arg(long, global = true, default_value = "text")]
    log_format: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum ModCommands {
    /// List mods in an instance (or all instances with --all-instances)
    List {
        /// Instance name (ignored when --all-instances is set)
        instance: String,
        /// List mods across all instances
        #[arg(long)]
        all_instances: bool,
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

    /// Query redstone signal at a block position (Despotes v26.9). Omit
    /// coordinates to probe the crosshair target block instead.
    Redstone {
        /// Instance name
        instance: String,

        /// Block X (requires y and z)
        #[arg(long)]
        x: Option<i32>,

        /// Block Y
        #[arg(long)]
        y: Option<i32>,

        /// Block Z
        #[arg(long)]
        z: Option<i32>,
    },

    /// Periodic command execution on the client thread (v26.9). Ops:
    /// add / status / remove. `add` needs --name, --period-ticks and at
    /// least one --command (each a JSON action, e.g.
    /// '{"type":"chat","text":"hi"}').
    Schedule {
        /// Instance name
        instance: String,

        /// Operation: add, status, remove
        op: String,

        /// Schedule name (add/remove)
        #[arg(long)]
        name: Option<String>,

        /// Repetition period in game ticks (20 = 1s)
        #[arg(long)]
        period_ticks: Option<u64>,

        /// JSON action to run each period; repeatable for sequences
        #[arg(long = "command")]
        commands: Vec<String>,
    },

    /// Record & replay action sequences (v26.9). Ops: start-recording,
    /// record-step (--step JSON), stop-recording, play, stop, delete,
    /// status — most take --name.
    Macro {
        /// Instance name
        instance: String,

        /// Macro operation
        op: String,

        /// Macro name
        #[arg(long)]
        name: Option<String>,

        /// One recorded step as a JSON action (record-step)
        #[arg(long)]
        step: Option<String>,
    },

    /// Conditional branch execution (v26.9): run --if query, extract a field
    /// via dot-path, compare, then execute the matching branch inline.
    /// Example:
    ///   mdl game condition inst --if '{"type":"status","field":"result.inGame","op":"exists"}' \
    ///     --then '[{"type":"ping"}]' --else '[{"type":"chat","text":"not in game"}]'
    Condition {
        /// Instance name
        instance: String,

        /// Condition query as JSON: {"type":..., "field":"dot.path", "op":...}
        #[arg(long = "if")]
        if_: String,

        /// Then-branch actions as a JSON array (default [{"type":"ping"}])
        #[arg(long = "then")]
        then_: Option<String>,

        /// Else-branch actions as a JSON array
        #[arg(long = "else")]
        else_: Option<String>,
    },

    /// Send a raw Despotes protocol action as JSON (escape hatch for
    /// primitives without dedicated MDL subcommands)
    RawAction {
        /// Instance name
        instance: String,

        /// Full action payload, e.g. '{"type":"ping"}'
        json: String,
    },

    /// Hot-attach a Java agent JAR into the RUNNING game JVM (v26.2-alpha.6).
    /// Uses the JVM Attach API (agentmain); the agent must implement
    /// agentmain in its manifest. Unlike launch-time --javaagent this works
    /// while the game is already up, without restarting it.
    InjectAgent {
        /// Instance name
        instance: String,

        /// Path to the agent JAR (or an entry name registered via `mdl javaagent install`)
        jar: String,

        /// Agent options string (text after `=` in -javaagent syntax)
        #[arg(short, long)]
        params: Option<String>,

        /// Custom java executable (default: auto-detect / instance runtime)
        #[arg(long)]
        java_path: Option<String>,
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
    /// Refresh access token(s) using stored refresh tokens (v26.2-alpha.6)
    Refresh {
        /// UUID or username; omit with --all to refresh every account
        account: Option<String>,
        /// Refresh all cached accounts
        #[arg(long)]
        all: bool,
    },
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
enum JavaAgentCommands {
    /// Install a JavaAgent JAR into an instance
    Install {
        /// Instance name
        instance: String,
        /// Path to the agent JAR file
        jar: String,
        /// Optional parameters (e.g. `port=25585`)
        #[arg(long)]
        params: Option<String>,
    },

    /// List registered JavaAgents in an instance
    List {
        /// Instance name
        instance: String,
    },

    /// Remove a registered JavaAgent from an instance
    Remove {
        /// Instance name
        instance: String,
        /// Agent name
        name: String,
    },

    /// Enable a disabled JavaAgent
    Enable {
        /// Instance name
        instance: String,
        /// Agent name
        name: String,
    },

    /// Disable a JavaAgent (keeps the file, skips at launch)
    Disable {
        /// Instance name
        instance: String,
        /// Agent name
        name: String,
    },
}

#[derive(Subcommand)]
enum AprismCommands {
    /// Unified ecosystem status for an instance (v26.2-alpha.8): cached
    /// agent JARs, installed Refract .aep extensions, Prismate bridge,
    /// native .aje mods, and compatibility notes.
    Status {
        /// Instance name
        instance: String,
    },
    /// Manage AprismRefract loader-support extensions (.aep)
    #[command(subcommand)]
    Refract(AprismRefractCommands),
    /// Manage AprismPrismate loader-side bridge (.jar)
    #[command(subcommand)]
    Prismate(AprismPrismateCommands),
}

#[derive(Subcommand)]
enum JdkCommands {
    /// Show remote AprismJDK releases and which applies to this host
    Available {
        /// Also consider pre-release tags
        #[arg(long)]
        prerelease: bool,
        /// Output format: text, json, yaml
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Download, verify (SHA-256) and install a runtime into the MDL cache
    Install {
        /// Tag or version hint (e.g. v26.2 or 26.2); defaults to newest stable
        #[arg(long)]
        version: Option<String>,
        /// Also consider pre-release tags when no stable matches
        #[arg(long)]
        prerelease: bool,
    },
    /// List AprismJDK runtimes installed in the MDL cache
    List {
        /// Output format: text, json, yaml
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Remove an installed runtime by tag
    Remove {
        /// Release tag, e.g. v26.2
        tag: String,
    },
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
    /// Remove installed .aep extensions (v26.2-alpha.9)
    Remove {
        /// Instance name
        instance: String,
        /// Filename substring filter; omit with --all to remove everything
        name: Option<String>,
        /// Remove all .aep extensions
        #[arg(long)]
        all: bool,
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
    /// Remove the installed Prismate bridge (v26.2-alpha.9)
    Remove {
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
        /// Block until the server logs its ready line "Done (...s)!" (v26.2-alpha.7)
        #[arg(long)]
        wait_ready: bool,
        /// Timeout in seconds for --wait-ready (default 180)
        #[arg(long, default_value = "180")]
        ready_timeout: u64,
    },
    /// Stop a running server (graceful via RCON when configured)
    Stop {
        /// Server name
        name: String,
    },
    /// Run a console command on a server via RCON (v26.2-alpha.7).
    /// Enables agent-driven test automation: op, gamerule, whitelist, ...
    Cmd {
        /// Server name
        name: String,
        /// Console command text (e.g. "gamerule doDaylightCycle false")
        #[arg(allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Edit server.properties with comment/order preservation (v26.3-alpha.4)
    Props {
        /// Server name
        name: String,
        #[command(subcommand)]
        action: PropsCommands,
    },
    /// Manage the allowlist (whitelist). add/remove/list need a running
    /// server (RCON); enable/disable edit server.properties and work when
    /// stopped (restart required to apply).
    Allowlist {
        /// Server name
        name: String,
        #[command(subcommand)]
        action: AllowlistCommands,
    },
    /// Grant operator status (RCON, running) or list ops (ops.json)
    Op {
        /// Server name
        name: String,
        #[command(subcommand)]
        action: OpCommands,
    },
    /// Ban/pardon players (RCON) or list bans (banned-players.json)
    Ban {
        /// Server name
        name: String,
        #[command(subcommand)]
        action: BanCommands,
    },
    /// Rotate the RCON password of a managed server (v26.3-alpha.5).
    /// Updates server.properties + server.json. A running server keeps the
    /// OLD password until restarted.
    RotateRcon {
        /// Server name
        name: String,
        /// Print the new password to stdout (default: masked)
        #[arg(long)]
        show: bool,
    },
    /// Show server status (running PID, version)
    Status {
        /// Server name
        name: String,
    },
}

#[derive(Subcommand)]
enum PropsCommands {
    /// List all key=value pairs
    List,
    /// Read one key
    Get {
        /// Property key
        key: String,
    },
    /// Write one key (creates or replaces; comments preserved)
    Set {
        /// Property key
        key: String,
        /// Property value
        #[arg(allow_hyphen_values = true)]
        value: String,
    },
}

#[derive(Subcommand)]
enum AllowlistCommands {
    /// Add a player to the allowlist (RCON)
    Add { player: String },
    /// Remove a player from the allowlist (RCON)
    Remove { player: String },
    /// List allowlisted players (RCON when running, else whitelist.json)
    List,
    /// Enable the allowlist in server.properties (white-list + enforce-whitelist)
    Enable,
    /// Disable the allowlist in server.properties
    Disable,
}

#[derive(Subcommand)]
enum OpCommands {
    /// Grant operator status to a player (RCON)
    Add { player: String },
    /// Revoke operator status from a player (RCON)
    Remove { player: String },
    /// List operators (from ops.json; works stopped or running)
    List,
}

#[derive(Subcommand)]
enum BanCommands {
    /// Ban a player (RCON)
    Add { player: String },
    /// Pardon a banned player (RCON)
    Remove { player: String },
    /// List banned players (from banned-players.json)
    List,
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

        /// Minecraft version (alias: --mc, as used in sibling project docs)
        #[arg(long, alias = "mc", default_value = "release")]
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

        /// Java runtime selection: "aprism" (latest installed AprismJDK) or
        /// "aprism@<tag|version>" (e.g. aprism@26.2). Conflicts with
        /// --java-path. v26.4-alpha.6.
        #[arg(long)]
        jdk: Option<String>,

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

        /// OOM second-confirmation policy before killing stale processes:
        /// auto (prompt only in interactive terminals, default), always,
        /// never. v26.3-alpha.2.
        #[arg(long, default_value = "auto")]
        oom_confirm: String,

        /// List OOM sweep candidates (PID / window title / memory) and skip
        /// termination entirely. v26.3-alpha.2.
        #[arg(long)]
        oom_list_only: bool,

        /// Attach an ad-hoc JavaAgent JAR at launch. Use `jar` or `jar=params`.
        /// Can be repeated to attach multiple agents.
        #[arg(long = "javaagent")]
        javaagents: Vec<String>,
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

    /// Show launch metrics for an instance (v26.2-alpha.9): spawn time,
    /// time-to-ready, download bytes and cache hit rate. Local-only data.
    Metrics {
        /// Instance name
        instance: String,

        /// Print the full recorded history instead of just the last launch
        #[arg(long)]
        history: bool,
    },

    /// Benchmark MDL CLI cold latency (v26.3-alpha.6): spawns this binary
    /// N times per tracked command and reports min/p50/p95/max in ms.
    Bench {
        /// Iterations per command
        #[arg(long, default_value = "20")]
        iterations: u32,
    },

    /// Get instance status
    Status {
        /// Instance name (optional, shows all if omitted)
        name: Option<String>,

        /// Show detailed instance information (config, mods, etc.)
        #[arg(short, long)]
        detail: bool,

        /// Report per-instance disk usage (v26.2-alpha.6)
        #[arg(long)]
        disk: bool,
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

    /// Manage JavaAgents attached to an instance at launch
    #[command(subcommand)]
    Javaagent(JavaAgentCommands),

    /// Download cache management
    #[command(subcommand)]
    Cache(CacheCommands),

    /// Aprism ecosystem: loader-support extensions (AprismRefract), the
    /// loader-side bridge (AprismPrismate) and the AprismJDK runtime
    #[command(subcommand)]
    Aprism(AprismCommands),

    /// Manage the AprismJDK (AJR - Aprism Java Runtime): install, list,
    /// remove, and use at launch via `--jdk aprism[@<version>]`
    #[command(subcommand)]
    Jdk(JdkCommands),

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

    /// Export an instance to a zip file or .mrpack modpack
    Export {
        /// Instance name
        instance: String,
        /// Output path for the archive (.zip or .mrpack)
        path: PathBuf,
        /// Export format: zip (default) or mrpack (Modrinth modpack)
        #[arg(long, default_value = "zip")]
        format: String,
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

fn main() -> Result<()> {
    // clap's derive help rendering recurses deeply and overflows the ~1MB
    // default main-thread stack in debug builds (`mdl --help` crashed with
    // "thread 'main' has overflowed its stack"). Run the whole program on a
    // dedicated thread with a generous stack; the tokio runtime is built
    // inside run() so all async work happens there too.
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(run)
        .expect("failed to spawn main thread")
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e))
}

#[tokio::main]
async fn run() -> Result<()> {
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

    // v26.2-alpha.9: --log-format json emits one structured JSON object per
    // log line (tracing-subscriber json format) for aggregation pipelines.
    // Both formats share the same tee writer (stderr + optional file).
    if cli.log_format == "json" {
        let subscriber = FmtSubscriber::builder()
            .json()
            .with_max_level(log_level)
            .with_writer(TeeMakeWriter::new(file_writer, machine_readable))
            .finish();
        tracing::subscriber::set_global_default(subscriber)?;
    } else {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(log_level)
            .with_ansi(!cli.no_color)
            .with_writer(TeeMakeWriter::new(file_writer, machine_readable))
            .finish();
        tracing::subscriber::set_global_default(subscriber)?;
    }

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
        Commands::Launch { name, username, server, fullscreen, width, height, detach, no_queue, agent, agent_port, java_path, jdk, memory, dynamic_memory, aprism, enter_test_world, wait_ready, idle_timeout, no_idle_timeout, oom_protect, oom_aggressive, oom_confirm, oom_list_only, javaagents } => {
            if java_path.is_some() && jdk.is_some() {
                anyhow::bail!("--jdk and --java-path are mutually exclusive");
            }
            // Resolve the AprismJDK selection to a concrete java executable
            // up front so errors surface before the launch pipeline starts.
            let resolved_java: Option<String> = match jdk.as_deref() {
                None => java_path.map(|p| p.to_string()),
                Some(spec) => {
                    // Contract: "aprism" | "aprism@<tag|version>". Tolerate a
                    // bare tag/version too by passing it straight through.
                    let hint = spec
                        .strip_prefix("aprism")
                        .map(|rest| rest.trim_start_matches('@'))
                        .unwrap_or(spec);
                    match loader::aprism_jdk::resolve(Some(hint)) {
                        Ok((tag, java)) => {
                            println!("Using AprismJDK runtime {tag}: {}", java.display());
                            Some(java.display().to_string())
                        }
                        Err(e) => {
                            // v26.4-alpha.7: graceful degradation. An absent or
                            // mismatched AprismJDK must not block launching -
                            // fall back to the standard detection chain, which
                            // provisions Eclipse Adoptium (Temurin) when no
                            // local runtime satisfies the MC version.
                            println!(
                                "AprismJDK unavailable ({e:#}); \
                                 falling back to system Java / Eclipse Adoptium provisioning"
                            );
                            None
                        }
                    }
                }
            };
            cmd_launch(&name, username.as_deref(), server.as_deref(), fullscreen, width, height, detach, no_queue, agent, agent_port, resolved_java.as_deref(), memory.as_deref(), dynamic_memory, aprism, enter_test_world, wait_ready, idle_timeout, no_idle_timeout, oom_protect, oom_aggressive, &oom_confirm, oom_list_only, &javaagents).await?;
        }
        Commands::Diagnose { name, export, analyze } => {
            cmd_diagnose(&name, export.as_deref(), analyze).await?;
        }
        Commands::Logs { name, follow, lines, level } => {
            cmd_logs(&name, follow, lines, level.as_deref()).await?;
        }
        Commands::Status { name, detail, disk } => {
            cmd_status(&cli.format, name.as_deref(), detail).await?;
            if disk {
                cmd_status_disk(&cli.format, name.as_deref()).await?;
            }
        }
        Commands::Metrics { instance, history } => {
            cmd_metrics(&cli.format, &instance, history).await?;
        }
        Commands::Bench { iterations } => {
            cmd_bench(&cli.format, iterations)?;
        }
        Commands::Mod(mod_cmd) => {
            match mod_cmd {
                ModCommands::List { instance, all_instances } => {
                    if all_instances {
                        cmd_mod_list_all(&cli.format).await?;
                    } else {
                        cmd_mod_list(&cli.format, &instance).await?;
                    }
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
                GameCommands::Redstone { instance, x, y, z } => {
                    cmd_game_redstone(&instance, x, y, z).await?;
                }
                GameCommands::Schedule { instance, op, name, period_ticks, commands } => {
                    cmd_game_schedule(&instance, &op, name.as_deref(), period_ticks, &commands).await?;
                }
                GameCommands::Macro { instance, op, name, step } => {
                    cmd_game_macro(&instance, &op, name.as_deref(), step.as_deref()).await?;
                }
                GameCommands::Condition { instance, if_, then_, else_ } => {
                    cmd_game_condition(&instance, &if_, then_.as_deref(), else_.as_deref()).await?;
                }
                GameCommands::RawAction { instance, json } => {
                    cmd_game_raw_action(&instance, &json).await?;
                }
                GameCommands::InjectAgent { instance, jar, params, java_path } => {
                    cmd_game_inject_agent(&instance, &jar, params.as_deref(), java_path.as_deref()).await?;
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
        Commands::Export { instance, path, format } => {
            cmd_export(&instance, &path, &format).await?;
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
            AccountCommands::Refresh { account, all } => { cmd_account_refresh(account.as_deref(), all).await?; }
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
        Commands::Javaagent(ja) => match ja {
            JavaAgentCommands::Install { instance, jar, params } => {
                cmd_javaagent_install(&instance, &jar, params.as_deref()).await?;
            }
            JavaAgentCommands::List { instance } => {
                cmd_javaagent_list(&instance).await?;
            }
            JavaAgentCommands::Remove { instance, name } => {
                cmd_javaagent_remove(&instance, &name).await?;
            }
            JavaAgentCommands::Enable { instance, name } => {
                cmd_javaagent_enable(&instance, &name).await?;
            }
            JavaAgentCommands::Disable { instance, name } => {
                cmd_javaagent_disable(&instance, &name).await?;
            }
        },
        Commands::Aprism(ac) => match ac {
            AprismCommands::Status { instance } => {
                cmd_aprism_status(&cli.format, &instance).await?;
            }
            AprismCommands::Refract(rc) => match rc {
                AprismRefractCommands::Install { instance, loader, mc_version, prerelease } => {
                    cmd_aprism_refract_install(&instance, loader.as_deref(), mc_version.as_deref(), prerelease).await?;
                }
                AprismRefractCommands::List { instance } => { cmd_aprism_refract_list(&instance).await?; }
                AprismRefractCommands::Remove { instance, name, all } => {
                    cmd_aprism_refract_remove(&instance, name.as_deref(), all).await?;
                }
            },
            AprismCommands::Prismate(pc) => match pc {
                AprismPrismateCommands::Install { instance, loader, mc_version, prerelease } => {
                    cmd_aprism_prismate_install(&instance, loader.as_deref(), mc_version.as_deref(), prerelease).await?;
                }
                AprismPrismateCommands::Status { instance } => { cmd_aprism_prismate_status(&instance).await?; }
                AprismPrismateCommands::Remove { instance } => { cmd_aprism_prismate_remove(&instance).await?; }
            },
        },
        Commands::Jdk(jc) => match jc {
            JdkCommands::Available { prerelease, format } => {
                cmd_jdk_available(prerelease, &format).await?;
            }
            JdkCommands::Install { version, prerelease } => {
                cmd_jdk_install(version.as_deref(), prerelease).await?;
            }
            JdkCommands::List { format } => { cmd_jdk_list(&format)?; }
            JdkCommands::Remove { tag } => { cmd_jdk_remove(&tag)?; }
        },
        Commands::Import { name, pack, no_download } => {
            cmd_import(&name, &pack, no_download).await?;
        }
        Commands::Server(sc) => match sc {
            ServerCommands::Create { name, mc_version, memory } => {
                cmd_server_create(&name, &mc_version, memory.as_deref()).await?;
            }
            ServerCommands::List => { cmd_server_list(&cli.format); }
            ServerCommands::Launch { name, attach, wait_ready, ready_timeout } => {
                cmd_server_launch(&name, attach, wait_ready, ready_timeout).await?;
            }
            ServerCommands::Stop { name } => { cmd_server_stop(&name).await?; }
            ServerCommands::Cmd { name, command } => {
                cmd_server_cmd(&name, &command.join(" ")).await?;
            }
            ServerCommands::Props { name, action } => match action {
                PropsCommands::List => cmd_server_props_list(&name)?,
                PropsCommands::Get { key } => cmd_server_props_get(&name, &key)?,
                PropsCommands::Set { key, value } => cmd_server_props_set(&name, &key, &value)?,
            },
            ServerCommands::Allowlist { name, action } => match action {
                AllowlistCommands::Add { player } => cmd_server_rcon(&name, &format!("whitelist add {player}")).await?,
                AllowlistCommands::Remove { player } => cmd_server_rcon(&name, &format!("whitelist remove {player}")).await?,
                AllowlistCommands::List => cmd_server_allowlist_list(&name).await?,
                AllowlistCommands::Enable => cmd_server_toggle_whitelist(&name, true)?,
                AllowlistCommands::Disable => cmd_server_toggle_whitelist(&name, false)?,
            },
            ServerCommands::Op { name, action } => match action {
                OpCommands::Add { player } => cmd_server_rcon(&name, &format!("op {player}")).await?,
                OpCommands::Remove { player } => cmd_server_rcon(&name, &format!("deop {player}")).await?,
                OpCommands::List => cmd_server_json_names(&name, "ops.json", "Operators")?,
            },
            ServerCommands::Ban { name, action } => match action {
                BanCommands::Add { player } => cmd_server_rcon(&name, &format!("ban {player}")).await?,
                BanCommands::Remove { player } => cmd_server_rcon(&name, &format!("pardon {player}")).await?,
                BanCommands::List => cmd_server_json_names(&name, "banned-players.json", "Banned players")?,
            },
            ServerCommands::RotateRcon { name, show } => { cmd_server_rotate_rcon(&name, show)?; }
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
        javaagents: Vec::new(),
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
    oom_confirm: &str,
    oom_list_only: bool,
    javaagents: &[String],
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
        oom_confirm: Some(oom_confirm.to_string()),
        oom_list_only,
        javaagents: javaagents.to_vec(),
    };

    let launcher = InstanceLauncher::new()?;
    let launch_start = std::time::Instant::now();
    let outcome = launcher.launch(name, &options).await?;
    let spawn_secs = launch_start.elapsed().as_secs_f64();

    // Record per-launch metrics (local-only; see util::metrics).
    if let Ok(manager) = instance::InstanceManager::new() {
        if let Ok(inst) = manager.get(name).await {
            let (bytes, downloads, hits) = util::metrics::snapshot_counters();
            let m = util::metrics::LaunchMetrics {
                timestamp: chrono::Utc::now().to_rfc3339(),
                instance: name.to_string(),
                pid: outcome.pid,
                detached: outcome.detached,
                spawn_secs,
                ready_secs: None, // filled below when --wait-ready succeeds
                download_bytes: bytes,
                downloads,
                cache_hits: hits,
            };
            let _ = util::metrics::save_launch(&inst.path, &m);
        }
    }

    if outcome.detached {
        println!("Instance '{}' launched in background (PID {})", name, outcome.pid);
        println!("  Game log: <instance>/logs/launch_detached.log");
        if agent {
            println!("  Agent control: use 'mdl game status {}' once in-game", name);
        }
        // Wait for the game-ready broadcast if requested (agent mode).
        if agent && wait_ready {
            let ready_start = std::time::Instant::now();
            let ready = wait_game_ready(&outcome, name).await;
            if ready {
                let ready_secs = ready_start.elapsed().as_secs_f64();
                println!("  Game is ready (t+{:.1}s).", ready_secs);
                // Patch the just-written metrics with the ready timing.
                if let Ok(manager) = instance::InstanceManager::new() {
                    if let Ok(inst) = manager.get(name).await {
                        if let Some(mut m) = util::metrics::load_latest(&inst.path) {
                            m.ready_secs = Some(ready_secs);
                            let _ = util::metrics::save_launch(&inst.path, &m);
                        }
                        // v26.3-alpha.8: arm a watchdog deferred by
                        // --wait-ready (pending file written at launch).
                        let pending =
                            inst.path.join("runtime").join("watchdog_pending.json");
                        if pending.exists() && !no_idle_timeout {
                            if let Ok(cfg) =
                                tokio::fs::read_to_string(&pending).await
                            {
                                if let Ok(v) =
                                    serde_json::from_str::<serde_json::Value>(&cfg)
                                {
                                    let pid = v["pid"].as_u64().unwrap_or(0) as u32;
                                    let timeout =
                                        v["timeout_secs"].as_u64().unwrap_or(60);
                                    let log = v["log"].as_str().unwrap_or("");
                                    let wd = game::watchdog::IdleWatchdog::new(
                                        pid,
                                        name.to_string(),
                                        std::path::Path::new(log),
                                        timeout,
                                    );
                                    wd.start();
                                    tracing::info!(
                                        "Idle watchdog armed after ready: {}s",
                                        timeout
                                    );
                                }
                            }
                            let _ = tokio::fs::remove_file(&pending).await;
                        }
                    }
                }
                if enter_test_world {
                    enter_world_after_ready(name).await?;
                }
            } else {
                // Launch never became ready: drop any deferred watchdog so a
                // hung startup is not silently killed mid-diagnosis.
                if let Ok(manager) = instance::InstanceManager::new() {
                    if let Ok(inst) = manager.get(name).await {
                        let _ = tokio::fs::remove_file(
                            inst.path.join("runtime").join("watchdog_pending.json"),
                        )
                        .await;
                    }
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
///
/// v26.2-alpha.7 completes the previously truncated flow: the old version
/// clicked Singleplayer -> Create New World and stopped there, never pressing
/// the final "Create" button, so no world was ever generated. The new flow is
/// screen-adaptive: it polls the Despotes status between steps instead of
/// blind sleeps, and handles both paths:
///   - existing test world (marker from --with-test-world or a prior run):
///       Title -> Singleplayer -> select first slot -> Play Selected World
///   - fresh instance:
///       Title -> Singleplayer -> Create New World -> Create (confirm)
///
/// Coordinates assume GUI scale 2 at the default window size (verified in the
/// v26.0 Alpha 7 E2E). Best-effort by contract: failures only warn.
async fn enter_world_after_ready(name: &str) -> Result<()> {
    use instance::InstanceManager;
    let manager = InstanceManager::new()?;
    let inst = manager.get(name).await?;

    // A world already exists when we seeded one at create time or a previous
    // run completed creation (saves/<world>/level.dat present).
    let has_world = inst.path.join("mdl-test-world").exists()
        || saves_contain_level(&inst.path);

    let click = |x: f64, y: f64| {
        let path = inst.path.clone();
        async move {
            let _ = game::client::mouse_input(&path, "left", "tap", Some(x), Some(y), None).await;
        }
    };
    let sleep = |ms: u64| async move {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
    };

    // Step 1: title screen -> Singleplayer.
    click(213.0, 136.0).await;
    sleep(1500).await;

    if has_world {
        // Select World screen: first save slot sits near the top of the list,
        // then confirm with "Play Selected World" at the lower center.
        click(213.0, 80.0).await;  // first world entry
        sleep(600).await;
        click(213.0, 233.0).await; // Play Selected World
        tracing::info!("enter_test_world: selected existing test world");
    } else {
        // Select World screen -> Create New World button.
        click(131.0, 226.0).await;
        sleep(1200).await;
        // Create World screen: confirm with the final Create button
        // (bottom center). Defaults are fine for testing: name "New World",
        // survival, seed random.
        click(213.0, 233.0).await;
        tracing::info!("enter_test_world: requested world creation; game will generate it");
        // Mark so subsequent runs take the faster "play existing" path.
        let _ = std::fs::write(inst.path.join("mdl-test-world"), "true");
    }

    // Wait up to 90s for in-game state as the final confirmation signal.
    for _ in 0..45 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if let Ok(st) = game::client::game_status(&inst.path).await {
            if st.get("inGame").and_then(|v| v.as_bool()) == Some(true) {
                tracing::info!("enter_test_world: player is in the world");
                return Ok(());
            }
        }
    }
    tracing::warn!("enter_test_world: did not observe inGame=true within 90s (continuing)");
    Ok(())
}

/// Whether any save directory under <instance>/saves contains a level.dat.
fn saves_contain_level(instance_dir: &std::path::Path) -> bool {
    let saves = instance_dir.join("saves");
    let Ok(entries) = std::fs::read_dir(&saves) else { return false };
    for e in entries.flatten() {
        if e.path().is_dir() && e.path().join("level.dat").exists() {
            return true;
        }
    }
    false
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

    // v26.3-alpha.3: runtime events + launch correlation.
    if let Some(ev) = &report.idle_timeout_event {
        println!("Runtime Events:");
        println!(
            "  Idle watchdog terminated this game at {} (PID {}, silent for {}s)",
            ev.timestamp, ev.pid, ev.idle_seconds
        );
        println!();
    }

    if let Some(m) = &report.last_launch_metrics {
        println!("Last Launch ({})", m.timestamp);
        println!("  spawn: {:.1}s | ready: {}",
            m.spawn_secs,
            match m.ready_secs { Some(r) => format!("{:.1}s", r), None => "n/a".into() });
        println!("  downloads: {} file(s), {} | cache hits: {}",
            m.downloads,
            format_bytes(m.download_bytes),
            m.cache_hits);
        println!();

        if !report.crash_reports.is_empty() {
            println!("Correlation Notes:");
            for note in diagnostic::collector::build_correlation_notes(
                true, report.idle_timeout_event.as_ref(), Some(m),
            ) {
                println!("  - {}", note);
            }
            println!();
        }
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

/// Per-instance disk usage report (v26.2-alpha.6). Walks each instance
/// directory summing file sizes. With `name`, reports one instance plus a
/// breakdown by top-level subdirectory; otherwise lists all instances
/// sorted largest-first.
async fn cmd_status_disk(format: &str, name: Option<&str>) -> Result<()> {
    use instance::InstanceManager;

    let manager = InstanceManager::new()?;
    let instances = if let Some(n) = name {
        vec![manager.get(n).await?]
    } else {
        manager.list().await?
    };

    // (name, total_bytes, breakdown) — breakdown only for single-instance.
    let mut rows: Vec<(String, u64, Option<Vec<(String, u64)>>)> = Vec::new();
    for inst in &instances {
        let total = dir_size(&inst.path).await;
        let breakdown = if name.is_some() {
            let mut subs: Vec<(String, u64)> = Vec::new();
            if let Ok(mut entries) = tokio::fs::read_dir(&inst.path).await {
                while let Some(entry) = entries.next_entry().await.ok().flatten() {
                    let p = entry.path();
                    let size = if p.is_dir() { dir_size(&p).await } else { 0 };
                    subs.push((
                        p.file_name().map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        size,
                    ));
                }
            }
            subs.sort_by(|a, b| b.1.cmp(&a.1));
            Some(subs)
        } else {
            None
        };
        rows.push((inst.name.clone(), total, breakdown));
    }

    if !name.is_some() {
        rows.sort_by(|a, b| b.1.cmp(&a.1));
    }

    if format == "json" {
        let data: Vec<serde_json::Value> = rows.iter().map(|(n, t, b)| {
            let mut v = serde_json::json!({ "instance": n, "bytes": t, "human": format_bytes(*t) });
            if let Some(subs) = b {
                v["breakdown"] = serde_json::json!(subs.iter().map(|(sn, sb)| serde_json::json!(
                    {"path": sn, "bytes": sb, "human": format_bytes(*sb)}
                )).collect::<Vec<_>>());
            }
            v
        }).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "status": "success",
            "data": { "instances": data }
        }))?);
        return Ok(());
    }

    match name {
        Some(n) => {
            println!("Disk usage for '{}':", n);
            if let Some((_, total, Some(breakdown))) = rows.first().map(|r| (&r.0, r.1, r.2.as_ref())) {
                for (sub, size) in breakdown.iter().take(12) {
                    if *size > 0 {
                        println!("  {:<24} {:>10}", sub, format_bytes(*size));
                    }
                }
                println!("  {}", "-".repeat(36));
                println!("  {:<24} {:>10}", "TOTAL", format_bytes(total));
            }
        }
        None => {
            println!("{:<28} {:>12}", "INSTANCE", "DISK USAGE");
            println!("{}", "-".repeat(42));
            let mut grand = 0u64;
            for (n, total, _) in &rows {
                grand += total;
                println!("{:<28} {:>12}", n, format_bytes(*total));
            }
            println!("{}", "-".repeat(42));
            println!("{:<28} {:>12}  ({} instance(s))", "TOTAL", format_bytes(grand), rows.len());
        }
    }
    Ok(())
}

/// Benchmark MDL CLI cold latency (v26.3-alpha.6).
fn cmd_bench(format: &str, iterations: u32) -> Result<()> {
    use anyhow::Context as _;
    let exe = std::env::current_exe().context("Failed to resolve current executable")?;
    let rows = util::bench::run_cli_bench(&exe, iterations)?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "status": "success",
            "data": { "iterations_per_command": iterations, "commands": rows }
        }))?);
        return Ok(());
    }

    println!("CLI cold latency ({} iteration(s) per command):", iterations);
    println!("{:<14} {:>9} {:>9} {:>9} {:>9}", "COMMAND", "MIN", "P50", "P95", "MAX");
    println!("{}", "-".repeat(54));
    for r in &rows {
        println!("{:<14} {:>7.1}ms {:>7.1}ms {:>7.1}ms {:>7.1}ms",
            r.command, r.min, r.p50, r.p95, r.max);
    }
    println!();
    println!("Gate these against scripts/perf-bench.ps1 -Baseline (p95 tracked).");
    Ok(())
}

/// Show recorded launch metrics for an instance (v26.2-alpha.9).
async fn cmd_metrics(format: &str, instance: &str, history: bool) -> Result<()> {    use instance::InstanceManager;
    let manager = InstanceManager::new()?;
    let inst = manager.get(instance).await?;

    let entries = if history {
        util::metrics::load_history(&inst.path)
    } else {
        util::metrics::load_latest(&inst.path).into_iter().collect::<Vec<_>>()
    };

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "status": "success",
            "data": { "instance": instance, "launches": entries }
        }))?);
        return Ok(());
    }

    if entries.is_empty() {
        println!("No launch metrics recorded for '{}'. Launch it once to collect data.", instance);
        return Ok(());
    }

    println!("Launch metrics for '{}' ({} record(s)):", instance, entries.len());
    for m in &entries {
        println!();
        println!("  {}  PID {}", m.timestamp, m.pid);
        println!("    spawn:         {:.1}s", m.spawn_secs);
        match m.ready_secs {
            Some(r) => println!("    ready:         {:.1}s", r),
            None => println!("    ready:         n/a (--wait-ready not used)"),
        }
        println!("    downloads:     {} file(s), {}", m.downloads, format_bytes(m.download_bytes));
        let total = m.downloads + m.cache_hits;
        let rate = if total > 0 { (m.cache_hits as f64 / total as f64) * 100.0 } else { 0.0 };
        println!("    cache hits:    {} / {} ({:.0}%)", m.cache_hits, total, rate);
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

async fn cmd_export(instance_name: &str, output_path: &std::path::Path, format: &str) -> Result<()> {
    use anyhow::Context;
    use instance::InstanceManager;

    info!("Exporting instance '{}' (format: {})...", instance_name, format);

    let manager = InstanceManager::new()?;
    let instance = manager.get(instance_name).await?;
    let instance_path = &instance.path;

    if !instance_path.exists() {
        return Err(anyhow::anyhow!("Instance directory does not exist"));
    }

    if format == "mrpack" || output_path.extension().and_then(|e| e.to_str()) == Some("mrpack") {
        // Export as Modrinth .mrpack format.
        let config_data = tokio::fs::read_to_string(instance_path.join("instance.json")).await?;
        let config: instance::config::InstanceConfig = serde_json::from_str(&config_data)?;

        let (mods, overrides) = loader::modpack::export_to_mrpack(instance_path, &config, output_path)?;
        println!("Successfully exported instance '{}' to {} (Modrinth .mrpack)", instance_name, output_path.display());
        println!("  Mods indexed: {}", mods);
        println!("  Override files: {}", overrides);
        return Ok(());
    }

    // Default: plain zip export.
    use std::fs::File;
    use zip::write::{FileOptions, ZipWriter};
    use zip::CompressionMethod;

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

/// List mods across all instances (v26.2-alpha.5).
async fn cmd_mod_list_all(format: &str) -> Result<()> {
    use instance::{InstanceManager, ModManager};

    let manager = InstanceManager::new()?;
    let instances = manager.list().await?;
    let mod_manager = ModManager::new()?;

    let mut all_data = Vec::new();
    let mut total_mods = 0usize;

    for inst in &instances {
        let mods = mod_manager.list_mods(&inst.name).await.unwrap_or_default();
        total_mods += mods.len();
        all_data.push(serde_json::json!({
            "instance": inst.name,
            "version": inst.config.version,
            "loader": inst.config.loader.as_ref().map(|l| l.loader_type.as_str()).unwrap_or("none"),
            "mods": mods,
            "mod_count": mods.len()
        }));
    }

    if format == "json" {
        let json = serde_json::json!({
            "status": "success",
            "data": {
                "instances": all_data,
                "instance_count": instances.len(),
                "total_mods": total_mods
            }
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        println!("Mods across all {} instances:", instances.len());
        println!();
        println!("{:<24} {:<12} {:<10} {:>6}  {}", "INSTANCE", "VERSION", "LOADER", "MODS", "SAMPLE");
        println!("{}", "-".repeat(80));

        for inst in &instances {
            let mods = mod_manager.list_mods(&inst.name).await.unwrap_or_default();
            let loader = inst.config.loader.as_ref().map(|l| l.loader_type.as_str()).unwrap_or("none");
            let sample = mods.first().map(|m| m.filename.as_str()).unwrap_or("-");
            println!("{:<24} {:<12} {:<10} {:>6}  {}", inst.name, inst.config.version, loader, mods.len(), sample);
        }

        println!();
        println!("Total: {} mod(s) across {} instance(s)", total_mods, instances.len());
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

// ---------------------------------------------------------------------------
// Despotes v26.9 automation primitives
// ---------------------------------------------------------------------------

async fn cmd_game_redstone(instance: &str, x: Option<i32>, y: Option<i32>, z: Option<i32>) -> Result<()> {
    if x.is_some() != y.is_some() || x.is_some() != z.is_some() {
        anyhow::bail!("--x, --y and --z must be given together (or all omitted for crosshair probe)");
    }
    let dir = resolve_instance_dir(instance).await?;
    let response = game::client::redstone_query(&dir, x, y, z).await?;
    print_game_response(&response);
    Ok(())
}

async fn cmd_game_schedule(
    instance: &str,
    op: &str,
    name: Option<&str>,
    period_ticks: Option<u64>,
    commands: &[String],
) -> Result<()> {
    let op = op.to_ascii_lowercase();
    match op.as_str() {
        "add" => {
            let name = name.ok_or_else(|| anyhow::anyhow!("schedule add requires --name"))?;
            let ticks = period_ticks
                .ok_or_else(|| anyhow::anyhow!("schedule add requires --period-ticks"))?;
            if commands.is_empty() {
                anyhow::bail!("schedule add requires at least one --command (JSON action)");
            }
            let mut parsed_cmds = Vec::new();
            for c in commands {
                parsed_cmds.push(serde_json::from_str::<serde_json::Value>(c)
                    .with_context(|| format!("Invalid --command JSON: {c}"))?);
            }
            let payload = game::client::schedule_payload(
                "add", Some(name), Some(ticks), Some(serde_json::Value::Array(parsed_cmds)),
            );
            let dir = resolve_instance_dir(instance).await?;
            print_game_response(&game::client::automation_action(&dir, payload).await?);
        }
        "status" | "remove" => {
            if op == "remove" && name.is_none() {
                anyhow::bail!("schedule remove requires --name");
            }
            let payload = game::client::schedule_payload(&op, name, None, None);
            let dir = resolve_instance_dir(instance).await?;
            print_game_response(&game::client::automation_action(&dir, payload).await?);
        }
        other => anyhow::bail!(
            "Unknown schedule op '{other}'. Supported: add / status / remove \
             (or use 'mdl game raw-action' for forward compatibility)"
        ),
    }
    Ok(())
}

async fn cmd_game_macro(
    instance: &str,
    op: &str,
    name: Option<&str>,
    step: Option<&str>,
) -> Result<()> {
    const OPS: &[&str] = &[
        "start-recording", "record-step", "stop-recording",
        "play", "stop", "delete", "status",
    ];
    if !OPS.contains(&op) {
        anyhow::bail!(
            "Unknown macro op '{op}'. Supported: {}",
            OPS.join(", ")
        );
    }
    let step_value = match step {
        Some(s) => Some(serde_json::from_str::<serde_json::Value>(s)
            .with_context(|| format!("Invalid --step JSON: {s}"))?),
        None => None,
    };
    let needs_name = !matches!(op, "stop-recording" | "status" | "stop");
    if needs_name && name.is_none() {
        anyhow::bail!("macro {op} requires --name");
    }
    if op == "record-step" && step_value.is_none() {
        anyhow::bail!("macro record-step requires --step '<json action>'");
    }
    let payload = game::client::macro_payload(op, name, step_value);
    let dir = resolve_instance_dir(instance).await?;
    print_game_response(&game::client::automation_action(&dir, payload).await?);
    Ok(())
}

async fn cmd_game_condition(
    instance: &str,
    if_json: &str,
    then_json: Option<&str>,
    else_json: Option<&str>,
) -> Result<()> {
    let parse_array = |label: &str, raw: Option<&str>| -> Result<Option<serde_json::Value>> {
        match raw {
            None => Ok(None),
            Some(s) => serde_json::from_str::<serde_json::Value>(s)
                .with_context(|| format!("Invalid --{label} JSON (expected an array of actions): {s}"))
                .map(Some),
        }
    };
    let if_query: serde_json::Value = serde_json::from_str(if_json)
        .with_context(|| format!("Invalid --if JSON: {if_json}"))?;
    let then_cmds = parse_array("then", then_json)?
        .unwrap_or_else(|| serde_json::json!([{"type": "ping"}]));
    let else_cmds = parse_array("else", else_json)?;

    let payload = game::client::condition_payload(if_query, then_cmds, else_cmds);
    let dir = resolve_instance_dir(instance).await?;
    print_game_response(&game::client::automation_action(&dir, payload).await?);
    Ok(())
}

async fn cmd_game_raw_action(instance: &str, json_payload: &str) -> Result<()> {
    let command: serde_json::Value = serde_json::from_str(json_payload)
        .with_context(|| format!("Invalid action JSON: {json_payload}"))?;
    let dir = resolve_instance_dir(instance).await?;
    print_game_response(&game::client::automation_action(&dir, command).await?);
    Ok(())
}

/// Hot-attach a Java agent JAR into the running game JVM (v26.2-alpha.6).
/// Resolves the PID from the instance's runtime/pid file, the java runtime
/// from --java-path / instance config / system detection, and delegates to
/// game::attach::inject_agent (embedded AttachHelper via jdk.attach).
async fn cmd_game_inject_agent(
    instance: &str,
    jar: &str,
    params: Option<&str>,
    java_path: Option<&str>,
) -> Result<()> {
    use instance::InstanceManager;

    let manager = InstanceManager::new()?;
    let inst = manager.get(instance).await?;

    // Resolve target PID from the runtime pid file.
    let pid: u32 = tokio::fs::read_to_string(inst.path.join("runtime").join("pid"))
        .await
        .ok()
        .and_then(|c| c.trim().parse().ok())
        .ok_or_else(|| anyhow::anyhow!(
            "Instance '{}' is not running (no runtime/pid). Launch it first.",
            instance
        ))?;

    // Resolve the agent JAR path: absolute/local path wins; otherwise treat
    // as a registered entry name from `mdl javaagent install`.
    let jar_path = std::path::PathBuf::from(jar);
    let jar_path = if jar_path.is_absolute() && jar_path.exists() {
        jar_path
    } else {
        let registered = inst.path.join("javaagents").join(jar);
        if !registered.exists() {
            // Also accept a name without .jar suffix.
            let with_ext = registered.with_extension("jar");
            if with_ext.exists() {
                with_ext
            } else {
                anyhow::bail!(
                    "Agent JAR not found: '{}' is neither an existing path nor a \
                     registered javaagent in this instance (see `mdl javaagent install`)",
                    jar
                );
            }
        } else {
            registered
        }
    };

    // Resolve the java executable used to run the attach helper.
    let java = match java_path {
        Some(p) => std::path::PathBuf::from(p),
        None => crate::version::java::JavaRuntime::detect()
            .map(|r| r.path)
            .unwrap_or_else(|_| std::path::PathBuf::from("java")),
    };

    game::attach::inject_agent(&java, pid, &jar_path, params).await?;
    println!(
        "Agent '{}' attached to instance '{}' (PID {})",
        jar_path.display(),
        instance,
        pid
    );
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

/// Refresh access token(s) via stored Microsoft refresh tokens
/// (v26.2-alpha.6). Targets one account by UUID/username, or all with --all.
async fn cmd_account_refresh(account: Option<&str>, all: bool) -> Result<()> {
    let mut targets = Vec::new();
    if all {
        targets.extend(util::account::list_accounts());
        if targets.is_empty() {
            anyhow::bail!("No cached accounts to refresh. Use `mdl account login` first.");
        }
    } else if let Some(name) = account {
        match util::account::find_account(name) {
            Some(a) => targets.push(a),
            None => anyhow::bail!("Account '{}' not found. Use `mdl account list`.", name),
        }
    } else {
        anyhow::bail!("Specify an account (UUID or username) or pass --all");
    }

    let mut ok = 0usize;
    for acc in &targets {
        match util::account::refresh_account(acc).await {
            Ok(_) => {
                println!("  Refreshed: {} ({})", acc.username, acc.uuid);
                ok += 1;
            }
            Err(e) => println!("  FAILED: {} - {}", acc.username, e),
        }
    }
    println!("{}/{} account(s) refreshed", ok, targets.len());
    if ok < targets.len() {
        std::process::exit(1);
    }
    Ok(())
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

/// Remove installed .aep Refract extensions (v26.2-alpha.9). Filters by
/// filename substring, or removes everything with --all.
async fn cmd_aprism_refract_remove(instance: &str, name: Option<&str>, all: bool) -> Result<()> {
    use anyhow::Context as _;
    if !all && name.is_none() {
        anyhow::bail!("Specify a filename filter or pass --all");
    }
    let manager = instance::InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    let exts = loader::refract::installed_extensions(&inst.path);
    if exts.is_empty() {
        println!("No AprismRefract extensions installed in '{}'.", instance);
        return Ok(());
    }

    let mut removed = 0usize;
    for e in exts {
        let fname = e.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if all || name.map(|n| fname.to_lowercase().contains(&n.to_lowercase())).unwrap_or(false) {
            tokio::fs::remove_file(&e).await.with_context(|| format!("Failed to remove {}", e.display()))?;
            println!("  removed {}", fname);
            removed += 1;
        }
    }
    println!("{} extension(s) removed from '{}'", removed, instance);
    Ok(())
}

/// Remove the installed Prismate bridge (v26.2-alpha.9). Only one bridge
/// should ever be present; removes every Prismate jar found.
async fn cmd_aprism_prismate_remove(instance: &str) -> Result<()> {
    use anyhow::Context as _;
    let manager = instance::InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    let jars = loader::prismate::installed_prismate(&inst.path);
    if jars.is_empty() {
        println!("No AprismPrismate bridge installed in '{}'.", instance);
        return Ok(());
    }
    for p in jars {
        let fname = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        tokio::fs::remove_file(&p).await.with_context(|| format!("Failed to remove {}", p.display()))?;
        println!("  removed {}", fname);
    }
    println!("Prismate bridge removed from '{}'", instance);
    Ok(())
}

/// Unified Aprism ecosystem status for an instance (v26.2-alpha.8).
/// Offline by design: reads the local agent cache, installed Refract
/// extensions, Prismate bridge and native .aje mods; never hits the network.
// ---------------------------------------------------------------------------
// AprismJDK (AJR) commands (v26.4-alpha.6)
// ---------------------------------------------------------------------------

async fn cmd_jdk_available(prerelease: bool, format: &str) -> Result<()> {
    let releases = loader::aprism_jdk::fetch_releases().await?;
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "releases": releases.iter().map(|r| {
                let picked = loader::aprism_jdk::select_release(&[r.clone()], None, prerelease)
                    .map(|(_, (a, p))| serde_json::json!({
                        "asset": a.name,
                        "size_mb": a.size / (1024 * 1024),
                        "version": p.version,
                    }));
                serde_json::json!({"tag": r.tag, "prerelease": r.prerelease, "for_this_host": picked})
            }).collect::<Vec<_>>()
        }))?);
        return Ok(());
    }
    let (os, arch) = loader::aprism_jdk::current_platform();
    println!("AprismJDK releases ({os}/{arch}):");
    for r in &releases {
        let mark = if r.prerelease { " [pre-release]" } else { "" };
        println!("  {}{}", r.tag, mark);
        match loader::aprism_jdk::select_release(std::slice::from_ref(r), None, prerelease) {
            Some((_, (a, p))) => println!(
                "    -> {} ({} MB, {})",
                a.name,
                a.size / (1024 * 1024),
                p.ext
            ),
            None => println!("    -> no runtime asset for this host"),
        }
    }
    Ok(())
}

async fn cmd_jdk_install(version: Option<&str>, prerelease: bool) -> Result<()> {
    let releases = loader::aprism_jdk::fetch_releases().await?;
    let (rel, (asset, parsed)) = loader::aprism_jdk::select_release(&releases, version, prerelease)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No AprismJDK release matches version={:?} prerelease={prerelease} for this host",
                version.unwrap_or("<latest stable>")
            )
        })?;
    println!("Installing AprismJDK {} ({} MB)...", rel.tag, asset.size / (1024 * 1024));
    let java = loader::aprism_jdk::download_and_install(rel, asset).await?;
    let runtime = crate::version::java::JavaRuntime::from_path(&java)?;
    println!(
        "Installed AprismJDK {} (Java {}) -> {}",
        rel.tag,
        runtime.version,
        java.display()
    );
    println!("Use it with: mdl launch <instance> --jdk aprism@{}", parsed.version);
    Ok(())
}

fn cmd_jdk_list(format: &str) -> Result<()> {
    let installed = loader::aprism_jdk::installed();
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "installed": installed.iter().map(|(tag, java)|
                serde_json::json!({"tag": tag, "java": java.display().to_string()})
            ).collect::<Vec<_>>()
        }))?);
        return Ok(());
    }
    if installed.is_empty() {
        println!("No AprismJDK runtimes installed. Try 'mdl jdk install'.");
        return Ok(());
    }
    println!("Installed AprismJDK runtimes:");
    for (tag, java) in &installed {
        println!("  {tag} -> {}", java.display());
    }
    Ok(())
}

fn cmd_jdk_remove(tag: &str) -> Result<()> {
    if loader::aprism_jdk::remove(tag)? {
        println!("Removed AprismJDK {tag}");
    } else {
        anyhow::bail!("No installed AprismJDK tagged '{tag}'. See 'mdl jdk list'.");
    }
    Ok(())
}

async fn cmd_aprism_status(format: &str, instance: &str) -> Result<()> {    use instance::{InstanceManager, ModManager};
    let manager = InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    let loader_type = inst.config.loader.as_ref().map(|l| l.loader_type.clone());

    // 1. Cached JE agent JARs (<data>/aprism/*.jar), parsed for tag + MC.
    let mut cached_agents: Vec<(String, String)> = Vec::new(); // (tag, mc)
    if let Ok(dir) = crate::util::paths::get_data_dir() {
        if let Ok(entries) = std::fs::read_dir(dir.join("aprism")) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !name.to_ascii_lowercase().ends_with(".jar") {
                    continue;
                }
                match loader::aprism::parse_asset_name(&name) {
                    Some((tag, edit, mc)) if edit == "JE" => cached_agents.push((tag, mc)),
                    _ => cached_agents.push((name.clone(), "?".into())),
                }
            }
        }
    }
    let covering_agent = cached_agents.iter()
        .find(|(_, mc)| *mc == inst.config.version);

    // 2. Refract .aep extensions in the instance.
    let refract_exts = loader::refract::installed_extensions(&inst.path);

    // 3. Prismate bridge.
    let prismate_jars = loader::prismate::installed_prismate(&inst.path);
    let prismate_on = loader::prismate::is_installed(&inst.path);

    // 4. Native .aje mods via mod manager.
    let aje_mods: Vec<String> = match ModManager::new() {
        Ok(mm) => mm.list_mods(instance).await.unwrap_or_default()
            .into_iter()
            .filter(|m| m.kind == "aje" && m.enabled)
            .map(|m| m.filename)
            .collect(),
        Err(_) => Vec::new(),
    };

    // 5. AprismJDK runtimes in the MDL java cache (v26.4-alpha.6).
    let aprism_jdks = loader::aprism_jdk::installed();

    // Compatibility notes (mirrors launch-time rules).
    let mut notes: Vec<String> = Vec::new();
    if prismate_on {
        notes.push("Prismate present: launching with --aprism is refused (agent and Prismate are mutually exclusive)".into());
    }
    if covering_agent.is_none() && !cached_agents.is_empty() {
        notes.push(format!(
            "No cached agent covers MC {}; launch --aprism will query GitHub Releases",
            inst.config.version
        ));
    }

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "instance": inst.name,
            "mc_version": inst.config.version,
            "loader": loader_type,
            "agent": {
                "cached": cached_agents.iter().map(|(t, mc)| serde_json::json!({"tag": t, "mc": mc})).collect::<Vec<_>>(),
                "covers_instance": covering_agent.map(|(t, _)| t),
            },
            "refract_extensions": refract_exts.iter().map(|p|
                p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            ).collect::<Vec<_>>(),
            "prismate": {
                "installed": prismate_on,
                "jars": prismate_jars.iter().map(|p|
                    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                ).collect::<Vec<_>>(),
            },
            "native_aje_mods": aje_mods,
            "aprism_jdk": aprism_jdks.iter().map(|(tag, java)|
                serde_json::json!({"tag": tag, "java": java.display().to_string()})
            ).collect::<Vec<_>>(),
            "notes": notes,
        }))?);
        return Ok(());
    }

    println!("Aprism ecosystem status: '{}' (MC {}, loader {})",
        inst.name, inst.config.version,
        loader_type.as_deref().unwrap_or("none"));
    println!();
    println!("Agent cache ({}):", cached_agents.len());
    for (tag, mc) in &cached_agents {
        let mark = if Some(mc) == covering_agent.as_ref().map(|(_, m)| m) { " [covers this instance]" } else { "" };
        println!("  {} (MC {}){}", tag, mc, mark);
    }
    if cached_agents.is_empty() { println!("  (empty — first --aprism launch downloads it)"); }

    println!("Refract (.aep): {}", if refract_exts.is_empty() { "none".to_string() } else { "".to_string() });
    for e in &refract_exts {
        println!("  {}", e.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
    }

    println!("Prismate: {}", if prismate_on { "installed" } else { "not installed" });
    for p in &prismate_jars {
        println!("  {}", p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
    }

    println!("Native mods (.aje): {}", aje_mods.len());
    for m in &aje_mods {
        println!("  {}", m);
    }

    println!("AprismJDK: {}", if aprism_jdks.is_empty() {
        "not installed ('mdl jdk install' to provision)".to_string()
    } else {
        format!("{} runtime(s)", aprism_jdks.len())
    });
    for (tag, java) in &aprism_jdks {
        println!("  {tag} -> {}", java.display());
    }

    if !notes.is_empty() {
        println!();
        println!("Notes:");
        for n in &notes {
            println!("  - {}", n);
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

// ---------- v26.2-alpha.5: JavaAgent management ----------

async fn cmd_javaagent_install(instance: &str, jar: &str, params: Option<&str>) -> Result<()> {
    use instance::InstanceManager;

    let manager = InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    let jar_path = std::path::Path::new(jar);
    if !jar_path.exists() {
        anyhow::bail!("JAR file not found: {}", jar);
    }

    let name = instance::javaagent::install(&inst.path, jar_path, params).await?;
    println!("JavaAgent '{}' installed in instance '{}'", name, instance);
    if let Some(p) = params {
        println!("  Parameters: {}", p);
    }
    Ok(())
}

async fn cmd_javaagent_list(instance: &str) -> Result<()> {
    use instance::InstanceManager;

    let manager = InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    let agents = instance::javaagent::list(&inst.path).await?;

    if agents.is_empty() {
        println!("No JavaAgents registered in instance '{}'", instance);
        return Ok(());
    }

    println!("JavaAgents in instance '{}':", instance);
    println!("{:<24} {:<12} {:<10} {}", "NAME", "ENABLED", "PARAMS", "PATH");
    println!("{}", "-".repeat(70));
    for a in &agents {
        let enabled = if a.enabled { "yes" } else { "no" };
        let params = a.params.as_deref().unwrap_or("-");
        println!("{:<24} {:<12} {:<10} {}", a.name, enabled, params, a.path);
    }
    Ok(())
}

async fn cmd_javaagent_remove(instance: &str, name: &str) -> Result<()> {
    use instance::InstanceManager;

    let manager = InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    instance::javaagent::remove(&inst.path, name).await?;
    println!("JavaAgent '{}' removed from instance '{}'", name, instance);
    Ok(())
}

async fn cmd_javaagent_enable(instance: &str, name: &str) -> Result<()> {
    use instance::InstanceManager;

    let manager = InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    instance::javaagent::enable(&inst.path, name).await?;
    println!("JavaAgent '{}' enabled in instance '{}'", name, instance);
    Ok(())
}

async fn cmd_javaagent_disable(instance: &str, name: &str) -> Result<()> {
    use instance::InstanceManager;

    let manager = InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    instance::javaagent::disable(&inst.path, name).await?;
    println!("JavaAgent '{}' disabled in instance '{}'", name, instance);
    Ok(())
}

async fn cmd_inject(target: &str, dll: &str) -> Result<()> {
    let pid: u32 = target.parse().unwrap_or_else(|_| {
        util::injector::find_pid_by_name(target).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        })
    });

    // v26.4-alpha.2 bug fix: JVM targets (java/javaw) now go through the
    // JavaAgent + System.load() path instead of CreateRemoteThread. JDK 25+
    // CFG/CET mitigations crash the remote-thread path before DllMain runs.
    // Non-JVM targets (e.g. bedrock_server.exe) keep the legacy path.
    if util::injector::is_jvm_process(pid) {
        let java = crate::version::java::JavaRuntime::detect()
            .map(|r| r.path)
            .unwrap_or_else(|_| std::path::PathBuf::from("java"));
        let base = crate::util::paths::get_data_dir()
            .map(|d| d.to_path_buf())
            .unwrap_or_else(|_| std::env::temp_dir());
        let agent_jar = game::native_loader::ensure_agent_jar(&base)?;
        game::attach::inject_agent(&java, pid, &agent_jar, Some(dll)).await?;
        println!(
            "Loaded {} into JVM PID {} via JavaAgent (System.load)",
            dll, pid
        );
    } else {
        util::injector::inject_dll(pid, std::path::Path::new(dll))?;
        println!("Injected {} into PID {}", dll, pid);
    }
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
        javaagents: Vec::new(),
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

async fn cmd_server_launch(name: &str, attach: bool, wait_ready: bool, ready_timeout: u64) -> Result<()> {
    let info = loader::server::load_server(name)?;
    let dir = info.dir()?;
    let pid = loader::server::launch_server(&info, !attach).await?;
    if !attach {
        println!("Server '{}' running in background (PID {})", name, pid);
        println!("  Log: {}/server.log", dir.display());
        if wait_ready {
            println!("Waiting for server ready (timeout {}s)...", ready_timeout);
            loader::server::wait_for_ready(&dir, ready_timeout).await?;
            println!("Server '{}' is ready.", name);
        }
    }
    Ok(())
}

/// Run a console command on a managed server via RCON (v26.2-alpha.7).
async fn cmd_server_cmd(name: &str, command: &str) -> Result<()> {
    if command.trim().is_empty() {
        anyhow::bail!("Empty console command");
    }
    let info = loader::server::load_server(name)?;
    // Refuse when the server is not running — RCON would be connection-refused
    // anyway, but the explicit check gives a clearer error.
    let running = info.dir().ok().and_then(|d| loader::server::running_pid(&d));
    if running.is_none() {
        anyhow::bail!("Server '{}' is not running", name);
    }
    let response = loader::server::run_console_command(&info, command).await?;
    if response.trim().is_empty() {
        println!("(empty response)");
    } else {
        println!("{}", response.trim_end());
    }
    Ok(())
}

async fn cmd_server_stop(name: &str) -> Result<()> {
    let info = loader::server::load_server(name)?;
    loader::server::stop_server(&info).await?;
    println!("Server '{}' stopped", name);
    Ok(())
}

/// Rotate a managed server's RCON password (v26.3-alpha.5).
fn cmd_server_rotate_rcon(name: &str, show: bool) -> Result<()> {
    let mut info = loader::server::load_server(name)?;
    let running = info.dir().ok().and_then(|d| loader::server::running_pid(&d));
    let new_pw = loader::server::rotate_rcon_password(&mut info)?;
    println!("RCON password rotated for '{}'.", name);
    if show {
        println!("  New password: {new_pw}");
    } else {
        println!("  New password: ******** (pass --show to display; also stored in server.json)");
    }
    if running.is_some() {
        println!("Note: the server is RUNNING and still uses the old password — restart it to pick up the new one.");
    } else {
        println!("Applies on next start.");
    }
    Ok(())
}

/// Path to a managed server's directory (helper for the file-backed
/// management commands below).
fn server_dir_of(name: &str) -> Result<std::path::PathBuf> {
    Ok(loader::server::load_server(name)?.dir()?)
}

// --- v26.3-alpha.4: structured properties editing ---

fn cmd_server_props_list(name: &str) -> Result<()> {
    let dir = server_dir_of(name)?;
    let props = loader::props::PropertiesFile::load(&dir.join("server.properties"))?;
    for pair in props.pairs() {
        println!("{} = {}", pair.key, pair.value);
    }
    Ok(())
}

fn cmd_server_props_get(name: &str, key: &str) -> Result<()> {
    let dir = server_dir_of(name)?;
    let props = loader::props::PropertiesFile::load(&dir.join("server.properties"))?;
    match props.get(key) {
        Some(v) => println!("{v}"),
        None => anyhow::bail!("Key '{}' not found in server.properties", key),
    }
    Ok(())
}

fn cmd_server_props_set(name: &str, key: &str, value: &str) -> Result<()> {
    let dir = server_dir_of(name)?;
    let path = dir.join("server.properties");
    let mut props = loader::props::PropertiesFile::load(&path)?;
    props.set(key, value);
    props.save(&path)?;
    println!("{key} = {value}");
    if loader::server::running_pid(&dir).is_some() {
        println!("Note: server is running — most properties need a restart to take effect.");
    }
    Ok(())
}

// --- v26.3-alpha.4: allowlist / op / ban wrappers ---

/// Run one console command via RCON with a consistent error surface.
async fn cmd_server_rcon(name: &str, command: &str) -> Result<()> {
    cmd_server_cmd(name, command).await
}

/// Allowlist listing: prefer RCON when running (authoritative live view),
/// fall back to whitelist.json when stopped.
async fn cmd_server_allowlist_list(name: &str) -> Result<()> {
    let info = loader::server::load_server(name)?;
    let running = info.dir().ok().and_then(|d| loader::server::running_pid(&d)).is_some();
    if running {
        return cmd_server_cmd(name, "whitelist list").await;
    }
    let names = loader::props::json_names(&info.dir()?.join("whitelist.json"));
    if names.is_empty() {
        println!("Allowlist is empty (or whitelist.json not present yet).");
    } else {
        println!("Allowlisted players (from whitelist.json):");
        for n in names {
            println!("  {n}");
        }
    }
    Ok(())
}

/// Toggle white-list + enforce-whitelist in server.properties. Works when
/// the server is stopped; warns that a restart is needed otherwise.
fn cmd_server_toggle_whitelist(name: &str, enable: bool) -> Result<()> {
    let dir = server_dir_of(name)?;
    let path = dir.join("server.properties");
    let mut props = loader::props::PropertiesFile::load(&path)?;
    let flag = if enable { "true" } else { "false" };
    props.set("white-list", flag);
    props.set("enforce-whitelist", flag);
    props.save(&path)?;
    println!(
        "Allowlist {} in server.properties.",
        if enable { "ENABLED (white-list + enforce-whitelist = true)" } else { "disabled" }
    );
    if loader::server::running_pid(&dir).is_some() {
        println!("Note: server is running — restart required for this to take effect.");
    } else {
        println!("Applies on next start.");
    }
    Ok(())
}

/// List player names from a vanilla JSON list file (ops.json /
/// banned-players.json). Works whether the server is running or not.
fn cmd_server_json_names(name: &str, file: &str, label: &str) -> Result<()> {
    let dir = server_dir_of(name)?;
    let names = loader::props::json_names(&dir.join(file));
    if names.is_empty() {
        println!("{label}: none ({file} empty or absent)");
    } else {
        println!("{label}:");
        for n in names {
            println!("  {n}");
        }
    }
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
