# MCDebugLauncher Technical Specifications

## Overview

This document provides detailed technical specifications for the MCDebugLauncher project, including API endpoints, data structures, command-line interfaces, and implementation requirements.

## Command-Line Interface

### Global Options

All commands support the following global options:

```bash
mdl [GLOBAL_OPTIONS] <COMMAND> [ARGS]

Global Options:
  --config <PATH>        Path to configuration file (default: ~/.mdl/config.toml)
  --data-dir <PATH>      Data directory for instances (default: ~/.mdl)
  --format <FORMAT>      Output format: text|json|yaml (default: text)
  --verbose, -v          Increase logging verbosity (can be repeated)
  --quiet, -q            Suppress non-error output
  --no-color             Disable colored output
  --help, -h             Display help information
  --version, -V          Display version information
```

### Commands

#### 1. Version Management

##### `mdl versions`
List available Minecraft versions.

```bash
mdl versions [OPTIONS]

Options:
  --type <TYPE>          Filter by version type: release|snapshot|all (default: release)
  --loader <LOADER>      Show versions compatible with loader: fabric|forge|neoforge|quilt
  --limit <N>            Limit output to N versions (default: 20)
  --search <PATTERN>     Search pattern (e.g., "1.20", "1.19.2")
  --format json          Output as JSON array

Output (text):
  1.21.4    release    2026-12-05
  1.21.3    release    2026-11-18
  1.21.2    release    2026-10-22

Output (json):
  {
    "versions": [
      {
        "id": "1.21.4",
        "type": "release",
        "release_time": "2026-12-05T10:41:37+00:00",
        "url": "https://..."
      }
    ]
  }
```

##### `mdl version info`
Get detailed information about a specific version.

```bash
mdl version info <VERSION> [OPTIONS]

Arguments:
  <VERSION>              Version identifier (e.g., "1.21.4", "release", "snapshot")

Options:
  --show-libraries       Include library list
  --show-assets          Include asset information

Output (json):
  {
    "id": "1.21.4",
    "type": "release",
    "release_time": "2026-12-05T10:41:37+00:00",
    "java_version": {
      "component": "java-runtime-delta",
      "major_version": 21
    },
    "downloads": {
      "client": {
        "url": "https://...",
        "sha1": "...",
        "size": 25165824
      },
      "server": { ... }
    }
  }
```

#### 2. Instance Management

##### `mdl instance create`
Create a new Minecraft instance.

```bash
mdl instance create <NAME> [OPTIONS]

Arguments:
  <NAME>                 Instance name (alphanumeric, hyphens, underscores)

Options:
  --version <VERSION>    Minecraft version (default: latest release)
  --loader <LOADER>      Mod loader: fabric|forge|neoforge|quilt|optifine|none
  --loader-version <V>   Specific loader version (default: latest compatible)
  --java <PATH>          Path to Java executable (default: auto-detect)
  --memory <SIZE>        Memory allocation (e.g., "4G", "2048M")
  --jvm-args <ARGS>      Additional JVM arguments
  --game-dir <PATH>      Game directory (default: <data-dir>/instances/<name>)
  --no-install           Create instance without downloading files

Examples:
  mdl instance create vanilla --version 1.21.4
  mdl instance create modded --version 1.20.1 --loader fabric
  mdl instance create forge-test --version 1.19.2 --loader forge --memory 6G
  mdl instance create optifine --version 1.21.4 --loader optifine
```

##### `mdl instance list`
List all instances.

```bash
mdl instance list [OPTIONS]

Options:
  --format json          Output as JSON

Output (text):
  NAME           VERSION    LOADER      STATUS
  vanilla        1.21.4     none        ready
  modded         1.20.1     fabric      ready
  forge-test     1.19.2     forge       incomplete

Output (json):
  {
    "instances": [
      {
        "name": "vanilla",
        "version": "1.21.4",
        "loader": "none",
        "status": "ready",
        "game_dir": "/home/user/.mdl/instances/vanilla",
        "last_played": "2026-07-22T10:30:00Z"
      }
    ]
  }
```

##### `mdl instance info`
Display detailed information about an instance.

```bash
mdl instance info <NAME> [OPTIONS]

Arguments:
  <NAME>                 Instance name

Options:
  --show-mods            Include installed mods
  --show-config          Include configuration

Output (json):
  {
    "name": "modded",
    "version": "1.20.1",
    "loader": {
      "type": "fabric",
      "version": "0.15.11"
    },
    "java": {
      "path": "/usr/lib/jvm/java-17-openjdk/bin/java",
      "version": "17.0.8"
    },
    "memory": {
      "min": "1G",
      "max": "4G"
    },
    "game_dir": "/home/user/.mdl/instances/modded",
    "mods": [
      {
        "file": "fabric-api-0.92.0.jar",
        "name": "Fabric API",
        "version": "0.92.0"
      }
    ]
  }
```

##### `mdl instance delete`
Delete an instance.

```bash
mdl instance delete <NAME> [OPTIONS]

Arguments:
  <NAME>                 Instance name

Options:
  --keep-saves           Keep world saves
  --force, -f            Skip confirmation prompt
```

##### `mdl instance export`
Export an instance as a portable archive.

```bash
mdl instance export <NAME> <OUTPUT> [OPTIONS]

Arguments:
  <NAME>                 Instance name
  <OUTPUT>               Output file path (.zip or .tar.gz)

Options:
  --include-saves        Include world saves
  --include-logs         Include log files
  --include-screenshots  Include screenshots
  --format <FORMAT>      Archive format: zip|tar.gz|modrinth (default: zip)

Example:
  mdl instance export modded backup.zip --include-saves
```

##### `mdl instance import`
Import an instance from an archive.

```bash
mdl instance import <ARCHIVE> <NAME> [OPTIONS]

Arguments:
  <ARCHIVE>              Archive file path
  <NAME>                 New instance name

Options:
  --force                Overwrite existing instance

Example:
  mdl instance import backup.zip modded-restored
```

#### 3. Launch Operations

##### `mdl launch`
Launch a Minecraft instance.

```bash
mdl launch <NAME> [OPTIONS]

Arguments:
  <NAME>                 Instance name

Options:
  --username <NAME>      Offline username (for offline mode)
  --account <ID>         Microsoft account ID (for online mode)
  --server <ADDRESS>     Auto-connect to server (format: host:port)
  --fullscreen           Launch in fullscreen mode
  --width <N>            Window width
  --height <N>           Window height
  --demo                 Launch in demo mode
  --log-format <FORMAT>  Log output format: text|json (default: text)
  --log-file <PATH>      Write logs to file
  --detach               Run game in background
  --dry-run              Show launch command without executing

Examples:
  mdl launch vanilla
  mdl launch modded --username TestPlayer
  mdl launch forge-test --server mc.hypixel.net:25565
  mdl launch vanilla --log-format json --log-file session.jsonl
```

##### `mdl kill`
Terminate a running instance.

```bash
mdl kill <NAME> [OPTIONS]

Arguments:
  <NAME>                 Instance name

Options:
  --force, -f            Force kill (SIGKILL)
  --timeout <SECONDS>    Wait timeout before force kill (default: 30)
```

#### 4. Mod Management

##### `mdl mod install`
Install a mod into an instance.

```bash
mdl mod install <INSTANCE> <MOD> [OPTIONS]

Arguments:
  <INSTANCE>             Instance name
  <MOD>                  Mod identifier or file path

Options:
  --source <SOURCE>      Mod source: modrinth|curseforge|file|url (default: auto-detect)
  --version <VERSION>    Specific mod version (default: latest compatible)
  --force                Install even if incompatible

Examples:
  mdl mod install modded sodium                    # From Modrinth
  mdl mod install modded fabric-api --version 0.92.0
  mdl mod install modded ./custom-mod.jar --source file
  mdl mod install modded https://example.com/mod.jar --source url
```

##### `mdl mod list`
List installed mods in an instance.

```bash
mdl mod list <INSTANCE> [OPTIONS]

Arguments:
  <INSTANCE>             Instance name

Options:
  --format json          Output as JSON

Output (text):
  NAME                VERSION    FILE
  Fabric API          0.92.0     fabric-api-0.92.0+1.20.1.jar
  Sodium              0.5.3      sodium-fabric-0.5.3+mc1.20.1.jar

Output (json):
  {
    "mods": [
      {
        "name": "Fabric API",
        "version": "0.92.0",
        "file": "fabric-api-0.92.0+1.20.1.jar",
        "path": "/home/user/.mdl/instances/modded/mods/fabric-api-0.92.0+1.20.1.jar"
      }
    ]
  }
```

##### `mdl mod remove`
Remove a mod from an instance.

```bash
mdl mod remove <INSTANCE> <MOD>

Arguments:
  <INSTANCE>             Instance name
  <MOD>                  Mod name or file name

Example:
  mdl mod remove modded sodium
  mdl mod remove modded sodium-fabric-0.5.3+mc1.20.1.jar
```

##### `mdl mod update`
Update mods in an instance.

```bash
mdl mod update <INSTANCE> [MOD] [OPTIONS]

Arguments:
  <INSTANCE>             Instance name
  [MOD]                  Specific mod to update (default: all)

Options:
  --check-only           Only check for updates, don't install
  --format json          Output as JSON

Output (text):
  Fabric API: 0.92.0 -> 0.93.1 (update available)
  Sodium: 0.5.3 (up to date)
```

#### 5. Diagnostics

##### `mdl diagnose`
Analyze instance for errors and collect diagnostic information.

```bash
mdl diagnose <INSTANCE> [OPTIONS]

Arguments:
  <INSTANCE>             Instance name

Options:
  --export <PATH>        Export diagnostics to archive
  --include-saves        Include world data in export
  --analyze              Analyze logs for known issues
  --format json          Output as JSON

Output (text):
  Analyzing instance: modded
  
  Latest Crash: 2026-07-22 10:15:33
    Cause: java.lang.NoClassDefFoundError: net/fabricmc/fabric/api/event/Event
    Suspected Mods: CustomMod (1.0.0)
    Solution: Install Fabric API dependency
  
  Log Errors (5):
    [ERROR] Mixin apply failed: custommod.mixins.json
    [WARN] Missing dependency: fabric-api

Output (json):
  {
    "instance": "modded",
    "timestamp": "2026-07-22T10:30:00Z",
    "crashes": [
      {
        "timestamp": "2026-07-22T10:15:33Z",
        "exception": "java.lang.NoClassDefFoundError",
        "message": "net/fabricmc/fabric/api/event/Event",
        "suspected_mods": ["CustomMod:1.0.0"],
        "file": "crash-2026-07-22_10.15.33-client.txt"
      }
    ],
    "errors": [ ... ],
    "warnings": [ ... ]
  }
```

##### `mdl logs`
View instance logs.

```bash
mdl logs <INSTANCE> [OPTIONS]

Arguments:
  <INSTANCE>             Instance name

Options:
  --file <FILE>          Specific log file (default: latest.log)
  --follow, -f           Follow log output (tail -f)
  --lines <N>            Number of lines to show (default: 100)
  --level <LEVEL>        Filter by log level: debug|info|warn|error
  --grep <PATTERN>       Filter by pattern

Examples:
  mdl logs modded
  mdl logs modded --follow
  mdl logs modded --level error
  mdl logs modded --grep "Exception"
```

##### `mdl crashes`
List crash reports for an instance.

```bash
mdl crashes <INSTANCE> [OPTIONS]

Arguments:
  <INSTANCE>             Instance name

Options:
  --limit <N>            Number of reports to show (default: 10)
  --format json          Output as JSON
  --export <PATH>        Export all crash reports to directory

Output (text):
  TIMESTAMP            EXCEPTION                          SUSPECTED MODS
  2026-07-22 10:15:33  NoClassDefFoundError               CustomMod
  2026-07-21 15:42:11  NullPointerException               Sodium, Lithium
```

#### 6. Configuration

##### `mdl config`
Manage global configuration.

```bash
mdl config <COMMAND> [ARGS]

Commands:
  get <KEY>              Get configuration value
  set <KEY> <VALUE>      Set configuration value
  list                   List all configuration
  reset [KEY]            Reset configuration (or specific key)

Examples:
  mdl config get java.default_path
  mdl config set java.default_path /usr/lib/jvm/java-17/bin/java
  mdl config set memory.default "4G"
  mdl config list --format json
```

## Configuration File Format

### Global Configuration (`~/.mdl/config.toml`)

```toml
[general]
data_dir = "/home/user/.mdl"
default_format = "text"
color = true

[java]
auto_install = true
default_path = "/usr/lib/jvm/java-17/bin/java"

[memory]
default_min = "1G"
default_max = "4G"

[network]
timeout = 30
max_concurrent_downloads = 6
mirror = "default"  # or "bmclapi", "mcbbs"

[logging]
level = "info"
file = "/home/user/.mdl/mdl.log"

[agent]
enable = false
port = 25580
auth_token = ""
```

### Instance Configuration (`<instance>/instance.toml`)

```toml
[instance]
name = "modded"
version = "1.20.1"
created = "2026-07-22T10:00:00Z"
last_played = "2026-07-22T10:30:00Z"

[loader]
type = "fabric"
version = "0.15.11"

[java]
path = "/usr/lib/jvm/java-17/bin/java"
version = "17.0.8"

[memory]
min = "1G"
max = "4G"

[jvm_args]
additional = ["-XX:+UseG1GC", "-Dsun.rmi.dgc.server.gcInterval=2147483646"]

[game_args]
additional = []

[window]
width = 1920
height = 1080
fullscreen = false
```

## JSON API Specification

All commands with `--format json` output follow these conventions:

### Success Response
```json
{
  "status": "success",
  "data": { ... },
  "timestamp": "2026-07-22T10:30:00Z"
}
```

### Error Response
```json
{
  "status": "error",
  "error": {
    "code": "INSTANCE_NOT_FOUND",
    "message": "Instance 'test' does not exist",
    "details": { ... }
  },
  "timestamp": "2026-07-22T10:30:00Z"
}
```

### Exit Codes

```
0   - Success
1   - General error
2   - Invalid arguments
3   - File/directory not found
4   - Permission denied
5   - Network error
6   - Download failure
7   - Installation failure
8   - Launch failure
9   - Instance already exists
10  - Instance not found
11  - Version not found
12  - Incompatible version
13  - Java not found
14  - Configuration error
15  - Diagnostic error
```

## Agent Interface

### HTTP/JSON-RPC Server

When agent mode is enabled, MDL starts an HTTP server for programmatic control.

#### Starting the Server

```bash
mdl agent start [OPTIONS]

Options:
  --port <PORT>          Server port (default: 25580)
  --bind <ADDRESS>       Bind address (default: 127.0.0.1)
  --auth-token <TOKEN>   Authentication token (required for non-localhost)
```

#### API Endpoints

##### POST `/api/v1/execute`
Execute a command.

Request:
```json
{
  "command": "instance",
  "args": ["list"],
  "options": {
    "format": "json"
  }
}
```

Response:
```json
{
  "status": "success",
  "exit_code": 0,
  "stdout": "...",
  "data": { ... }
}
```

##### GET `/api/v1/status`
Get server status.

Response:
```json
{
  "version": "0.1.0",
  "uptime": 3600,
  "active_instances": ["modded"],
  "running_instances": {
    "modded": {
      "pid": 12345,
      "started": "2026-07-22T10:00:00Z"
    }
  }
}
```

##### WebSocket `/api/v1/events`
Subscribe to real-time events via WebSocket.

**Connection:**
```
ws://localhost:8080/api/v1/events
```

**Event Format:**
All events are JSON objects with a `type` field and a `timestamp` field.

**Launch Events:**
```json
{
  "type": "launch_started",
  "instance": "my-instance",
  "timestamp": "2026-07-22T10:00:00Z"
}

{
  "type": "launch_progress",
  "instance": "my-instance",
  "stage": "downloading_libraries",
  "progress": 0.45,
  "message": "Downloaded 23/51 libraries",
  "timestamp": "2026-07-22T10:00:15Z"
}

{
  "type": "launch_completed",
  "instance": "my-instance",
  "pid": 12345,
  "timestamp": "2026-07-22T10:00:30Z"
}

{
  "type": "launch_failed",
  "instance": "my-instance",
  "error": "Failed to download library: connection timeout",
  "timestamp": "2026-07-22T10:00:20Z"
}
```

**Log Events:**
```json
{
  "type": "log_line",
  "instance": "my-instance",
  "level": "info",
  "message": "[Render thread/INFO]: Setting user: Player123",
  "timestamp": "2026-07-22T10:01:00Z"
}
```

**Instance Events:**
```json
{
  "type": "instance_stopped",
  "instance": "my-instance",
  "exit_code": 0,
  "timestamp": "2026-07-22T11:00:00Z"
}
```

**Client Example (Python):**
```python
import asyncio
import websockets
import json

async def listen():
    async with websockets.connect("ws://localhost:8080/api/v1/events") as ws:
        async for message in ws:
            event = json.loads(message)
            print(f"[{event['type']}] {event.get('message', '')}")

asyncio.run(listen())
```

## Implementation Requirements

### Performance
- Download speeds: Support parallel downloads (default: 6 concurrent)
- Startup time: <2 seconds for CLI initialization
- Instance creation: <30 seconds for vanilla, <2 minutes with mod loaders

### Error Handling
- All errors must include actionable messages
- Network errors: Automatic retry with exponential backoff (3 attempts)
- File system errors: Check permissions and disk space before operations
- Validation: Pre-flight checks before destructive operations

### Security
- SHA1 verification for all downloads
- HTTPS only for API requests
- No execution of untrusted code
- Token-based authentication for agent API

### Testing
- Unit tests for all core components
- Integration tests for installation workflows
- End-to-end tests for launch operations
- Mock API responses for network tests

### Logging
- Structured logging with levels: trace, debug, info, warn, error
- Rotation: 10MB max size, 5 files kept
- JSON format option for machine parsing
- Sensitive data redaction (tokens, passwords)

This specification is subject to revision during implementation.
