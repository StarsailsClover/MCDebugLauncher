# Changelog

All notable changes to MCDebugLauncher will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [26.0.0-alpha.5] - 2026-07-31

### Fixed
- **Critical**: NeoForge "Missing main class net.minecraft.client.main.Main" fatal startup error
  - Added fallback to vanilla client JAR when NeoForge patched client is missing
  - Ensures Minecraft core classes are always available on classpath
  - Prevents mod loading failures in NeoForge instances

### Added
- **Self-Update System**
  - `mdl update --check` - Check for new versions without installing
  - `mdl update` - Interactive update with automatic backup
  - GitHub API integration for release checking
  - Semantic version comparison
  - Automatic binary download and replacement
  - Backup creation (.exe.bak) before update
  - Windows update script generation for safe replacement

- **Environment Setup**
  - `mdl setup` - Add MDL to system PATH automatically
  - PowerShell integration for Windows PATH modification
  - Duplicate-entry detection
  - Update check during setup

### Changed
- Window title now displays loaded mods (feature was already implemented, now documented)
- Console title on Windows shows: `MDL: <instance> [mod1, mod2, ...]`
- Game window title shows same format via `--title` parameter

## [26.0.0-alpha.4] - 2026-07-27

### Added
- **Mod Management System**
  - `mdl mod list` - List installed mods with metadata
  - `mdl mod install` - Install mods from local JAR files
  - `mdl mod remove` - Uninstall mods
  - `mdl mod enable` - Enable disabled mods
  - `mdl mod disable` - Disable mods without removing them
  - Automatic mod metadata extraction from fabric.mod.json and mcmod.info

- **Configuration Management System**
  - `mdl config get` - Read individual options from options.txt
  - `mdl config set` - Write configuration options
  - `mdl config export` - Export full configuration as JSON
  - `mdl config import` - Import configuration from JSON
  - Support for backup and restore of configuration files

- **World Backup Management**
  - `mdl backup create` - Create compressed ZIP backups of world saves
  - `mdl backup list` - List all backups with metadata
  - `mdl backup restore` - Restore backups with automatic pre-restore backup
  - `mdl backup delete` - Delete backups
  - Async ZIP compression with progress tracking

### Changed
- Improved instance status tracking with last-played timestamps
- Enhanced JSON output format for machine parsing

## [26.0.0-alpha.3] - 2026-07-20

### Fixed
- NeoForge 21.4+ duplicate module error with mixin_synthetic
- Fixed `-DignoreList` processing to ensure "neoforge-" prefix

### Changed
- Post-process NeoForge `-DignoreList` to keep patched/universal JARs on classpath

## [26.0.0-alpha.2] - 2026-07-15

### Added
- Java auto-provisioning system
  - Automatic JRE download from Adoptium API
  - Version-specific Java installation (Java 8, 17, 21)
  - Cache → System → Download fallback chain
- On-demand asset download during launch
- GitHub update checker with 6-hour cache throttle
- NeoForge installer delegation for deobfuscation pipeline

### Changed
- NeoForge installation now delegates to official installer
- Asset downloads moved from instance creation to launch time
- Improved error messages for missing Java versions

## [26.0.0-alpha.1] - 2026-07-10

### Added
- Multi-loader support: Vanilla, Forge, NeoForge, Fabric, Quilt, OptiFine
- Instance creation and management
- Launch functionality with full library support
- Automatic crash report collection and log analysis
- Agent API server with HTTP REST and WebSocket event streaming
- Diagnostic tools for error detection
- JSON-structured output for automation

### Technical
- Rust 1.95.0 implementation
- Tokio async runtime
- Axum HTTP server
- Comprehensive error handling with anyhow
