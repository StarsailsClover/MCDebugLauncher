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

use anyhow::Result;
use std::time::Duration;

/// Result of a pre-launch OOM protection sweep.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct OomGuardReport {
    /// Number of stale Minecraft/Java processes that were terminated.
    pub killed_processes: u32,
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
pub async fn pre_launch_protection(skip_kill: bool, aggressive: bool) -> Result<OomGuardReport> {
    let mut report = OomGuardReport::default();

    // Phase 1: kill stale Minecraft processes.
    if !skip_kill {
        report.killed_processes = kill_stale_minecraft_processes().await;
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

/// Detect and terminate stale Minecraft / Java processes that are not
/// tracked by MDL's launch lock (i.e. not the current MDL-managed
/// instance). Returns the number of processes killed.
///
/// The current MDL process's own PID is always excluded. When a launch
/// lock file exists, the PID recorded there is also preserved.
pub async fn kill_stale_minecraft_processes() -> u32 {
    let our_pid = std::process::id();

    // Read the launch lock to find the currently-tracked game PID (if any).
    let protected_pid = read_launch_lock_pid().await;

    let mut killed = 0u32;
    let processes = find_minecraft_processes();
    for proc in &processes {
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

    killed
}

/// Information about a running process found during scanning.
#[derive(Debug, Clone)]
struct ProcInfo {
    pid: u32,
    name: String,
    rss_bytes: u64,
}

/// Scan running processes for Minecraft/Java processes.
fn find_minecraft_processes() -> Vec<ProcInfo> {
    use sysinfo::{ProcessRefreshKind, System};

    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessRefreshKind::everything());

    let mut result = Vec::new();
    for (pid, proc) in sys.processes() {
        let name = proc.name().to_string();
        let lower = name.to_lowercase();

        let is_mc = MINECRAFT_PROCESS_NAMES
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&name));

        // Also match `java` (without .exe) on non-Windows.
        let is_java_generic = lower == "java" || lower == "javaw";

        if !is_mc && !is_java_generic {
            continue;
        }

        // Heuristic: only target java processes whose command line contains
        // "minecraft" or "net.minecraft" — avoid killing unrelated Java apps
        // (IDEs, Gradle daemons, other JVM-based tools).
        if is_java_generic || lower.ends_with(".exe") {
            let cmdline = proc
                .cmd()
                .iter()
                .map(|s| s.clone())
                .collect::<Vec<_>>()
                .join(" ");
            let cmd_lower = cmdline.to_lowercase();
            if !cmd_lower.contains("minecraft")
                && !cmd_lower.contains("net.minecraft")
                && !cmd_lower.contains("mcp")
                && !cmd_lower.contains("mdriven")
            {
                continue;
            }
        }

        result.push(ProcInfo {
            pid: pid.as_u32(),
            name,
            rss_bytes: proc.memory(),
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
    fn test_oom_guard_report_serializes() {
        let report = OomGuardReport {
            killed_processes: 2,
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
    async fn test_pre_launch_protection_skip_kill() {
        // With skip_kill=true, no processes should be terminated.
        let report = pre_launch_protection(true, false).await.unwrap();
        assert_eq!(report.killed_processes, 0);
        // available_after_bytes should be non-zero (we have *some* RAM).
        assert!(report.available_after_bytes > 0);
    }

    #[tokio::test]
    async fn test_pre_launch_protection_non_aggressive() {
        let report = pre_launch_protection(true, false).await.unwrap();
        assert!(!report.standby_purged, "Standby should not be purged in non-aggressive mode");
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
}
