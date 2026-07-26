# Handoff Document

## Bug Fix: NeoForge 21.4+ Duplicate Module Error

### Issue Summary
NeoForge 21.4.157 instances failed to launch with a duplicate module error for `mixin_synthetic` package. The error occurred because BootstrapLauncher loaded both neoforge-client.jar and neoforge-universal.jar as named modules, causing a conflict due to overlapping packages.

### Root Cause
The NeoForge official installer generates a `version.json` file with `-DignoreList` JVM argument, but the list may be missing the `neoforge-` prefix. Without this prefix, BootstrapLauncher treats the patched client JAR and universal JAR as named modules instead of keeping them on the unnamed module path (classpath). When two JARs with the same package are loaded as named modules, the JVM raises a duplicate-module error at startup.

### Solution
Modified `src/instance/launcher.rs` in the `load_loader_args` function (around line 960) to post-process JVM arguments for NeoForge instances:

1. Made `jvm_args` mutable to allow modification after loading from version.json
2. Added NeoForge-specific post-processing logic that:
   - Searches for existing `-DignoreList=` argument
   - If found but missing `neoforge-` prefix, appends `,neoforge-`
   - If not found at all, injects `-DignoreList=neoforge-`

This ensures BootstrapLauncher always keeps NeoForge JARs on the unnamed module path, avoiding the duplicate module conflict.

### Code Changes
**File**: `src/instance/launcher.rs`

**Change**: Lines 934-1008 in `load_loader_args` function
- Changed `let jvm_args = ...` to `let mut jvm_args = ...`
- Added post-processing block after JVM args parsing:

```rust
// NeoForge 21.4+ fix: the installer-produced version.json may omit
// "neoforge-" from -DignoreList, causing BootstrapLauncher to load the
// patched client/universal JARs as named modules. When two JARs with
// overlapping packages are treated as named modules, the JVM raises a
// mixin_synthetic duplicate-module error at startup. Appending
// "neoforge-" to the ignore list tells BootstrapLauncher to keep those
// JARs on the unnamed module path (classpath) instead.
let loader_type = config.loader.as_ref().map(|l| l.loader_type.as_str());
if loader_type == Some("neoforge") {
    let mut found = false;
    for arg in &mut jvm_args {
        if let Some(list) = arg.strip_prefix("-DignoreList=") {
            if !list.split(',').any(|e| e.trim() == "neoforge-") {
                arg.push_str(",neoforge-");
            }
            found = true;
            break;
        }
    }
    // If the installer omitted -DignoreList entirely, inject it so the
    // JVM flag is always present for NeoForge instances.
    if !found {
        jvm_args.push("-DignoreList=neoforge-".to_string());
    }
}
```

### Verification
- Build completed successfully: `cargo build` with 0 errors
- Solution handles both cases: missing prefix and completely absent `-DignoreList`
- No breaking changes to other loader types (Vanilla, Forge, Fabric, Quilt, OptiFine)

### Status
**Not committed yet** - Changes are staged but awaiting explicit commit instruction.

### Documentation Updates
- Added Technical Decision #16 to `memory/FACT.md` documenting the NeoForge 21.4+ fix

---

**Date**: 2026-07-26  
**Fixed by**: 泽川 (Sails)
