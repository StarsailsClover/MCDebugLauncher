# Changelog

All notable changes to MCDebugLauncher will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [26.0.0-alpha.7] - 2026-08-09

### Added
- **Despotes integration (Alpha 7, first slice)** — MDL now detects, downloads and installs the [Despotes](https://github.com/NDBlockConnect/Despotes) control mod, replacing the old bundled companion:
  - `Latest Release` detection with Pre-Release policy: stable releases are preferred; Pre-Releases require explicit confirmation; when no applicable stable exists, the newest applicable Pre-Release is the fallback candidate.
  - Asset matching by loader + Minecraft version, covering the v26.0 compatibility matrix (fabric 1.20-1.21.11 / 26.x, etc.).
  - `mdl create` now lists applicable Despotes packages with numbered selection; non-interactive sessions auto-select the newest stable (pre-releases still need explicit opt-in). New flags: `--no-despotes`, `--despotes-prerelease`.
  - Downloads are sha256-verified and cached in `<data>/despotes/`; instances install a copy.
- **Mod Menu auto-install** — Fabric instances now automatically install [ModMenu](https://modrinth.com/mod/modmenu) from Modrinth during creation (best-effort).
- **Dual-channel screenshots** — `mdl game screenshot` prefers the Despotes in-game framebuffer (works when minimized) and falls back to Windows.Graphics.Capture.

### Changed
- Game control runtime migrated from the MDL TCP companion to the Despotes HTTP protocol (`/despotes/v1/actions`, `/query`, `/screenshot`). `mdl game ...` and the agent API endpoints are unchanged at the CLI/HTTP surface.
- Agent launch now passes `-Ddespotes.port` and records `runtime/despotes.port`; the old `mdl.agent.port`/`agent.port` mechanism is removed.

### Removed
- The bundled `mdl-agent-companion` source tree and local-JAR install path. Use Despotes instead.

### Technical Notes
- Despotes releases are fetched from the GitHub Releases API (`NDBlockConnect/Despotes`); asset names follow `Despotes-<tag>-<loader>-<mc>.jar`.
- Remaining Alpha 7 roadmap (mirrors, chunked downloads, mod/resource/shader search, auth, BE, Aprism, cache eviction, etc.) continues in subsequent pre-releases.

## [26.0.0-alpha.6] - 2026-08-08

### Added
- **Agent Game Control (highlight)** — the agent can now observe and operate a running Minecraft instance without touching the user's keyboard/mouse or stealing window focus:
  - **High-performance screenshots** via Windows.Graphics.Capture: GPU-accelerated per-window capture that works while the game is unfocused, occluded, or in the background (764 ms round-trip in testing). New `mdl game screenshot <instance>` and `GET /api/v1/game/:instance/screenshot` (raw PNG or `?base64=true` JSON).
  - **In-game input injection** via the new `mdl-agent-companion` Fabric mod (bundled with MDL, auto-installed on `--agent` launch): keys (`mdl game key`), view rotation (`mdl game look`), mouse/GUI clicks (`mdl game click`), hotbar scroll (`mdl game scroll`), chat/commands (`mdl game chat`) — plus `POST /api/v1/game/:instance/input`. Verified end-to-end: menu navigation, world entry, movement and look all work while the user's focus stays in other apps.
  - **Game state queries** (`mdl game status`, `GET /api/v1/game/:instance/status`): in-world flag, pause state, screen, player position/rotation.
  - **No-pause-while-multitasking**: agent launches automatically set `pauseOnLostFocus:false` and pass `mdl.agent.keepFocus=true`, so the game never shows the pause menu when the user focuses another application.
  - New `mdl game windows` and `GET /api/v1/game/windows` to list MDL game windows visible for capture.
- **Detached launching** — `mdl launch <instance> --detach` now actually works: returns immediately with the real PID, game output goes to `logs/launch_detached.log`. The launch lock is transferred to the game process so the single-instance guarantee still holds.
- `--agent` / `--agent-port` launch options.

### Changed
- Agent API `launch` command is no longer blocking: it launches detached and returns the real PID immediately (previously the HTTP request hung until the game exited and reported PID 0). The server now tracks running instances and emits `instance_stopped` events.
- Game window discovery is PID-first (titles are unreliable — some clients ignore `--title`), with title-prefix matching as fallback.

### Fixed
- CLI commands could hang for minutes at exit on machines where the background GitHub update check hits an unresponsive network path; the check is now hard-capped and the process exits explicitly.
- HTTP client gained a connect timeout so slow networks fail fast instead of blocking.

### Technical Notes
- Companion mod protocol v1 over a local TCP socket (JSON lines, 127.0.0.1 only, default port 25590). The bound port is reported via `runtime/agent.port`.
- The companion is shipped as `mdl-agent-companion-1.0.0.jar` alongside the launcher and is installed into Fabric/Quilt instances at agent launch.
- Input works without focus because the companion injects through Minecraft's own keybinding/screen systems on the client thread, and the keepFocus Mixin defeats the game's internal focus gating.

## [26.0.0-alpha.5.1] - 2026-08-01

### Fixed
- **Critical**: NeoForge version selection now correctly prioritizes stable releases over pre-releases
  - Fixed `fetch_versions()` to use semantic version sorting instead of alphabetical
  - Instance creation with `--loader-version latest` now selects highest stable version (e.g., 21.10.64 instead of 21.10.0-beta)
  - Added version mismatch detection at launch time with clear error messages
  - Prevents cryptic "Missing main class" errors caused by version mismatches

- **Critical**: NeoForge patched client path correction
  - Fixed launcher to look for patched client at correct Maven path: `net/neoforged/minecraft-client-patched/<version>/`
  - Previous path (`net/neoforged/neoforge/<version>/neoforge-<version>-client.jar`) was incorrect
  - Eliminates "NeoForge patched client not found" warnings during launch
  - Ensures deobfuscated Minecraft classes are available for mod loading

### Changed
- Code quality improvements: removed 24 unused import warnings
- Reduced compiler warnings from 58 to 34

### Technical Notes
Alpha 5's NeoForge fixes were incomplete. This release addresses the root causes:
1. Version selection was alphabetical, causing beta versions to be selected over stable releases
2. Patched client lookup used wrong Maven coordinates from pre-installer implementation
Users experiencing NeoForge 21.10+ launch failures should recreate instances with this version.

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
