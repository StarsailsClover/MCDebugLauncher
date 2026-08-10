# MCDebugLauncher v26.0 Alpha 11 — Release Notes

> Theme: **Global UX improvement — real download progress display.**
>
> Date: 2026-08-11 · Branch: `main` · Version: `26.0.0-alpha.11`

## Highlights

### Real download progress bars
Every real file download driven through `download_file` — libraries, client
jars, Despotes / AprismRefract / AprismPrismate artifacts, the Bedrock
Dedicated Server, modpack files — now streams through a live `indicatif`
progress bar showing downloaded/total bytes and ETA:

```
client.jar [######>-----------------] 12.3 MB/45.2 MB (35%)
```

Both download paths are covered:
- **Single-stream downloads** (small/non-range servers) stream the response
  body and update the bar per chunk.
- **Chunked-parallel downloads** (large files on range-capable servers) use a
  single shared bar driven by received bytes across all four parallel Range
  tasks.
- **Asset batch downloads** (Minecraft objects) use one aggregate collection
  bar instead of hundreds of per-file bars.

### Smart gating (no flicker, no noise)
Progress bars only appear when all three hold:
1. `stderr` is an interactive terminal (TTY),
2. the terminal supports ANSI,
3. the transfer is at least 1 MB (smaller files finish too fast to need one).

Non-interactive runs, agent/API mode, and `--format json` output are
completely unaffected — JSON stays parseable, logs stay clean. Transfers with
unknown total size still show a bar with position updates.

## Verification
- `cargo test`: **67 passed / 0 failed** (+ 3 opt-in network integration tests).
- Release build green; `mdl --version` reports `26.0.0-alpha.11`.

## Files
- `mdl.exe` (Windows x64 release build)
- See `CHANGELOG.md` for the detailed changelog entry.
