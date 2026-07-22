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
        Commands::Agent { port, bind } => {
            cmd_agent(port, &bind).await?;
        }
        Commands::Info => {
            cmd_info(&cli.format).await?;
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

async fn cmd_launch(name: &str, _username: Option<&str>, _server: Option<&str>, _fullscreen: bool, _width: Option<u32>, _height: Option<u32>, _detach: bool) -> Result<()> {
    use instance::InstanceLauncher;

    let launcher = InstanceLauncher::new()?;
    launcher.launch(name).await?;

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

async fn cmd_agent(port: u16, bind_address: &str) -> Result<()> {
    use agent::server::AgentServer;

    info!("Starting agent server on {}:{}", bind_address, port);

    let server = AgentServer::new();
    server.start(port, bind_address).await?;

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
