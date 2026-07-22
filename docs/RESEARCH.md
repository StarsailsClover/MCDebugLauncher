# MCDebugLauncher Research Document

## Project Overview

MCDebugLauncher (MDL) is a command-line Minecraft launcher designed for developers, testers, and AI agents. It enables rapid testing of Minecraft with any mod loader (including OptiFine), automated mod installation, comprehensive error logging, and specialized developer/agent features.

## Core Requirements

1. **Multi-Loader Support**: Vanilla, Forge, NeoForge, Fabric, Quilt, LegacyFabric, OptiFine
2. **Rapid Testing**: One-command installation and launch for any version + loader combination
3. **Error Diagnostics**: Automatic collection and export of crash reports, logs, and diagnostic data
4. **Agent-Friendly**: Structured output formats (JSON) for programmatic control
5. **Developer Tools**: Debug logging, performance profiling, headless mode support
6. **Enterprise-Grade**: Production-ready code quality, comprehensive error handling

## Technical Research Summary

### 1. Minecraft Version Management

**Official APIs (Mojang/Microsoft)**:
- Version Manifest: `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`
- Returns all available Minecraft versions with metadata
- Each version links to a detailed JSON with:
  - Client/server download URLs with SHA1 hashes
  - Asset index URLs
  - Library dependencies (platform-specific)
  - Java version requirements
  - Launch arguments (JVM and game)

**Implementation Pattern**:
```
1. Fetch version_manifest_v2.json
2. Parse and filter by version type (release/snapshot)
3. Download specific version JSON
4. Download client.jar, libraries, assets
5. Verify SHA1 checksums
6. Construct launch command
```

### 2. Mod Loader Installation

#### Fabric
- **Installer API**: `https://meta.fabricmc.net/v2/versions/loader`
- **Installation**: Download installer JAR, run with `java -jar` in `client` mode
- **Dependencies**: Requires separate Fabric API mod installation
- **Version Format**: `fabric:<mc_version>` or `fabric:<loader_version>`

#### Forge / NeoForge
- **Forge**: `https://files.minecraftforge.net/net/minecraftforge/forge/index.html`
- **NeoForge**: `https://maven.neoforged.net/releases/net/neoforged/neoforge/`
- **Installation**: Run installer JAR, generates profile in vanilla launcher structure
- **Format**: Creates version JSON with libraries and tweakers
- **Note**: NeoForge is the modern fork (1.20.2+), Forge for legacy versions

#### Quilt
- **Installer**: `https://quiltmc.org/install/`
- **Similar to Fabric**: Lightweight, requires Quilted Fabric API
- **Fabric Compatible**: Can run most Fabric mods

#### OptiFine
- **No Official API**: Must scrape or use mirrors (e.g., BMCLAPI)
- **Installation Process**:
  1. Download OptiFine installer JAR
  2. Extract with `java -cp <installer> optifine.Patcher <vanilla.jar> <installer> <output>`
  3. Install launchwrapper if bundled
  4. Generate version JSON with tweaker class
- **Compatibility**: Can be installed standalone or on top of Forge/Fabric (via OptiFabric)

### 3. Existing Launcher Architectures

#### PrismLauncher (C++/Qt)
- **Architecture**: Centralized `Application` singleton, Qt Model/View pattern
- **Task System**: Asynchronous task chains with progress tracking
- **Instance Management**: Isolated instances with separate configs
- **Strengths**: Mature codebase, comprehensive GUI
- **Limitation**: GUI-centric, not designed for CLI/automation

#### PortableMC (Python/Rust)
- **Python Version**: Single-file script, minimal dependencies
- **Rust Version**: Fast, compiled, modern architecture
- **Features**:
  - One-command launch: `portablemc start <version>`
  - Mod loader prefixes: `fabric:1.21.4`, `forge:1.20.1`
  - Headless LWJGL patching for CI/CD
  - Java runtime auto-download
- **Strengths**: Clean CLI design, automation-friendly
- **Architecture**: Modular, version-agnostic core

#### HeadlessMC (Java)
- **Purpose**: Headless Minecraft testing in CI/CD pipelines
- **Features**:
  - LWJGL patching for headless mode
  - HMC-Specifics mods for command-line control
  - In-memory JVM launching
  - Game automation via commands (`msg`, `gui`, `click`, `connect`)
- **Use Case**: Automated testing, mod development, CI pipelines
- **Strengths**: Comprehensive testing capabilities

#### MC-CLI / Shard (Rust)
- **Purpose**: LLM agent control interface
- **Features**:
  - JSON-structured output for parsing
  - Commands: `status`, `teleport`, `shader`, `capture`, `analyze`
  - Multi-instance management
  - Built-in screenshot/comparison tools
- **Target**: AI agents, automated workflows
- **Architecture**: Client-server model (TCP JSON)

### 4. Log Collection and Error Analysis

#### Log Files Location
- **Vanilla/Paper/Spigot**: `logs/latest.log`
- **Forge**: `logs/latest.log`, `logs/debug.log` (with debug config)
- **Fabric**: `logs/latest.log`, `fabricloader.log` (on critical errors)
- **Crash Reports**: `crash-reports/crash-<timestamp>.txt`
- **JVM Crashes**: `hs_err_pid<pid>.log`

#### Log4j Configuration
- **Enable Debug Logging**: `-Dlog4j.configurationFile=<custom_log4j.xml>`
- **Packet Logging**: Marker `NETWORK_PACKETS` for protocol debugging
- **Custom Log Levels**: `trace`, `debug`, `info`, `warn`, `error`

#### Crash Report Analysis
Key sections:
1. **Description**: Error type (e.g., `NoClassDefFoundError`, `NullPointerException`)
2. **Stack Trace**: Identify mod package names in trace
3. **Suspected Mods**: Forge auto-identifies potential culprits
4. **System Details**: Java version, OS, memory, mod list
5. **Mixin Crashes**: Look for `mod-id$handlerName` patterns

#### Automated Diagnostic Tools
- **Forge**: Built-in `CrashReportAnalyser` (scans stack traces for mod packages)
- **Fabric**: Mixin errors clearly identify target classes
- **Online Analyzers**: mclo.gs, pastebin parsers
- **Common Patterns**:
  - Missing dependencies: `NoClassDefFoundError`
  - Version mismatch: `UnsupportedClassVersionError`
  - Mod conflicts: `ConcurrentModificationException`, Mixin crashes

### 5. Agent-Friendly Design Patterns

#### Structured Output
- **JSON Format**: All command outputs should be machine-parseable
- **Exit Codes**: Standard Unix conventions (0 = success, non-zero = error)
- **Progress Indicators**: Percentage-based or event-stream formats

#### Command Interface Examples
```bash
# Version listing
mdl versions --format json --type release

# Installation with progress
mdl install fabric:1.21.4 --progress json

# Launch with structured logging
mdl launch my-instance --log-format json --output logs/session.jsonl

# Diagnostic export
mdl diagnose my-instance --export diagnostics.tar.gz
```

#### Agent Control Patterns (from MC-CLI/HeadlessMC)
- **Status Queries**: Game state, player position, inventory
- **Commands**: Send chat/commands to running instance
- **Capture**: Screenshots, world data, performance metrics
- **Events**: Subscribe to game events (join, damage, chat)

## Technology Stack Recommendations

### Option 1: Rust (Recommended)
**Pros**:
- Fast, memory-safe, single binary distribution
- Excellent error handling (Result/Option types)
- Strong async ecosystem (tokio)
- Cross-platform with no runtime dependencies
- JSON serialization (serde)
- PortableMC Rust crate available as reference

**Cons**:
- Steeper learning curve
- Longer compilation times

**Libraries**:
- `clap`: CLI argument parsing
- `serde`/`serde_json`: JSON handling
- `reqwest`: HTTP client for API calls
- `tokio`: Async runtime
- `sha1`: Checksum verification
- `zip`: Archive handling
- `log`/`tracing`: Logging infrastructure

### Option 2: Python
**Pros**:
- Rapid development
- Rich ecosystem (requests, click, rich for CLI)
- Easy JSON/API handling
- PortableMC reference implementation

**Cons**:
- Requires Python runtime
- Slower execution
- Distribution complexity (PyInstaller, etc.)

### Option 3: Go
**Pros**:
- Fast compilation, single binary
- Good standard library
- Excellent concurrency
- Cross-compilation support

**Cons**:
- Less mature Minecraft ecosystem
- Verbose error handling

## Proposed Architecture

### Core Components

#### 1. Version Manager
- Fetch and cache version manifests
- Download and verify game files
- Manage Java runtime installations
- Track installed versions

#### 2. Loader Manager
- Abstract interface for all mod loaders
- Loader-specific installers (Fabric, Forge, NeoForge, Quilt, OptiFine)
- Dependency resolution
- Version compatibility checking

#### 3. Instance Manager
- Create/delete/list instances
- Instance configuration (memory, JVM args, mods)
- Instance isolation (separate .minecraft folders)
- Import/export capabilities

#### 4. Launch Manager
- Construct launch commands
- Environment variable setup
- Process management
- Output capture and parsing

#### 5. Diagnostic Manager
- Log collection and aggregation
- Crash report parsing
- Automated error detection
- Diagnostic package generation

#### 6. Agent Interface
- JSON-RPC or REST API server
- Command execution
- Event streaming
- State queries

### Project Structure
```
MCDebugLauncher/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── version/             # Version management
│   │   ├── manifest.rs
│   │   ├── downloader.rs
│   │   └── java.rs
│   ├── loader/              # Mod loader support
│   │   ├── fabric.rs
│   │   ├── forge.rs
│   │   ├── neoforge.rs
│   │   ├── quilt.rs
│   │   └── optifine.rs
│   ├── instance/            # Instance management
│   │   ├── config.rs
│   │   ├── manager.rs
│   │   └── launcher.rs
│   ├── diagnostic/          # Error analysis
│   │   ├── log_parser.rs
│   │   ├── crash_analyzer.rs
│   │   └── collector.rs
│   ├── agent/               # Agent interface
│   │   ├── server.rs
│   │   ├── commands.rs
│   │   └── events.rs
│   └── util/                # Utilities
│       ├── http.rs
│       ├── checksum.rs
│       └── archive.rs
├── docs/
│   ├── RESEARCH.md          # This document
│   ├── ARCHITECTURE.md      # Detailed design
│   └── API.md               # Command reference
├── tests/
│   ├── integration/
│   └── unit/
├── Cargo.toml               # Rust dependencies
├── README.md                # English
├── README_CN.md             # Chinese
└── LICENSE                  # Apache 2.0
```

## Implementation Phases

### Phase 1: Foundation (Weeks 1-2)
- [ ] Project setup with Rust/Cargo
- [ ] CLI framework with clap
- [ ] Version manifest fetching
- [ ] Basic vanilla Minecraft download and launch
- [ ] Configuration management

### Phase 2: Mod Loaders (Weeks 3-4)
- [ ] Fabric installer implementation
- [ ] Forge installer implementation
- [ ] NeoForge installer implementation
- [ ] Quilt installer implementation
- [ ] OptiFine installer implementation

### Phase 3: Instance Management (Week 5)
- [ ] Instance creation/deletion
- [ ] Instance isolation
- [ ] Mod folder management
- [ ] Configuration profiles

### Phase 4: Diagnostics (Week 6)
- [ ] Log file collection
- [ ] Crash report parsing
- [ ] Error pattern detection
- [ ] Diagnostic package export

### Phase 5: Agent Interface (Week 7)
- [ ] JSON output formatting
- [ ] Command API design
- [ ] Event streaming
- [ ] Documentation

### Phase 6: Testing & Polish (Week 8)
- [ ] Integration tests
- [ ] CI/CD setup
- [ ] Documentation completion
- [ ] Binary packaging

## Key Design Decisions

### 1. No GUI
Pure command-line interface for maximum automation potential. Users who need GUI can use PrismLauncher, MultiMC, etc.

### 2. Standard Directory Structure
Use official Minecraft directory structure (`.minecraft/`) for maximum compatibility with existing tools and mods.

### 3. JSON-First Output
All commands support `--format json` for machine parsing. Human-readable format is the default.

### 4. Isolated Instances
Each instance is completely independent with its own mods, configs, saves, and Java version.

### 5. Aggressive Error Handling
Never silently fail. Always provide actionable error messages with suggested fixes.

### 6. Offline-First
Cache everything locally. Support fully offline operation after initial download.

## Risk Analysis

### Technical Risks
1. **OptiFine Installation**: No official API, requires JAR introspection
   - *Mitigation*: Use BMCLAPI mirror, implement robust fallback mechanisms

2. **Launcher Changes**: Mojang may change version manifest format
   - *Mitigation*: Version format detection, backward compatibility

3. **Mod Loader Updates**: Installers may change their CLI interfaces
   - *Mitigation*: Version-specific installers, update detection

4. **Java Compatibility**: Different versions require different Java runtimes
   - *Mitigation*: Auto-detect and download appropriate Java from Mojang

### Maintenance Risks
1. **Mod Loader Ecosystem**: New loaders may emerge
   - *Mitigation*: Plugin architecture for loader support

2. **Breaking Changes**: Minecraft updates may break assumptions
   - *Mitigation*: Comprehensive test suite, version-specific handling

## References

- [Minecraft Version Manifest](https://piston-meta.mojang.com/mc/game/version_manifest_v2.json)
- [PortableMC GitHub](https://github.com/mindstorm38/portablemc)
- [PrismLauncher Architecture](https://github.com/PrismLauncher/PrismLauncher)
- [HeadlessMC](https://github.com/headlesshq/headlessmc)
- [MC-CLI](https://github.com/Th0rgal/mc-cli)
- [Fabric Meta API](https://meta.fabricmc.net/)
- [Forge Files](https://files.minecraftforge.net/)
- [NeoForge Maven](https://maven.neoforged.net/)
- [Wiki.vg Game Files](https://wiki.vg/Game_Files)

## Next Steps

1. Finalize technology choice (Rust recommended)
2. Set up project repository structure
3. Implement Phase 1: Foundation
4. Create comprehensive API documentation
5. Establish testing framework
6. Begin CI/CD pipeline setup
