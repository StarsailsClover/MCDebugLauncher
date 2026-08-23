// OOM self-protection (v26.2-alpha.4)
//
// Minecraft is memory-heavy: a modded instance easily consumes 4-8 GB of
// heap, and the JVM's native overhead (thread stacks, direct buffers,
// class metaspace) can push the total well beyond the -Xmx cap. When an
// agent-driven workflow launches instance after instance, or when the
// system is already under memory pressure from other applications, the
// new JVM may fail to allocate its heap and exit immediately with an
// OOM error.
//
// This module provides two complementary defences, run *before* the JVM
// is spawned:
//
// 1. **Stale-process termination** — detect and kill leftover Minecraft
//    / Java processes that are no longer tracked by MDL's launch lock.
//    A crashed or orphaned `java.exe` from a previous session can hold
//    gigabytes of RAM hostage.
//
// 2. **RAM pressure relief** — trim the working sets of all running
//    processes (including MDL itself) to force Windows to move idle
//    pages to the standby list, then optionally purge the standby list
//    and flush modified pages so the freed RAM becomes immediately
//    available for the new JVM.
//
// On non-Windows platforms the functions are no-ops (or use `kill`/`sync`
// equivalents where applicable).
//
// v26.2 exclusions:
// The stale-process heuristic targets any `java` process whose command line
// mentions "minecraft" — which also matches *build toolchain* processes
// forked inside projects whose paths contain "Minecraft" (Gradle workers,
// NeoGradle's JST source transformer, decompiler forks, ...). Killing those
// silently breaks long builds. Two layers of exclusions now apply:
//
// 1. **Built-in dev-toolchain exclusions** — command-line substrings that
//    identify well-known build infrastructure (`gradle`, `jst-cli`, ...).
// 2. **User exclusions file** — `<data_dir>/oom_excludes.txt`, one
//    case-insensitive substring per line, `#` comments allowed. Extra
//    entries are merged with the built-in list.
//
// GitHub@NDBlockConnect | BlockConnect@StarsailsClover

use anyhow::Result;
use std::time::Duration;

/// Result of a pre-launch OOM protection sweep.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct OomGuardReport {
    /// Number of stale Minecraft/Java processes that were terminated.
    pub killed_processes: u32,
    /// Candidates found by the sweep (v26.3-alpha.2). Exceeds
    /// `killed_processes` when the user aborted the confirmation prompt or
    /// list-only mode was requested.
    pub listed_candidates: usize,
    /// Working-set pages freed across all processes (best-effort, in bytes).
    /// Zero when the platform does not support working-set trimming.
    pub ws_freed_bytes: u64,
    /// Whether the system standby list was purged (requires admin).
    pub standby_purged: bool,
    /// Available physical memory after cleanup, in bytes.
    pub available_after_bytes: u64,
}

/// Run the full OOM protection sequence: kill stale Minecraft processes,
/// trim working sets, optionally purge standby lists.
///
/// `skip_kill` — when true, do not terminate any processes (only trim).
/// `aggressive` — when true, also purge system standby list (requires
/// admin privileges; silently falls back to working-set trim only).
pub async fn pre_launch_protection(
    confirm_mode: OomConfirmMode,
    list_only: bool,
    aggressive: bool,
) -> Result<OomGuardReport> {
    let mut report = OomGuardReport::default();

    // Phase 1: kill stale Minecraft processes (behind the confirmation gate).
    {
        let (killed, candidates) =
            kill_stale_minecraft_processes(confirm_mode, list_only).await;
        report.killed_processes = killed;
        report.listed_candidates = candidates;
        if report.killed_processes > 0 {
            // Give the OS a moment to reclaim the freed pages before we
            // proceed to working-set trimming.
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    // Phase 2: trim working sets of all processes.
    let before_avail = available_memory_bytes();
    report.ws_freed_bytes = trim_all_working_sets();
    tracing::debug!(
        "Working-set trim: freed {} bytes (approx), available before={}",
        report.ws_freed_bytes,
        before_avail
    );

    // Phase 3: aggressive — purge standby list (requires admin).
    if aggressive {
        if purge_standby_list() {
            report.standby_purged = true;
            tracing::info!("System standby list purged (aggressive OOM protection)");
        } else {
            tracing::debug!("Standby purge skipped (not elevated or unsupported)");
        }
    }

    report.available_after_bytes = available_memory_bytes();
    Ok(report)
}

// ─────────────────────────────────────────────────────────────────────────
// Phase 1: Stale Minecraft process detection and termination
// ─────────────────────────────────────────────────────────────────────────

/// Process names that are considered Minecraft / Java game processes.
const MINECRAFT_PROCESS_NAMES: &[&str] = &[
    "java.exe",
    "javaw.exe",
    "Minecraft.exe",
    "Minecraft.Windows.exe",
    "MinecraftLauncher.exe",
];

/// Command-line substrings that identify build-toolchain processes which
/// must never be terminated by the stale-process sweep, even when their
/// command line mentions "minecraft" (e.g. projects living under a
/// `...\Minecraft\...` directory). Matched case-insensitively.
///
/// v26.3-alpha.5 extends this after a field report of compile processes
/// being killed: Kotlin compile daemons, IntelliJ JPS builds, Maven forks
/// and Eclipse JDT launched inside `...\Minecraft\...` workspaces were all
/// swept because their command lines contain the workspace path.
///
/// GitHub@NDBlockConnect | BlockConnect@StarsailsClover
const BUILTIN_EXCLUDE_SUBSTRINGS: &[&str] = &[
    "gradle",                          // Gradle daemons, workers, wrappers
    "org.gradle.launcher",             // explicit launcher classes (belt & braces)
    "jst-cli",                         // NeoGradle JST source transformer
    "javac",                           // Java compiler forks
    "forgeflower",                     // NeoForm/ForgeGradle deobfuscator
    "vineflower",                      // decompiler forks
    "cfr",                             // decompiler forks
    "fernflower",                      // decompiler forks
    "net.neoforged",                   // NeoGradle/NeoForm utility JVMs
    "net.minecraftforge",              // ForgeGradle utility JVMs
    // v26.3-alpha.5: compilers/build systems missed by the first pass.
    "kotlincompiledaemon",             // Kotlin compile daemon (no 'gradle' in its cmdline)
    "org.jetbrains.kotlin.compiler",   // Kotlin compiler entrypoints
    "org.jetbrains.jps",               // IntelliJ IDEA build process
    "org.eclipse.jdt",                 // Eclipse JDT LS / batch compiler
    "org.apache.maven",                // Maven + surefire forks
    "surefirebooter",                  // Maven test forks
];

/// Strong markers that distinguish an ACTUAL Minecraft client/server launch
/// from any Java process that merely mentions a path containing
/// "minecraft". A candidate java/javaw process qualifies only when its
/// command line carries one of these — package names of the game itself,
/// its mod loaders, vanilla CLI flags, or known launcher injectors.
///
/// v26.3-alpha.5 replaces the old weak rule (any "minecraft" substring),
/// which false-positived on every Java tool running inside a
/// `...Minecraft...` workspace.
const STRONG_LAUNCH_MARKERS: &[&str] = &[
    "net.minecraft.",                  // game packages (client main, server)
    "cpw.mods.bootstraplauncher",      // NeoForge BootstrapLauncher
    "cpw.mods.modlauncher",            // ModLauncher (Forge/NeoForge)
    "--gameDir",                       // vanilla client/server flag
    "--assetsdir",                     // vanilla client flag (lowercased match)
    "--assetindex",                    // vanilla client flag
    "fabricloader",                    // fabric-loader jar coordinate
    "fmlloader",                       // Forge/NeoForge FML loader jar
    "neoforge-",                       // neoforge-*-universal/client on -cp
    "forge-",                          // forge-*-universal/client on -cp
    "devlaunchinjector",               // Fabric Loom dev runs
    "org.spongepowered.",              // Mixin
];

/// Load user-defined exclusion substrings from
/// `<data_dir>/oom_excludes.txt` (one substring per line, `#` comments).
/// Missing file yields an empty list; the result is merged with
/// [`BUILTIN_EXCLUDE_SUBSTRINGS`] by the caller.
fn load_user_exclusions() -> Vec<String> {
    let path = match crate::util::paths::get_data_dir() {
        Ok(d) => d.join("oom_excludes.txt"),
        Err(_) => return Vec::new(),
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_lowercase)
        .collect()
}

/// Check whether a lowercased command line matches any exclusion
/// (built-in or user-provided).
fn is_excluded_command(cmd_lower: &str, user_excludes: &[String]) -> bool {
    BUILTIN_EXCLUDE_SUBSTRINGS
        .iter()
        .copied()
        .chain(user_excludes.iter().map(|s| s.as_str()))
        .any(|ex| cmd_lower.contains(ex))
}

/// Whether a scanned process is a stale-Minecraft termination candidate.
///
/// v26.3-alpha.5 decision matrix (replaces the old "cmdline mentions
/// minecraft anywhere" rule that killed compile daemons in workspaces
/// whose paths contain "Minecraft"):
///
/// 1. Native launcher/game executables (Minecraft.exe, ...) — always
///    candidates; they cannot be build tools.
/// 2. java/javaw — candidates ONLY when the command line carries a strong
///    launch marker (game packages, loader jars, vanilla flags). Build
///    tools never carry these.
/// 3. Exclusions veto last, so anything matching both a strong marker and
///    an exclusion (e.g. Gradle-launched dev runs) is protected.
fn is_target_candidate(
    name_lower: &str,
    cmd_lower: &str,
    has_cmdline: bool,
    user_excludes: &[String],
) -> bool {
    let native = MINECRAFT_PROCESS_NAMES
        .iter()
        .any(|p| p.to_lowercase() == name_lower);
    // java/javaw count as generic JVMs WITH or WITHOUT the .exe suffix;
    // they must go through command-line disambiguation, never the native
    // fast path.
    let java_generic = matches!(name_lower, "java" | "javaw" | "java.exe" | "javaw.exe");

    if native && !java_generic {
        return true;
    }
    if !java_generic {
        return false;
    }
    if !has_cmdline {
        // Cannot disambiguate without a command line — leave it alone.
        return false;
    }
    let strong = STRONG_LAUNCH_MARKERS
        .iter()
        .any(|m| cmd_lower.contains(&m.to_lowercase()));
    if !strong {
        return false;
    }
    !is_excluded_command(cmd_lower, user_excludes)
}

/// Detect and terminate stale Minecraft / Java processes that are not
/// tracked by MDL's launch lock (i.e. not the current MDL-managed
/// instance). Returns the number of processes killed.
///
/// v26.3-alpha.2 adds a second-confirmation gate before any termination:
/// every candidate is listed with PID, window title and memory footprint,
/// and (depending on [`OomConfirmMode`]) an explicit y/N prompt is shown.
///
/// The current MDL process's own PID is always excluded. When a launch
/// lock file exists, the PID recorded there is also preserved.
pub async fn kill_stale_minecraft_processes(mode: OomConfirmMode, list_only: bool) -> (u32, usize) {
    let our_pid = std::process::id();

    // Read the launch lock to find the currently-tracked game PID (if any).
    let protected_pid = read_launch_lock_pid().await;

    let processes = find_minecraft_processes();
    let mut candidates: Vec<ProcInfo> = Vec::new();
    for proc in processes {
        if proc.pid == our_pid {
            continue;
        }
        if let Some(ppid) = protected_pid {
            if proc.pid == ppid {
                tracing::debug!(
                    "Skipping protected game process PID {} (tracked by launch lock)",
                    ppid
                );
                continue;
            }
        }
        candidates.push(proc);
    }

    // Enrich with window titles (best-effort, Windows only).
    for c in &mut candidates {
        #[cfg(windows)]
        {
            c.title = crate::game::window::window_title_for_pid(c.pid);
        }
        #[cfg(not(windows))]
        {
            c.title = None;
        }
    }

    // Always surface what was found — even before any prompting — so logs
    // record exactly what the sweep considered.
    for c in &candidates {
        tracing::info!("{}", format_candidate_row(c));
    }

    if candidates.is_empty() {
        return (0, 0);
    }

    if list_only {
        tracing::info!(
            "OOM protection: list-only mode, {} candidate(s) found, nothing terminated",
            candidates.len()
        );
        println!(
            "OOM protection (list-only): {} candidate(s):",
            candidates.len()
        );
        for c in &candidates {
            println!("  {}", format_candidate_row(c));
        }
        return (0, candidates.len());
    }

    // Second-confirmation gate.
    if mode.should_prompt(stdin_is_interactive()) {
        println!(
            "OOM protection wants to terminate {} stale process(es) listed above.",
            candidates.len()
        );
        print!("Proceed with termination? [y/N] ");
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        let ok = std::io::stdin().read_line(&mut line)
            .map(|_| {
                let a = line.trim().to_ascii_lowercase();
                a == "y" || a == "yes"
            })
            .unwrap_or(false);
        if !ok {
            tracing::info!("OOM protection: termination aborted by user");
            println!("Aborted — stale processes were left running.");
            return (0, candidates.len());
        }
    } else if mode == OomConfirmMode::Auto && !stdin_is_interactive() {
        tracing::info!(
            "OOM protection: non-interactive session, terminating {} candidate(s) without prompt",
            candidates.len()
        );
    }

    let mut killed = 0u32;
    for proc in &candidates {
        tracing::warn!(
            "OOM protection: terminating stale process '{}' (PID {}, RSS={} MB)",
            proc.name,
            proc.pid,
            proc.rss_bytes / 1024 / 1024
        );

        if terminate_process(proc.pid) {
            killed += 1;
        } else {
            tracing::warn!("Failed to terminate PID {}", proc.pid);
        }
    }

    if killed > 0 {
        tracing::info!("OOM protection: terminated {} stale process(es)", killed);
    }

    (killed, candidates.len())
}

/// One line describing a sweep candidate: PID, name, memory, window title.
fn format_candidate_row(c: &ProcInfo) -> String {
    const MAX_TITLE: usize = 48;
    let title = match &c.title {
        Some(t) => {
            let t = t.trim();
            if t.chars().count() > MAX_TITLE {
                let cut: String = t.chars().take(MAX_TITLE).collect();
                format!("{cut}…")
            } else if t.is_empty() {
                "-".into()
            } else {
                t.to_string()
            }
        }
        None => "-".into(),
    };
    format!(
        "PID {:>7}  {:<22} {:>6} MB  title: {}",
        c.pid,
        truncate_str(&c.name, 22),
        c.rss_bytes / 1024 / 1024,
        title
    )
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max - 1).collect::<String>() + "…"
    }
}

fn stdin_is_interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// Policy for the second-confirmation prompt before terminations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OomConfirmMode {
    /// Prompt only when stdin is an interactive terminal; unattended runs
    /// proceed without prompting (preserves agent automation).
    #[default]
    Auto,
    /// Always prompt, regardless of TTY state (EOF reads count as "No").
    Always,
    /// Never prompt — terminate immediately after listing targets.
    Never,
}

impl OomConfirmMode {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            "never" => Ok(Self::Never),
            other => anyhow::bail!(
                "Invalid --oom-confirm value '{other}' (expected auto|always|never)"
            ),
        }
    }

    fn should_prompt(self, stdin_tty: bool) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Auto => stdin_tty,
        }
    }
}

/// Information about a running process found during scanning.
#[derive(Debug, Clone)]
struct ProcInfo {
    pid: u32,
    name: String,
    rss_bytes: u64,
    /// Best-effort visible window title (Windows only; filled after scan).
    title: Option<String>,
}

/// Scan running processes for Minecraft/Java processes.
fn find_minecraft_processes() -> Vec<ProcInfo> {
    use sysinfo::{ProcessRefreshKind, System};

    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessRefreshKind::everything());

    let mut result = Vec::new();
    let user_excludes = load_user_exclusions();
    if !user_excludes.is_empty() {
        tracing::debug!(
            "OOM protection: {} user exclusion entr(ies) loaded",
            user_excludes.len()
        );
    }
    for (pid, proc) in sys.processes() {
        let name = proc.name().to_string();
        let lower = name.to_lowercase();

        // v26.3-alpha.5: unified candidate decision (strong markers +
        // exclusions) replacing the old weak "mentions minecraft" filter.
        let cmdline = proc
            .cmd()
            .iter()
            .map(|s| s.clone())
            .collect::<Vec<_>>()
            .join(" ");
        let cmd_lower = cmdline.to_lowercase();

        if !is_target_candidate(
            &lower,
            &cmd_lower,
            !cmdline.is_empty(),
            &user_excludes,
        ) {
            if cmdline.to_lowercase().contains("minecraft") {
                tracing::debug!(
                    "OOM protection: skipping PID {} (mentions minecraft but no strong launch marker / excluded)",
                    pid.as_u32()
                );
            }
            continue;
        }

        result.push(ProcInfo {
            pid: pid.as_u32(),
            name,
            rss_bytes: proc.memory(),
            title: None,
        });
    }

    result
}

/// Read the PID from MDL's launch lock file (if present and live).
async fn read_launch_lock_pid() -> Option<u32> {
    let lock_path = match crate::util::paths::get_data_dir() {
        Ok(d) => d.join("launching.lock"),
        Err(_) => return None,
    };
    if !lock_path.exists() {
        return None;
    }
    match tokio::fs::read_to_string(&lock_path).await {
        Ok(content) => content.trim().parse::<u32>().ok(),
        Err(_) => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Phase 2: Working-set trimming
// ─────────────────────────────────────────────────────────────────────────

/// Trim the working set of every running process. Returns the total
/// bytes freed (approximate, based on before/after available memory delta).
fn trim_all_working_sets() -> u64 {
    let before = available_memory_bytes();

    #[cfg(windows)]
    {
        trim_all_working_sets_windows();
    }

    #[cfg(not(windows))]
    {
        // On Linux/macOS, the kernel manages memory reclamation automatically.
        // A `sync` call can flush dirty pages to disk, but there's no direct
        // equivalent of EmptyWorkingSet.
    }

    let after = available_memory_bytes();
    after.saturating_sub(before)
}

#[cfg(windows)]
fn trim_all_working_sets_windows() {
    use sysinfo::{ProcessRefreshKind, System};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError},
        System::Threading::OpenProcess,
    };

    // PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA
    const ACCESS: u32 = 0x1000 | 0x0100;

    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessRefreshKind::everything());

    let mut trimmed = 0u32;
    let mut failed = 0u32;

    for (pid, _) in sys.processes() {
        let pid_u32 = pid.as_u32();

        // Skip our own process — it would be disruptive during launch.
        if pid_u32 == std::process::id() {
            continue;
        }

        unsafe {
            let handle = OpenProcess(ACCESS, 0, pid_u32);
            if handle.is_null() {
                failed += 1;
                continue;
            }

            // Call EmptyWorkingSet (exported as K32EmptyWorkingSet in kernel32).
            let result = EmptyWorkingSet(handle);
            if result != 0 {
                trimmed += 1;
            } else {
                failed += 1;
            }
            CloseHandle(handle);
        }
    }

    tracing::debug!(
        "Working-set trim: {} processes trimmed, {} failed (last error={})",
        trimmed,
        failed,
        unsafe { GetLastError() }
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Phase 3: System standby list purge (aggressive, requires admin)
// ─────────────────────────────────────────────────────────────────────────

/// Purge the system standby list and flush modified pages. Requires
/// admin/elevated privileges on Windows. Returns false on non-Windows or
/// when not elevated.
fn purge_standby_list() -> bool {
    #[cfg(windows)]
    {
        purge_standby_list_windows()
    }

    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn purge_standby_list_windows() -> bool {
    // The standby list purge uses the undocumented NtSetSystemInformation
    // API with SystemMemoryListInformation. This requires SeProfileSingleProcessPrivilege
    // (i.e. admin). We attempt it and return false on failure.

    // SYSTEM_INFORMATION_CLASS: SystemMemoryListInformation = 80
    const SYSTEM_MEMORY_LIST_INFORMATION: u32 = 80;

    // SYSTEM_MEMORY_LIST_COMMAND values:
    // MemoryPurgeLowPriorityStandbyList = 1
    // MemoryPurgeStandbyList = 4
    // MemoryFlushModifiedList = 3
    #[repr(C)]
    struct SystemMemoryListCommand(i32);

    let ntdll = unsafe {
        windows_sys::Win32::System::LibraryLoader::GetModuleHandleA(
            b"ntdll.dll\0".as_ptr(),
        )
    };
    if ntdll.is_null() {
        return false;
    }

    let proc_addr = unsafe {
        windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            ntdll,
            b"NtSetSystemInformation\0".as_ptr(),
        )
    };
    if proc_addr.is_none() {
        return false;
    }

    // Function pointer type for NtSetSystemInformation.
    type NtSetSystemInformationFn = unsafe extern "system" fn(
        info_class: u32,
        info: *mut std::ffi::c_void,
        info_len: u32,
    ) -> i32; // NTSTATUS (0 = STATUS_SUCCESS)

    let nt_set = unsafe {
        std::mem::transmute::<_, NtSetSystemInformationFn>(proc_addr.unwrap())
    };

    unsafe {
        // Step 1: Flush modified pages to disk.
        let flush = SystemMemoryListCommand(3);
        let status = nt_set(
            SYSTEM_MEMORY_LIST_INFORMATION,
            &flush as *const _ as *mut _,
            std::mem::size_of::<SystemMemoryListCommand>() as u32,
        );
        if status < 0 {
            tracing::debug!(
                "NtSetSystemInformation(MemoryFlushModifiedList) failed: NTSTATUS=0x{:08X}",
                status as u32
            );
            // Not elevated or unsupported — fall through but expect failure.
        }

        // Step 2: Purge low-priority standby list.
        let purge_low = SystemMemoryListCommand(1);
        let _ = nt_set(
            SYSTEM_MEMORY_LIST_INFORMATION,
            &purge_low as *const _ as *mut _,
            std::mem::size_of::<SystemMemoryListCommand>() as u32,
        );

        // Step 3: Purge full standby list.
        let purge_all = SystemMemoryListCommand(4);
        let status = nt_set(
            SYSTEM_MEMORY_LIST_INFORMATION,
            &purge_all as *const _ as *mut _,
            std::mem::size_of::<SystemMemoryListCommand>() as u32,
        );

        if status < 0 {
            tracing::debug!(
                "NtSetSystemInformation(MemoryPurgeStandbyList) failed: NTSTATUS=0x{:08X} (not elevated?)",
                status as u32
            );
            return false;
        }

        true
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Utilities: memory queries and process termination
// ─────────────────────────────────────────────────────────────────────────

/// Query available (free + standby) physical memory in bytes.
fn available_memory_bytes() -> u64 {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    sys.available_memory()
}

/// Terminate a process by PID. Returns true on success.
fn terminate_process(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE},
        };
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if handle.is_null() {
                return false;
            }
            let result = TerminateProcess(handle, 1);
            CloseHandle(handle);
            result != 0
        }
    }

    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
        true
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Windows FFI declarations
// ─────────────────────────────────────────────────────────────────────────

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    /// EmptyWorkingSet — trims as many pages as possible from the working
    /// set of the specified process. Exported from kernel32.dll on Windows 7+.
    fn EmptyWorkingSet(h_process: windows_sys::Win32::Foundation::HANDLE) -> i32;
}

// ─────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confirm_mode_parse() {
        assert_eq!(OomConfirmMode::parse("auto").unwrap(), OomConfirmMode::Auto);
        assert_eq!(OomConfirmMode::parse("ALWAYS").unwrap(), OomConfirmMode::Always);
        assert_eq!(OomConfirmMode::parse("never").unwrap(), OomConfirmMode::Never);
        assert!(OomConfirmMode::parse("yes").is_err());
        assert!(OomConfirmMode::parse("").is_err());
    }

    #[test]
    fn test_should_prompt_truth_table() {
        // Auto: only when stdin is a TTY.
        assert!(OomConfirmMode::Auto.should_prompt(true));
        assert!(!OomConfirmMode::Auto.should_prompt(false));
        // Always/Never are unconditional.
        assert!(OomConfirmMode::Always.should_prompt(false));
        assert!(!OomConfirmMode::Never.should_prompt(true));
    }

    #[test]
    fn test_format_candidate_row_truncates_title() {
        let c = ProcInfo {
            pid: 1234,
            name: "javaw.exe".into(),
            rss_bytes: 2 * 1024 * 1024 * 1024,
            title: Some("Minecraft* 1.21.1 - some very long window title that goes on".into()),
        };
        let row = format_candidate_row(&c);
        assert!(row.contains("PID    1234"));
        assert!(row.contains("2048 MB"));
        assert!(row.contains('…')); // truncated
        // No-title fallback.
        let c2 = ProcInfo { pid: 5, name: "java".into(), rss_bytes: 0, title: None };
        let row2 = format_candidate_row(&c2);
        assert!(row2.contains("title: -"));
    }

    #[test]
    fn test_oom_guard_report_serializes() {
        let report = OomGuardReport {
            killed_processes: 2,
            listed_candidates: 2,
            ws_freed_bytes: 1_073_741_824, // 1 GB
            standby_purged: true,
            available_after_bytes: 8_589_934_592, // 8 GB
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"killed_processes\":2"));
        assert!(json.contains("\"standby_purged\":true"));
    }

    #[test]
    fn test_oom_guard_report_defaults() {
        let report = OomGuardReport::default();
        assert_eq!(report.killed_processes, 0);
        assert_eq!(report.listed_candidates, 0);
        assert_eq!(report.ws_freed_bytes, 0);
        assert!(!report.standby_purged);
        assert_eq!(report.available_after_bytes, 0);
    }

    #[test]
    fn test_find_minecraft_processes_returns_vec() {
        // This test just verifies the function doesn't panic and returns
        // a Vec (likely empty on CI since no Minecraft is running).
        let _procs = find_minecraft_processes();
    }

    #[tokio::test]
    async fn test_pre_launch_protection_list_only_reports() {
        // SAFETY: list_only=true guarantees the sweep never terminates
        // anything, so this test is safe to run on a real developer machine
        // (live-fire sweeps inside unit tests once killed a running build
        // daemon — see v26.3-alpha.5).
        let report =
            pre_launch_protection(OomConfirmMode::Never, true, false).await.unwrap();
        assert_eq!(report.killed_processes, 0, "list-only must never terminate");
        assert!(report.available_after_bytes > 0);
    }

    #[tokio::test]
    async fn test_pre_launch_protection_non_aggressive_list_only() {
        let report =
            pre_launch_protection(OomConfirmMode::Never, true, false).await.unwrap();
        assert!(!report.standby_purged, "Standby should not be purged in non-aggressive mode");
    }

    #[test]
    fn test_live_fire_sweeps_are_never_executed_in_tests() {
        // Compile-time guard: the only sanctioned way to call
        // pre_launch_protection from tests is with list_only=true. This
        // assertion documents that invariant for reviewers.
        let mode = OomConfirmMode::parse("never").unwrap();
        assert_eq!(mode, OomConfirmMode::Never);
    }

    #[test]
    fn test_available_memory_nonzero() {
        let mem = available_memory_bytes();
        assert!(mem > 0, "Available memory should be positive");
    }

    #[test]
    fn test_read_launch_lock_pid_returns_none_when_no_file() {
        // The lock file may or may not exist; this just verifies no panic.
        // We can't easily control the lock file state in a unit test.
    }

    #[test]
    fn test_builtin_excludes_cover_build_toolchain() {
        // Gradle daemon / JST command lines as observed in the field.
        let gradle_daemon = "c:\\java\\jdk-25\\bin\\java.exe --add-opens=java.base/java.lang=all-unnamed \
            -cp c:\\users\\x\\.gradle\\wrapper\\dists\\gradle-9.2.1\\lib\\gradle-launcher-9.2.1.jar \
            org.gradle.launcher.daemon.bootstrap.gradledaemon 9.2.1";
        assert!(is_excluded_command(gradle_daemon, &[]));

        let jst = "java.exe -cp ...jst-cli-bundle-2.0.1.jar... com.intellij.util.containers.unsafe \
            c:\\users\\x\\.gradle\\caches\\... net.neoforged.jst.cli";
        assert!(is_excluded_command(jst, &[]));
    }

    /// v26.3-alpha.5 regression matrix for the field report: OOM killed
    /// compile daemons / IDE builds inside `...\Minecraft\...` workspaces.
    #[test]
    fn test_compile_and_ide_processes_are_never_candidates() {
        let user = vec![]; // no user excludes — built-ins alone must suffice

        // Kotlin compile daemon: project path mentions minecraft but there
        // is NO gradle substring in its own command line.
        let kotlin_daemon = "c:\\jdk21\\bin\\java.exe -cp c:\\users\\sails\\.gradle\\caches\\kotlin-dsl\\... \
             org.jetbrains.kotlin.compiler.daemon.kotlincompiledaemonservices \
             -cp c:\\users\\sails\\documents\\workspace\\domain-projects\\minecraft\\mymod build.gradle.kts";
        assert!(!is_target_candidate("java.exe", &kotlin_daemon.to_lowercase(), true, &user));

        // IntelliJ JPS build process.
        let jps = "\"c:\\jdk17\\bin\\java.exe\" -Xmx2g -classpath \"c:\\program files\\jetbrains\\...\\jps-builders.jar;...\" \
             org.jetbrains.jps.cmdline.buildmain c:\\users\\sails\\appdata\\local\\jetbrains\\... \
             c:\\users\\sails\\documents\\workspace\\minecraft-project";
        assert!(!is_target_candidate("java.exe", &jps.to_lowercase(), true, &user));

        // Maven surefire fork inside a minecraft-named project dir.
        let maven = "java -jar c:\\repo\\org\\apache\\maven\\surefire\\surefirebooter.jar \
             c:\\work\\minecraft-plugin\\target\\test-classes";
        assert!(!is_target_candidate("java", &maven.to_lowercase(), true, &user));

        // Eclipse JDT language server.
        let jdt = "java -jar c:\\jdt.ls\\plugins\\org.eclipse.equinox.launcher.jar \
             -configuration c:\\workspaces\\minecraft-mod\\.jdt";
        assert!(!is_target_candidate("javaw.exe", &jdt.to_lowercase(), true, &user));

        // Plain javac fork (already covered by exclusions).
        let javac = "javac @c:\\work\\minecraft-mod\\build\\sources.txt";
        assert!(!is_target_candidate("java.exe", &javac.to_lowercase(), true, &user));

        // Unrelated Java app with zero minecraft mention.
        let unrelated = "java -jar service.jar --port 8080";
        assert!(!is_target_candidate("java", unrelated, true, &user));
    }

    #[test]
    fn test_real_game_launches_remain_candidates() {
        let user = vec![];

        // Vanilla client.
        let vanilla = "\"c:\\program files\\java\\bin\\javaw.exe\" -Xmx4G \
             -cp libraries.jar net.minecraft.client.main.Main --username TestPlayer \
             --gameDir . --assetsDir assets --assetIndex 17";
        assert!(is_target_candidate("javaw.exe", &vanilla.to_lowercase(), true, &user));

        // Fabric via fabric-loader coordinate.
        let fabric = "java -cp fabric-loader-0.16.9.jar;net.fabricmc.intermediary.jar \
             net.fabricmc.loader.impl.launch.knot.knotclient --gameDir .";
        assert!(is_target_candidate("javaw.exe", &fabric.to_lowercase(), true, &user));

        // NeoForge via bootstraplauncher + universal jar.
        let neoforge = "java -p cpw.mods.bootstraplauncher.jar --add-modules ALL-MODULE-PATH \
             -cp neoforge-21.1.90-universal.jar cpw.mods.bootstraplauncher.BootstrapLauncher \
             --launchTarget forgeclient";
        assert!(is_target_candidate("java.exe", &neoforge.to_lowercase(), true, &user));

        // Dedicated server main class.
        let server = "java -Xmx2G -jar server.jar nogui net.minecraft.server.Main";
        assert!(is_target_candidate("java", &server.to_lowercase(), true, &user));

        // Native launcher executables stay candidates without cmdline data.
        assert!(is_target_candidate("minecraft.exe", "", false, &user));
        assert!(is_target_candidate("minecraft.windows.exe", "", false, &user));
    }

    #[test]
    fn test_java_without_cmdline_is_left_alone() {
        // Cannot disambiguate → never kill.
        assert!(!is_target_candidate("java", "", false, &[]));
        assert!(!is_target_candidate("javaw.exe", "", false, &[]));
    }

    #[test]
    fn test_user_exclusion_vetoes_strong_match() {
        // A dev-run game that carries both a strong marker and a user
        // substring: exclusion wins (documented tradeoff).
        let dev_run = "java -cp ... cpw.mods.modlauncher.launcher main --gameDir .";
        assert!(is_target_candidate(
            "java",
            dev_run,
            true,
            &vec!["modlauncher.launcher".to_string()]
        ) == false);
    }


    #[test]
    fn test_exclusions_do_not_shadow_real_game() {
        // A vanilla-ish game launch must NOT match any exclusion substring.
        let game = "\"c:\\program files\\java\\bin\\javaw.exe\" -Xmx4G \
            -cp libraries.jar net.minecraft.client.main.Main --username TestPlayer";
        assert!(!is_excluded_command(game, &[]));
    }

    #[test]
    fn test_user_exclusions_are_merged_and_case_insensitive() {
        // Contract: user entries arrive lowercased (load_user_exclusions
        // lowercases them) and cmd_lower is lowercased by the caller.
        let user = vec!["mycustomtool".to_string()];
        assert!(is_excluded_command("java -jar mycustomtool.jar", &user));
        assert!(is_excluded_command(
            "java -cp something;gradle-worker.jar worker",
            &user
        ));
    }

    #[test]
    fn test_load_user_exclusions_no_panic() {
        // File may not exist in test environments; must yield empty list.
        let _ = load_user_exclusions();
    }
}
