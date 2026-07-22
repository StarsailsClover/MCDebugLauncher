# MCDebugLauncher - Research Phase Summary

## Executive Summary

Completed comprehensive research and technical planning for MCDebugLauncher, a command-line Minecraft launcher designed for developers, testers, and AI agents. The project aims to streamline Minecraft mod testing workflows through automation, intelligent diagnostics, and structured output formats.

## Deliverables Completed

### Documentation
1. **RESEARCH.md** (13.7KB) - English technical research document
   - Analysis of Minecraft version management APIs
   - Mod loader installation mechanisms (Fabric, Forge, NeoForge, Quilt, OptiFine)
   - Existing launcher architectures (PrismLauncher, PortableMC, HeadlessMC, MC-CLI)
   - Log collection and crash report analysis patterns
   - Agent-friendly design recommendations

2. **RESEARCH_CN.md** (12.8KB) - Chinese translation
   - Complete translation of technical research
   - Localized for Chinese-speaking developers

3. **SPECIFICATION.md** (15.9KB) - Technical specifications
   - Complete CLI command reference with examples
   - Configuration file formats (TOML)
   - JSON API specification for agent control
   - Exit codes and error handling conventions
   - Performance and security requirements

4. **README.md** & **README_CN.md** - Project overview
   - Feature highlights
   - Quick start examples
   - Status indicators
   - Attribution and acknowledgments

## Key Research Findings

### 1. Minecraft Launcher Ecosystem

**Official APIs (Mojang)**:
- Version manifest: `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`
- Complete metadata for all Minecraft versions
- SHA1 checksums for file integrity verification
- Java version requirements per game version
- Asset and library dependency information

**Mod Loader Installation**:
- Fabric: Lightweight, JSON-based installer with separate API dependency
- Forge: Traditional installer with tweaker system (pre-1.20.2)
- NeoForge: Modern fork of Forge (1.20.2+), active development
- Quilt: Fabric fork with enhanced compatibility
- OptiFine: No official API, requires JAR introspection and patching

### 2. Existing Launcher Analysis

**PrismLauncher** (C++/Qt):
- Strength: Mature, feature-rich GUI
- Architecture: Centralized singleton, async task system
- Limitation: GUI-focused, not designed for CLI automation

**PortableMC** (Python/Rust):
- Strength: Clean CLI design, automation-friendly
- Architecture: Modular, version-agnostic core
- Key feature: One-command launch with loader prefixes

**HeadlessMC** (Java):
- Strength: CI/CD testing capabilities
- Key feature: LWJGL patching for headless mode
- Use case: Automated mod testing in pipelines

**MC-CLI** (Rust):
- Strength: LLM agent integration
- Key feature: JSON-structured output for parsing
- Architecture: Client-server model over TCP

### 3. Log and Diagnostic Patterns

**Log Files**:
- `logs/latest.log` - Standard game log
- `logs/debug.log` - Forge debug output (when enabled)
- `crash-reports/crash-*.txt` - Crash reports with stack traces
- `fabricloader.log` - Fabric loader critical errors

**Error Analysis**:
- Forge auto-identifies suspected mods in crash reports
- Mixin crashes clearly identify mod conflicts
- Common patterns: missing dependencies, version mismatches, class loading errors

**Diagnostic Automation**:
- Stack trace parsing for mod package identification
- Log4j configuration for enhanced debug output
- Automated error pattern detection

### 4. Agent-Friendly Design

**Key Principles**:
- JSON-structured output for all commands
- Standard exit codes (0 = success, non-zero = specific errors)
- Event streaming for long-running operations
- Stateless operation where possible

**Control Interfaces**:
- CLI with `--format json` option
- HTTP/JSON-RPC API for remote control
- WebSocket for real-time events
- Instance status queries and command execution

## Technology Recommendation

### Primary: Rust

**Rationale**:
- Performance: Native speed, minimal overhead
- Safety: Memory safety without garbage collection
- Distribution: Single binary, no runtime dependencies
- Ecosystem: Excellent libraries (clap, serde, tokio, reqwest)
- Cross-platform: Compile once for each target
- Reference: PortableMC Rust crate available

**Libraries**:
- `clap` - CLI argument parsing with auto-completion
- `serde`/`serde_json` - JSON serialization
- `reqwest` - HTTP client for API calls
- `tokio` - Async runtime for concurrent operations
- `sha1` - Checksum verification
- `zip` - Archive handling
- `tracing` - Structured logging

**Alternatives Considered**:
- Python: Faster development but requires runtime, slower execution
- Go: Good performance but less mature Minecraft ecosystem

## Proposed Architecture

### Core Modules

1. **Version Manager**
   - Fetch and cache version manifests
   - Download and verify game files
   - Manage Java runtime installations

2. **Loader Manager**
   - Abstract interface for all mod loaders
   - Loader-specific installers
   - Dependency resolution
   - Version compatibility checking

3. **Instance Manager**
   - Create/delete/list instances
   - Instance configuration and isolation
   - Import/export capabilities

4. **Launch Manager**
   - Construct launch commands
   - Process management
   - Output capture and parsing

5. **Diagnostic Manager**
   - Log collection and aggregation
   - Crash report parsing
   - Automated error detection
   - Diagnostic package generation

6. **Agent Interface**
   - HTTP/JSON-RPC server
   - Command execution
   - Event streaming
   - State queries

### Directory Structure
```
MCDebugLauncher/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── version/             # Version management
│   ├── loader/              # Mod loader support
│   ├── instance/            # Instance management
│   ├── diagnostic/          # Error analysis
│   ├── agent/               # Agent interface
│   └── util/                # Utilities
├── docs/
│   ├── RESEARCH.md
│   ├── RESEARCH_CN.md
│   ├── SPECIFICATION.md
│   └── ARCHITECTURE.md      # (To be created)
├── tests/
│   ├── integration/
│   └── unit/
├── Cargo.toml
├── README.md
├── README_CN.md
└── LICENSE
```

## Implementation Roadmap

### Phase 1: Foundation (2 weeks)
- Project setup with Rust/Cargo
- CLI framework with clap
- Version manifest fetching
- Basic vanilla Minecraft download and launch
- Configuration management

### Phase 2: Mod Loaders (2 weeks)
- Fabric installer implementation
- Forge installer implementation
- NeoForge installer implementation
- Quilt installer implementation
- OptiFine installer implementation

### Phase 3: Instance Management (1 week)
- Instance creation/deletion
- Instance isolation
- Mod folder management
- Configuration profiles

### Phase 4: Diagnostics (1 week)
- Log file collection
- Crash report parsing
- Error pattern detection
- Diagnostic package export

### Phase 5: Agent Interface (1 week)
- JSON output formatting
- Command API design
- Event streaming
- Documentation

### Phase 6: Testing & Polish (1 week)
- Integration tests
- CI/CD setup
- Documentation completion
- Binary packaging

**Total Estimated Time**: 8 weeks

## Design Decisions

1. **No GUI**: Pure CLI for maximum automation potential
2. **Standard Directory Structure**: Use `.minecraft/` for compatibility
3. **JSON-First Output**: All commands support `--format json`
4. **Isolated Instances**: Independent mods, configs, saves per instance
5. **Aggressive Error Handling**: Never silently fail, actionable messages
6. **Offline-First**: Cache everything, support fully offline operation

## Risk Assessment

### Technical Risks
- OptiFine: No official API (Mitigation: BMCLAPI mirror, JAR introspection)
- Launcher Changes: Mojang format updates (Mitigation: Version detection, backward compatibility)
- Mod Loader Updates: CLI changes (Mitigation: Version-specific installers)
- Java Compatibility: Multiple runtimes (Mitigation: Auto-download from Mojang)

### Maintenance Risks
- New mod loaders (Mitigation: Plugin architecture)
- Breaking changes (Mitigation: Comprehensive test suite)

## Next Steps

1. **Immediate**: Create ARCHITECTURE.md with detailed design
2. **Week 1**: Set up Rust project structure with Cargo
3. **Week 1-2**: Implement Phase 1 (Foundation)
4. **Week 2**: Establish CI/CD pipeline
5. **Week 3-4**: Implement Phase 2 (Mod Loaders)
6. **Ongoing**: Documentation updates and testing

## Resources and References

- [Minecraft Version Manifest](https://piston-meta.mojang.com/mc/game/version_manifest_v2.json)
- [PrismLauncher GitHub](https://github.com/PrismLauncher/PrismLauncher)
- [PortableMC GitHub](https://github.com/mindstorm38/portablemc)
- [HeadlessMC GitHub](https://github.com/headlesshq/headlessmc)
- [MC-CLI GitHub](https://github.com/Th0rgal/mc-cli)
- [Fabric Meta API](https://meta.fabricmc.net/)
- [Forge Files](https://files.minecraftforge.net/)
- [NeoForge Maven](https://maven.neoforged.net/)
- [Wiki.vg](https://wiki.vg/)

## Conclusion

The research phase has established a solid technical foundation for MCDebugLauncher. The analysis of existing launchers, Minecraft APIs, and mod loader ecosystems provides clear implementation patterns. Rust is the recommended technology for its performance, safety, and distribution advantages.

The proposed architecture balances simplicity with extensibility, following proven patterns from successful CLI tools. The 8-week implementation roadmap is achievable with focused development.

The project fills a gap in the Minecraft tooling ecosystem: a lightweight, automation-friendly launcher specifically designed for developers, CI/CD pipelines, and AI agent control.

---

**Document Version**: 1.0  
**Date**: 2026-07-22  
**Status**: Research Phase Complete, Ready for Implementation
