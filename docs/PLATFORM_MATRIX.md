# MDL Platform Matrix (v26.3-alpha.2)

Code-level audit of every platform gate in the source tree, plus the
verification status per platform. Use this as the checklist for real
Linux/macOS bring-up.

## Verification Status

| Platform | Build | Unit tests | E2E launch | Notes |
|---|---|---|---|---|
| windows-x64 (MSVC) | ✅ primary | ✅ 136 passed | ✅ continuous dogfooding | Reference platform |
| linux-x64 | ⏳ blocked | ⏳ | ⏳ | Target std unavailable on this host (rustup mirror 404); run `cargo check --target x86_64-unknown-linux-gnu` in CI/WSL |
| macos (aarch64/x64) | ⏳ blocked | ⏳ | ⏳ | Same; no Darwin host |
| windows-x64 (GNU) | ⏳ blocked | — | — | dlltool.exe missing; needs mingw-w64 binutils on PATH |

Blocked items carry the exact blocker so a CI pipeline (or WSL session) can
pick them up without re-diagnosis:

```bash
# Linux check (CI or WSL):
cargo check --bin mdl
cargo test --bin mdl

# GNU backend (after installing mingw-w64):
cargo +stable-x86_64-pc-windows-gnu check --bin mdl
```

## Feature Gates by Module

| Module / capability | Windows | Linux | macOS | Gate mechanism |
|---|---|---|---|---|
| Core CLI, versions, downloads, mirrors, cache | ✅ | ✅ | ✅ | portable (reqwest/tokio/walkdir) |
| Instance lifecycle, modpack import/export | ✅ | ✅ | ✅ | portable |
| Java auto-provisioning (Adoptium) | ✅ zip | ✅ tar.gz | ✅ tar.gz | archive_ext by OS (`version/java.rs`) |
| Agent HTTP/WebSocket server | ✅ | ✅ | ✅ | axum, loopback bind |
| Idle watchdog: log tailing + timeout | ✅ | ✅ | ✅ | portable |
| Watchdog termination | `TerminateProcess` via windows-sys | `kill -TERM` then `-KILL` | same | cfg branches (`game/watchdog.rs`) |
| Screenshots — Despotes framebuffer | ✅ | ✅ (in-game) | ✅ (in-game) | Despotes HTTP, portable |
| Screenshots — WGC window fallback | ✅ | ❌ 501 | ❌ 501 | `#[cfg(windows)] game::capture`; REST returns NOT_IMPLEMENTED |
| Game window discovery / titles | ✅ | ❌ | ❌ | `#[cfg(windows)] game::window` |
| OOM sweep (stale-process kill) | ✅ | ✅ | ✅ | sysinfo, portable |
| OOM working-set trim | ✅ EmptyWorkingSet | no-op | no-op | cfg (`game/oom_guard.rs`) |
| OOM standby-list purge | ✅ admin | ❌ no-op | ❌ no-op | NtSetSystemInformation |
| DLL injector (`mdl inject`) | ✅ CreateRemoteThread | ❌ clean bail | ❌ clean bail | `util/injector.rs` |
| Hot-attach JavaAgent | ✅ | ✅ | ✅ | jdk.attach helper, portable by design |
| JE dedicated server lifecycle | ✅ | ✅ | ✅ | taskkill vs kill(1) in `loader/server.rs` |
| Bedrock Dedicated Server | ✅ target | ⚠️ untested upstream artifact | ❌ Windows-oriented BDS builds | download path only gates nothing |
| UTF-8 console (SetConsoleOutputCP) | ✅ | n/a | n/a | cfg in `main.rs` |

## Known Behavioral Differences to Verify on First Linux/macOS Run

1. **Watchdog grace**: SIGTERM to a JVM normally shuts it down; confirm the
   5s grace is enough for Minecraft to exit cleanly before `-KILL`.
2. **Launch-lock transfer** writes the game PID; verify `is_pid_running`
   `/proc/<pid>` probe behaves with zombie processes.
3. **Detached spawn handle clearing** is a no-op on Unix (no inherit flags);
   confirm backgrounded launches detach from the calling shell.
4. **Window title feature** (`--title`, console title) silently degrades;
   screenshots rely solely on Despotes framebuffer.
5. **`mdl setup` PATH helper** currently assumes Windows registry semantics;
   expect it to be a no-op/error on Unix until ported (documented gap).
6. **BDS support** targets Windows builds of the server; treat Linux BDS as
   unsupported until the download matrix learns the linux artifact.

*Audit scope: static code inspection at v26.3-alpha.2; runtime rows marked ⏳
are pending first non-Windows CI run.*
