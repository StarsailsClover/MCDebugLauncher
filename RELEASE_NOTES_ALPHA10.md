# MCDebugLauncher v26.0 Alpha 10 — Release Notes

> Theme: **Aprism product-matrix support** + **Despotes for vanilla**.
>
> Date: 2026-08-11 · Branch: `main` · Version: `26.0.0-alpha.10`

## Highlights

### 1. Aprism JE Native loader (`--aprism`) — now actually wired
The `--aprism` launch flag previously existed but was never connected. It now:
- detects the applicable `Aprism-<tag>-JE-<mc>.jar` from GitHub Releases
  (stable-first, pre-release only with explicit opt-in),
- downloads + caches it,
- mounts `-javaagent:<jar>=aprismVersion=<tag>;mcEdit=JE;mcVersion=<mc>;gameRoot=<dir>`.

### 2. AprismRefract — loader-support extensions
`mdl aprism refract install <instance> [--loader <l>] [--mc-version <v>] [--prerelease]`
installs the matching `.aep` (`<Loader>-Support-A<range>-<Key><range>-JE-<mc>.aep`)
into the instance's `aprism-extensions/` directory, enabling the Aprism loader
to run Fabric / Forge / NeoForge / Quilt / LiteLoader mods. `refract list` shows
what is installed.

### 3. AprismPrismate — loader-side bridge
`mdl aprism prismate install <instance> ...` installs
`AprismPrismate-v<ver>-<Fa|N|Fo>-<mc>.jar` into `mods/`, letting
Fabric / NeoForge / Forge load Aprism-native `.aje` packs. `prismate status`
reports installation. The launcher refuses the conflicting `--aprism` + Prismate
combination with a clear error (they are mutually exclusive in one instance).

### 4. Despotes for vanilla (bug fix)
Reported bug: *“找不到 loader none 对应的 Despotes”*. Root cause: vanilla
instances have no mod loader, so `despotes_loader_for(None)` returned `None`
and the offer skipped with “not applicable”.

Fix:
- Vanilla / `none` instances now map to the Despotes **`native`** branch, which
  attaches as a JVM `-javaagent` rather than a `mods/` jar.
- `install_native` places the agent at the instance root
  (`despotes-agent.jar`); `mdl create`/offer installs it there; the launcher
  mounts `-javaagent:...` automatically in `--agent` mode (when the Aprism
  loader is not requested, since the Aprism loader pairs with the `aprism`
  Despotes variant).
- Despotes asset parsing now accepts the Aprism variant's `.aje` suffix.

## Verification
- `cargo test`: **67 passed / 0 failed** (+ 3 opt-in network integration tests).
- End-to-end (real GitHub downloads):
  - `mdl aprism refract install despotes-test-26.2 --prerelease` →
    installed `Fabric-Support-...-JE-26.2.aep` into `aprism-extensions/`.
  - `mdl aprism prismate install despotes-test-26.2 --prerelease` →
    installed `AprismPrismate-v26.1-Fa-26.2.jar` into `mods/`.
  - Integration tests `test_real_native_variant_for_vanilla` and
    `test_real_aprism_javaagent_download` pass against live releases.

## Known limitation
- `mdl aprism refract install` requires the target loader to have a published
  `.aep`; currently the AprismRefract releases only ship `JE-26.2` artifacts,
  so older MC versions report “No applicable AprismRefract .aep”.

## Files
- `mdl.exe` (Windows x64 release build)
- See `CHANGELOG.md` for the detailed changelog entry.
