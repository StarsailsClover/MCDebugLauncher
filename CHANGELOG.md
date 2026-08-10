# Changelog

All notable changes to MCDebugLauncher will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [26.0.0-alpha.10] - 2026-08-11

### Added (Alpha 10: Aprism product matrix)
- **Aprism JE Native loader**: the `--aprism` launch flag now actually attaches
  the Aprism javaagent (it was defined but never wired). Downloads
  `Aprism-<tag>-JE-<mc>.jar` from GitHub Releases (stable-first, pre-release
  opt-in) and mounts
  `-javaagent:...=aprismVersion=...;mcEdit=JE;mcVersion=...;gameRoot=...`.
- **AprismRefract support** (`mdl aprism refract install|list`): detects and
  installs loader-support `.aep` extensions
  (`<Loader>-Support-A<range>-<Key><range>-JE-<mc>.aep`) into the instance's
  `aprism-extensions/` directory so the Aprism loader can run Fabric/Forge/
  NeoForge/Quilt/LiteLoader mods.
- **AprismPrismate support** (`mdl aprism prismate install|status`): installs
  the loader-side bridge `AprismPrismate-v<ver>-<Fa|N|Fo>-<mc>.jar` into
  `mods/`, letting Fabric/NeoForge/Forge load Aprism-native `.aje` packs.
  Refuses the conflicting `--aprism` + Prismate combination.

### Fixed
- **Despotes for vanilla** (reported bug): vanilla/none instances now map to
  the Despotes `native` branch instead of failing with "no loader none". The
  native build attaches as a JVM `-javaagent` (installed at the instance root,
  not mods/); the launcher mounts it automatically in `--agent` mode.
- Despotes asset parsing now accepts the Aprism variant's `.aje` suffix.
- Corrected a mangled mutual-exclusion error message in the Aprism launcher path.

### Tests
- 67 unit tests + 3 opt-in network integration tests (Despotes fabric/native,
  Aprism javaagent download). All green.

## [26.0.0-alpha.8.1] - 2026-08-10

### Added
- **Modrinth modpack import** — `mdl import <name> <pack.mrpack>` parses `modrinth.index.json`, creates the instance with the pack's exact Minecraft version and loader, copies `overrides/` into the instance, then auto-completes every indexed file (sha1-verified download, skipped when already intact — fully idempotent). Zip-slip paths are rejected. `--no-download` imports structure only.
- **Java Edition dedicated servers** — `mdl server create|list|launch|stop|status`: downloads the official server.jar (sha1-verified) from the version manifest, writes `eula.txt` and a default `server.properties`, launches detached with PID tracking, background log at `<server>/server.log`, and stops via taskkill. Supports JSON output for agents.
- **Startup update digest** — every MDL run now prints the four most recent version digests (parsed from the CHANGELOG embedded at build time) so returning users see what changed; suppressed under `--format json` to keep machine-readable output clean.

### Fixed
- **Detached spawn pipe-handle leak (UX)** — on Windows the launcher's stdout/stderr handles were inherited by detached children (game and server processes), keeping the caller's pipe open so `mdl launch --detach` / `mdl server launch` appeared to hang until the game exited. The launcher now clears the inherit flags and creates detached children with `DETACHED_PROCESS`, so both commands return immediately. This also applies to the existing client `--detach` path.

### Changed
- Fabric instances additionally benefit from the launch-time Fabric API repair introduced in Alpha 8 (verified working end-to-end in 8.1).
- README rewritten to reflect Alpha 5–8.1 capabilities (modpack import, servers, agent control, mirrors, accounts).

## [26.0.0-alpha.8] - 2026-08-09

### Added
- **Mirror sources with live probing** — built-in Chinese mirror (BMCLAPI) + official; probes are ranked by latency and cached 10 min; every download prefers the best mirror and falls back down the list (flexible source switching).
- **Chunked parallel downloads** — large artifacts split into 4 parallel HTTP Range chunks when the server supports it; single-shot otherwise; auto-fallback across sources on failure.
- **Download cache (7-day copy-install)** — downloaded versions/mods/etc. are stored once and instances install a *copy*; entries unused for 7 days are evicted (`mdl cache info` / `mdl cache clean`).
- **Content search & install** — `mdl search mod|resourcepack|shader <query>` via Modrinth with numbered selection and per-instance install into mods/ / resourcepacks/ / shaderpacks/.
- **Microsoft account login** — `mdl account login` (OAuth Device Code flow, headless-friendly); `mdl account list`; `mdl account skin` to download the skin PNG / print avatar URL.
- **Test worlds & auto-entry** — `mdl create --with-test-world` marks the instance; `mdl launch --enter-test-world --wait-ready` enters (or creates) the test world via Despotes once the game broadcasts ready.
- **JDK customization & dynamic memory/perf** — `--java-path` override; `--memory` explicit or dynamic (half of system RAM, capped 8G); GC chosen by allocation tier (G1 for >=4G).
- **Aprism JE Native support** — `mdl launch --aprism` detects the applicable Aprism artifact (stable-first / pre-release fallback), downloads+ caches it and attaches `-javaagent:...=aprismVersion=...;mcEdit=JE;mcVersion=...;gameRoot=...`.
- **Minecraft BE (BDS) support** — `mdl bedrock install|launch` downloads and runs the official Bedrock Dedicated Server for Windows (version probing against the stable link pattern).
- **dll/exe injector** — `mdl inject <pid|name> --dll <path>` (CreateRemoteThread/LoadLibraryW), groundwork for Aprism BE Native.
- **Logging** — persistent `<data>/logs/mdl.log` (tee stdout+file), `--log-file` override, and `--lang zh` Chinese launcher messages.

### Added (Alpha 8)
- **Game-ready broadcast** — the agent server polls Despotes after launch and emits a `game_ready` WebSocket/JSON event when the game finishes booting; `GET /api/v1/game/:instance/ready` reports readiness; `mdl launch --wait-ready` blocks until ready.

### Technical Notes
- Mirror URL mapping follows the OpenBMCLAPI convention (root/maven/assets).
- BDS Windows client is UWP-locked; client-side BE support is limited to injection (injector) pending Aprism BE.
- Microsoft device flow uses the public PrismLauncher client id; re-login required on token expiry (offline_access simplification).

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
