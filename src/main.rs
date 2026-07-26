// MCDebugLauncher - Main entry point

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

mod version;
mod loader;
mod instance;
mod diagnostic;
mod agent;
mod util;

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
    },

    /// List instances
    List,

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

        /// Run in background
        #[arg(long)]
        detach: bool,
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
    },

    /// Manage mods
    #[command(subcommand)]
    Mod(ModCommands),

    /// Manage configuration
    #[command(subcommand)]
    Config(ConfigCommands),

    /// Delete an instance
    Delete {
        /// Instance name
        name: String,
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

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

    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_ansi(!cli.no_color)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("MCDebugLauncher v{}", env!("CARGO_PKG_VERSION"));

    // Kick off a best-effort GitHub update check concurrently with the command.
    // It is throttled by an on-disk cache and never blocks or fails the command.
    let update_check = tokio::spawn(util::update::check_for_update());

    // Execute command
    match cli.command {
        Commands::Versions { type_filter, limit, search } => {
            cmd_versions(&cli.format, &type_filter, limit, search.as_deref()).await?;
        }
        Commands::VersionInfo { version, show_libraries, show_assets } => {
            cmd_version_info(&cli.format, &version, show_libraries, show_assets).await?;
        }
        Commands::Create { name, mc_version, loader, loader_version, memory, no_install } => {
            cmd_create(&name, &mc_version, loader.as_deref(), loader_version.as_deref(), memory.as_deref(), no_install).await?;
        }
        Commands::List => {
            cmd_list(&cli.format).await?;
        }
        Commands::Launch { name, username, server, fullscreen, width, height, detach } => {
            cmd_launch(&name, username.as_deref(), server.as_deref(), fullscreen, width, height, detach).await?;
        }
        Commands::Diagnose { name, export, analyze } => {
            cmd_diagnose(&name, export.as_deref(), analyze).await?;
        }
        Commands::Logs { name, follow, lines, level } => {
            cmd_logs(&name, follow, lines, level.as_deref()).await?;
        }
        Commands::Status { name } => {
            cmd_status(&cli.format, name.as_deref()).await?;
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
        Commands::Delete { name } => {
            cmd_delete(&name).await?;
        }
        Commands::Agent { port, bind } => {
            cmd_agent(port, &bind).await?;
        }
        Commands::Info => {
            cmd_info(&cli.format).await?;
        }
    }

    // Surface any available update after the command completes so the notice is
    // the last thing the user sees. Non-JSON output only, to keep machine-
    // readable output clean.
    if cli.format != "json" {
        if let Ok(Some(info)) = update_check.await {
            eprintln!(
                "\nA new version of MCDebugLauncher is available: {} -> {}\nDownload: {}\n",
                info.current, info.latest, info.url
            );
        }
    }

    Ok(())
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

async fn cmd_create(name: &str, version: &str, loader: Option<&str>, loader_version: Option<&str>, _memory: Option<&str>, no_install: bool) -> Result<()> {
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

    println!("Instance '{}' created successfully", instance.name);
    println!("Path: {}", instance.path.display());

    Ok(())
}

async fn cmd_list(format: &str) -> Result<()> {
    use instance::InstanceManager;

    let manager = InstanceManager::new()?;
    let instances = manager.list().await?;

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

async fn cmd_launch(name: &str, username: Option<&str>, server: Option<&str>, fullscreen: bool, width: Option<u32>, height: Option<u32>, _detach: bool) -> Result<()> {
    use instance::{InstanceLauncher, launcher::LaunchOptions};

    let options = LaunchOptions {
        username: username.map(str::to_string),
        server: server.map(str::to_string),
        fullscreen,
        width,
        height,
    };

    let launcher = InstanceLauncher::new()?;
    launcher.launch(name, &options).await?;

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

async fn cmd_status(format: &str, name: Option<&str>) -> Result<()> {
    use instance::InstanceStatus;

    let status = InstanceStatus::new()?;

    if let Some(instance_name) = name {
        // Show single instance status
        let info = status.get_instance_status(instance_name).await?;

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

async fn cmd_delete(name: &str) -> Result<()> {
    use instance::InstanceManager;

    let manager = InstanceManager::new()?;
    manager.delete(name).await?;

    println!("Instance '{}' deleted", name);
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
    use std::path::Path;

    let config_manager = ConfigManager::new()?;
    let config = config_manager.export_config(instance).await?;

    let json = serde_json::to_string_pretty(&config)?;
    tokio::fs::write(output, json).await?;

    println!("Exported configuration to: {}", output);
    Ok(())
}

async fn cmd_config_import(instance: &str, input: &str) -> Result<()> {
    use instance::ConfigManager;
    use std::path::Path;

    let content = tokio::fs::read_to_string(input).await?;
    let config: serde_json::Value = serde_json::from_str(&content)?;

    let config_manager = ConfigManager::new()?;
    config_manager.import_config(instance, &config).await?;

    println!("Imported configuration from: {}", input);
    Ok(())
}

/// Check whether a process with the given PID is currently running.
fn agent_pid_running(pid: u32) -> bool {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes();
    sys.process(sysinfo::Pid::from_u32(pid)).is_some()
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
