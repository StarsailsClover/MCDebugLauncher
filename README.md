# MCDebugLauncher

A command-line Minecraft launcher designed for rapid testing, mod development, and AI agent automation. Supports all major mod loaders (Forge, NeoForge, Fabric, Quilt, OptiFine), comprehensive error diagnostics, and structured output for programmatic control.

[中文文档](README_CN.md)

## Features

- **One-Command Launch**: Install and launch any Minecraft version with any mod loader in a single command
- **Multi-Loader Support**: Vanilla, Forge, NeoForge, Fabric, Quilt, LegacyFabric, OptiFine
- **Intelligent Diagnostics**: Automatic crash report collection, log analysis, and error detection
- **Agent-Friendly**: JSON-structured output for AI agents and automation tools
- **Developer Tools**: Debug logging, performance profiling, headless mode support
- **Instance Isolation**: Independent instances with separate mods, configs, and Java versions
- **Enterprise-Grade**: Production-ready code quality with comprehensive error handling

## Quick Start

```bash
# Create and launch a Fabric instance
mdl create my-instance --mc-version 1.21.1 --loader fabric
mdl launch my-instance

# List all instances
mdl list --format json

# View logs and diagnostics
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

**Current Phase**: Phase 5 - Agent API (In Progress)

- ✅ Phase 1: CLI framework and version management
- ✅ Phase 2: Instance management and Fabric loader
- ✅ Phase 3: Launch functionality with full library support
- ✅ Phase 4: Diagnostics and log analysis
- 🔄 Phase 5: Agent API server with WebSocket events
- ⏳ Phase 6: Additional loaders (Forge, NeoForge, Quilt, OptiFine)

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
