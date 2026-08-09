# MCDebugLauncher

A command-line Minecraft launcher designed for rapid testing, mod development, and AI agent automation. Supports all major mod loaders (Forge, NeoForge, Fabric, Quilt, OptiFine), comprehensive error diagnostics, and structured output for programmatic control.

[中文文档](README_CN.md)

## Features

- **One-Command Launch**: Install and launch any Minecraft version with any mod loader in a single command
- **Multi-Loader Support**: Vanilla, Forge, NeoForge, Fabric, Quilt, LegacyFabric, OptiFine, Aprism JE Native
- **Agent Game Control (Despotes)**: Observe and operate a running game without stealing focus — GPU screenshots (Windows.Graphics.Capture + in-game framebuffer), input injection (keys/mouse/look/chat), status queries; the game keeps running while you focus other apps (`pauseOnLostFocus` handled automatically)
- **Modrinth Modpack Import**: `mdl import` creates the instance with the pack's exact version/loader, copies overrides and auto-completes every missing file (sha1-verified, idempotent)
- **Java Edition Dedicated Servers**: `mdl server create/launch/stop` downloads the official server.jar, manages eula/properties and runs servers in the background
- **Mirrors & Resilient Downloads**: built-in official + Chinese mirror sources with live latency probing, chunked (HTTP Range) parallel downloads, automatic source switching, sha1-verified 7-day copy-install cache
- **Content Search & Install**: search and install mods / resource packs / shaders from Modrinth with one command
- **Microsoft Accounts**: device-code login (headless-friendly), account list, skin download
- **Integrity & Self-Repair**: pre-launch verification of client JAR, libraries and assets with automatic re-download of corrupted files; Fabric API auto-install at launch
- **Intelligent Diagnostics**: automatic crash report collection, log analysis, and error detection
- **Agent-Friendly**: JSON-structured output, HTTP/WebSocket agent server with launch progress and game-ready events
- **Instance Management**: mod management, configuration import/export, world backup/restore, optional launch queue (`--no-queue`)
- **Chinese Localization**: `--lang zh` messages and UTF-8 console output on Windows

## Quick Start

```bash
# Create and launch a Fabric instance (Fabric API + ModMenu installed automatically)
mdl create my-instance --mc-version 1.21.1 --loader fabric
mdl launch my-instance

# Import a Modrinth modpack (.mrpack) with auto-completion
mdl import my-pack ./cool-pack.mrpack

# Search and install content
mdl search mod sodium --mc-version 1.21.1 --loader fabric --instance my-instance

# Launch in the background with agent control, wait for the game-ready broadcast
mdl launch my-instance --detach --agent --wait-ready
mdl game status my-instance
mdl game screenshot my-instance --output shot.png

# Java Edition dedicated server
mdl server create my-server --mc-version 1.21.4
mdl server launch my-server
mdl server stop my-server

# Manage mods / backups / diagnostics
mdl mod list my-instance
mdl backup create my-instance world1
mdl logs my-instance --follow
mdl diagnose my-instance --analyze
```

## Agent API

MCDebugLauncher includes a built-in HTTP/WebSocket server for programmatic control by AI agents and automation tools.

```bash
# Start the agent server
mdl agent --port 8080
```

**REST API:**
```bash
# Get server status
curl http://localhost:8080/api/v1/status

# Execute commands
curl -X POST http://localhost:8080/api/v1/execute \
  -H "Content-Type: application/json" \
  -d '{"command":"list","args":[],"options":{}}'
```

**WebSocket Events:**
```python
import asyncio
import websockets

async def listen():
    async with websockets.connect("ws://localhost:8080/api/v1/events") as ws:
        async for message in ws:
            event = json.loads(message)
            print(f"[{event['type']}] {event.get('message', '')}")
```

See [docs/specification.md](docs/specification.md) for complete API documentation.

## Documentation

- [Specification](docs/specification.md) - Complete CLI and API reference
- [Research Document](docs/RESEARCH.md) - Technical analysis and architecture decisions

## Status

**Current Version**: v26.0 Alpha 8.1

- ✅ CLI framework, version management, instance management
- ✅ Forge / NeoForge / Fabric / Quilt / OptiFine loaders
- ✅ Diagnostics, log analysis, agent HTTP/WebSocket server
- ✅ Alpha 5: mod/config/backup management, self-update
- ✅ Alpha 6: agent game control (screenshots, input injection, focus-free play)
- ✅ Alpha 7: Despotes integration, mirrors + chunked downloads + cache, Modrinth search, Microsoft login, Aprism JE, Bedrock Dedicated Server, dll injector
- ✅ Alpha 8: game-ready broadcast, pre-launch integrity verification, Fabric API auto-repair, --no-queue, UTF-8 console fixes
- ✅ Alpha 8.1: Modrinth modpack import (auto-completion), JE dedicated servers, startup update digest, detached-spawn handle-leak fix

**Tested Configurations:**
- Vanilla Minecraft 1.21.x ✅
- Fabric Loader + Fabric API ✅
- Forge 52.x / NeoForge 21.x ✅
- Bedrock Dedicated Server 1.26.x ✅
- JE Dedicated Server 1.21.4 ✅

## Contributing

Contributions are welcome! Please read our contribution guidelines before submitting pull requests.

## Acknowledgments

This project was developed with assistance from AI (Claude). Technical research and implementation guidance were provided through human-AI collaboration.

The following open-source projects served as inspiration and technical references:
- [PrismLauncher](https://github.com/PrismLauncher/PrismLauncher) - Modern Minecraft launcher
- [PortableMC](https://github.com/mindstorm38/portablemc) - CLI launcher design patterns
- [HeadlessMC](https://github.com/headlesshq/headlessmc) - Headless testing infrastructure
- [MC-CLI](https://github.com/Th0rgal/mc-cli) - Agent control interfaces

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.
