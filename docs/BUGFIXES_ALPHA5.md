# Bug Fixes and New Features - Alpha 5

## Critical Bug Fixes

### 1. NeoForge "Missing main class net.minecraft.client.main.Main" Error

**Problem**: NeoForge instances would occasionally fail to start with the fatal error:
```
Fatal Startup Error
Failed to create entrypoint object.

Technical Details:
net.neoforged.fml.startup.FatalStartupException: Missing main class 
net.minecraft.client.main.Main from the game content loader 
(but available on jdk.internal.loader.ClassLoaders$AppClassLoader)
```

**Root Cause**: 
- NeoForge uses a deobfuscated + binary-patched client JAR produced by the official installer
- MDL was skipping the vanilla client JAR for NeoForge instances (line 476-478 in launcher.rs)
- If the patched client JAR was missing or corrupted, Minecraft core classes were unavailable
- This caused class loading failures, especially with multiple mods

**Solution**:
- Added fallback logic in `build_classpath()` function
- When NeoForge patched client is not found, automatically add vanilla client JAR to classpath
- Ensures `net.minecraft.client.main.Main` and other core classes are always available
- Logs warning when fallback is triggered for debugging

**Code Changes** (`src/instance/launcher.rs` lines 514-524):
```rust
if patched_client.exists() {
    tracing::debug!("Adding NeoForge patched client: {:?}", patched_client);
    deferred_jars.push(patched_client.display().to_string());
} else {
    tracing::warn!("NeoForge patched client not found: {:?}", patched_client);
    // CRITICAL FIX: fallback to vanilla client JAR
    tracing::warn!("Falling back to vanilla client JAR as emergency classpath entry");
    classpath_entries.insert(0, client_jar.display().to_string());
}
```

**Testing**:
- Tested with NeoForge 21.1.1 on Minecraft 1.21.1
- Verified startup with and without mods
- Confirmed fallback logic triggers correctly when patched client is missing

---

## New Features

### 2. Self-Update System

**Commands**:
- `mdl update --check` - Check for new versions without installing
- `mdl update` - Interactive update with automatic backup

**Features**:
- GitHub API integration for release checking
- Semantic version comparison (supports alpha/beta tags)
- Automatic binary download from GitHub releases
- Backup creation (`.exe.bak`) before update
- Windows update script generation for safe replacement after exit
- Network failure handling with manual download fallback

**Implementation** (`src/util/selfupdate.rs`):
- `check_for_update()` - Queries GitHub API for latest release
- `version_compare()` - Semantic version comparison
- `perform_update()` - Downloads and stages new binary
- Creates batch script for post-exit replacement on Windows

**Usage Example**:
```bash
# Check for updates
mdl update --check

# Install update interactively
mdl update
```

---

### 3. Environment Variable Registration

**Command**: `mdl setup`

**Features**:
- Automatically adds MDL to system PATH (Windows)
- PowerShell integration for environment variable modification
- Duplicate-entry detection (won't add twice)
- Update check during setup
- Cross-platform awareness (notifies on non-Windows)

**Implementation** (`src/util/selfupdate.rs`):
- `add_to_path()` - Modifies user PATH via PowerShell
- `is_in_path()` - Checks if directory already in PATH
- Windows-specific using `[Environment]::SetEnvironmentVariable()`

**Usage Example**:
```bash
# One-time setup
mdl setup

# Restart terminal, then mdl is available globally
mdl --version
```

---

### 4. Window Title Mod Display (Existing Feature)

**Feature**: Console and game window titles show loaded mods

**Format**: `MDL: <instance> [mod1, mod2, ...]`

**Where It Appears**:
- Terminal/console window title (Windows only)
- Minecraft game window title (all platforms, via `--title` parameter)

**Implementation** (`src/instance/launcher.rs` lines 369-384):
- `enumerate_mods()` - Scans `mods/` directory for JAR files
- `display_mod_list()` - Logs mod list and sets console title
- Window title built from instance name + comma-separated mod names
- Helps identify test sessions at a glance

**Example Output**:
```
Loaded 2 mod(s) in 'bc-test':
  - Fabric API (0.119.4+1.21.4)
  - MinecraftBC (2.0.0)

[Window Title]: MDL: bc-test [Fabric API, MinecraftBC]
```

---

## Technical Details

### Changes Summary
- **Files Modified**: 4
- **Lines Added**: 274
- **New Module**: `src/util/selfupdate.rs` (192 lines)

### Dependencies
No new dependencies added - uses existing:
- `reqwest` for HTTP requests
- `tokio::fs` for async file operations
- `serde_json` for GitHub API parsing

### Testing
- All 16 existing tests pass
- Compilation successful with only unused import warnings (non-critical)
- Manual testing:
  - NeoForge instance startup (verified fallback)
  - Update check (validated version comparison)
  - Setup command (confirmed PATH modification)
  - Mod display (verified window titles)

---

## Migration Notes

### For Users
- No breaking changes
- Existing instances work without modification
- New commands are optional enhancements
- Run `mdl setup` once for PATH convenience

### For Developers
- NeoForge classpath logic is more robust
- Fallback behavior is logged at WARN level
- Self-update system can be disabled by not publishing releases

---

## Future Enhancements

### Planned for Alpha 6
- Plugin system for custom mod loaders
- Java arguments customization per-instance
- Performance optimization for large mod packs
- Cross-platform binary distribution

### Under Consideration
- Automatic mod updates from Modrinth/CurseForge
- Mod compatibility checking
- Instance cloning/templating
- TUI interface for interactive management
