// DLL / executable injector (Alpha 7, Windows).
//
// Provides `mdl inject <pid|process> --dll <path>` which injects a DLL into
// a target process using the classic CreateRemoteThread(LoadLibraryW)
// technique. This is the groundwork for the Aprism BE Native loader, which
// on Bedrock is loaded as a native library rather than a javaagent.
//
// Security: this is a local, explicit, user-invoked operation. The caller
// must already hold a PID (or exact process name) and the DLL must be on
// disk; MDL never downloads and injects on its own.

#[cfg(windows)]
pub use windows_impl::*;

#[cfg(windows)]
mod windows_impl {
    use anyhow::{bail, Context, Result};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    type HANDLE = *mut std::ffi::c_void;

    extern "system" {
        fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: i32, dwProcessId: u32) -> HANDLE;
        fn VirtualAllocEx(
            hProcess: HANDLE,
            lpAddress: *mut std::ffi::c_void,
            dwSize: usize,
            flAllocationType: u32,
            flProtect: u32,
        ) -> *mut std::ffi::c_void;
        fn WriteProcessMemory(
            hProcess: HANDLE,
            lpBaseAddress: *mut std::ffi::c_void,
            lpBuffer: *const std::ffi::c_void,
            nSize: usize,
            lpNumberOfBytesWritten: *mut usize,
        ) -> i32;
        fn CreateRemoteThread(
            hProcess: HANDLE,
            lpThreadAttributes: *const std::ffi::c_void,
            dwStackSize: usize,
            lpStartAddress: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
            lpParameter: *mut std::ffi::c_void,
            dwCreationFlags: u32,
            lpThreadId: *mut u32,
        ) -> HANDLE;
        fn WaitForSingleObject(hHandle: HANDLE, dwMilliseconds: u32) -> u32;
        fn GetExitCodeThread(hThread: HANDLE, lpExitCode: *mut u32) -> i32;
        fn CloseHandle(hObject: HANDLE) -> i32;
        fn LoadLibraryW(lpLibFileName: *const u16) -> HANDLE;
    }

    const PROCESS_CREATE_THREAD: u32 = 0x0002;
    const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
    const PROCESS_VM_OPERATION: u32 = 0x0008;
    const PROCESS_VM_WRITE: u32 = 0x0020;
    const MEM_COMMIT: u32 = 0x1000;
    const MEM_RESERVE: u32 = 0x2000;
    const PAGE_READWRITE: u32 = 0x04;

    // LoadLibraryW as a CreateRemoteThread-compatible entry point.
    unsafe extern "system" fn load_library_trampoline(lp: *mut std::ffi::c_void) -> u32 {
        LoadLibraryW(lp as *const u16) as usize as u32
    }

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Inject `dll_path` into the process identified by `pid`.
    pub fn inject_dll(pid: u32, dll_path: &std::path::Path) -> Result<()> {
        let abs = std::fs::canonicalize(dll_path)
            .with_context(|| format!("DLL not found: {}", dll_path.display()))?;
        let wide = to_wide(&abs.to_string_lossy());
        let byte_len = wide.len() * 2;

        unsafe {
            let proc = OpenProcess(
                PROCESS_CREATE_THREAD
                    | PROCESS_QUERY_INFORMATION
                    | PROCESS_VM_OPERATION
                    | PROCESS_VM_WRITE,
                0,
                pid,
            );
            if proc.is_null() {
                bail!(
                    "Failed to open process {} (insufficient privileges or process gone)",
                    pid
                );
            }

            let remote = VirtualAllocEx(
                proc,
                std::ptr::null_mut(),
                byte_len,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            );
            if remote.is_null() {
                CloseHandle(proc);
                bail!("VirtualAllocEx failed for process {}", pid);
            }

            let mut written: usize = 0;
            if WriteProcessMemory(
                proc,
                remote,
                wide.as_ptr() as *const _,
                byte_len,
                &mut written,
            ) == 0
            {
                CloseHandle(proc);
                bail!("WriteProcessMemory failed for process {}", pid);
            }

            let thread = CreateRemoteThread(
                proc,
                std::ptr::null(),
                0,
                load_library_trampoline,
                remote,
                0,
                std::ptr::null_mut(),
            );
            if thread.is_null() {
                CloseHandle(proc);
                bail!("CreateRemoteThread failed for process {}", pid);
            }

            WaitForSingleObject(thread, 10_000);
            let mut exit_code: u32 = 0;
            GetExitCodeThread(thread, &mut exit_code);
            CloseHandle(thread);
            CloseHandle(proc);

            if exit_code == 0 {
                bail!("Injected LoadLibraryW returned null (DLL failed to load in target)");
            }
            Ok(())
        }
    }

    /// Find the PID of the first process whose executable name matches.
    pub fn find_pid_by_name(name: &str) -> Result<u32> {
        use sysinfo::System;
        let mut sys = System::new_all();
        sys.refresh_processes();
        for (pid, proc) in sys.processes() {
            let exe = proc
                .exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            if exe.to_lowercase().ends_with(&name.to_lowercase()) {
                return Ok(pid.as_u32());
            }
        }
        bail!("No running process matches '{}'", name);
    }
}

#[cfg(not(windows))]
pub fn inject_dll(_pid: u32, _dll_path: &std::path::Path) -> anyhow::Result<()> {
    anyhow::bail!("DLL injection is supported only on Windows");
}

#[cfg(not(windows))]
pub fn find_pid_by_name(_name: &str) -> anyhow::Result<u32> {
    anyhow::bail!("Process lookup for injection is supported only on Windows");
}
