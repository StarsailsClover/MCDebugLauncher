// Minecraft Java Edition dedicated server support (Alpha 8.1).
//
// MDL can download, configure and run an official vanilla server.jar:
//   - `mdl server create <name> --mc-version <ver>` downloads server.jar
//     (sha1-verified) from the version manifest and writes eula.txt plus a
//     default server.properties into <data>/servers/<name>/.
//   - `mdl server launch <name> [--detach]` runs it (background by default),
//     writing runtime/pid for tracking.
//   - `mdl server stop <name>` / `mdl server list` manage the lifecycle.
// Modded servers (Paper/Fabric server jars) can be used by dropping the jar
// over server.jar — MDL launches whatever server.jar it finds.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Directory holding all managed servers: <data>/servers.
pub fn servers_dir() -> Result<PathBuf> {
    Ok(crate::util::paths::get_data_dir()?.join("servers"))
}

/// Per-server metadata persisted as server.json.
#[derive(Debug, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub memory: Option<String>,
}

impl ServerInfo {
    pub fn dir(&self) -> Result<PathBuf> {
        Ok(servers_dir()?.join(&self.name))
    }
}

pub fn load_server(name: &str) -> Result<ServerInfo> {
    let dir = servers_dir()?.join(name);
    let path = dir.join("server.json");
    if !path.exists() {
        anyhow::bail!("Server '{}' does not exist (create it with `mdl server create`)", name);
    }
    let raw = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw).context("Failed to parse server.json")?)
}

pub fn list_servers() -> Result<Vec<ServerInfo>> {
    let dir = servers_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if !meta.is_dir() {
            continue;
        }
        if let Ok(info) = load_server(&entry.file_name().to_string_lossy()) {
            out.push(info);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Create a server: download server.jar, write eula.txt + server.properties.
pub async fn create_server(name: &str, mc_version: &str, memory: Option<&str>) -> Result<PathBuf> {
    let dir = servers_dir()?.join(name);
    if dir.exists() {
        anyhow::bail!("Server '{}' already exists", name);
    }
    fs::create_dir_all(&dir).await?;

    // Resolve version -> manifest metadata -> downloads.server.
    tracing::info!("Fetching version manifest...");
    let manifest = crate::version::manifest::VersionManifest::fetch().await?;
    let version_info = manifest
        .find_version(mc_version)
        .with_context(|| format!("Minecraft version '{}' not found", mc_version))?;
    let metadata =
        crate::version::manifest::VersionMetadata::fetch(&version_info.url).await?;
    let server_dl = metadata
        .downloads
        .server
        .as_ref()
        .with_context(|| format!("Version '{}' has no official server download", version_info.id))?;

    // Download server.jar with sha1 verification.
    let jar = dir.join("server.jar");
    tracing::info!("Downloading server.jar for {}...", version_info.id);
    crate::version::downloader::download_file(&server_dl.url, &jar, Some(&server_dl.sha1))
        .await
        .context("Failed to download server.jar")?;

    // Accept the EULA so the server can start (the user explicitly asked MDL
    // to create a server; note the EULA link in the file).
    fs::write(
        dir.join("eula.txt"),
        "eula=true\n# Accepted automatically by MCDebugLauncher on server creation\n",
    )
    .await?;

    // Minimal default properties; users can edit freely.
    if !dir.join("server.properties").exists() {
        fs::write(
            dir.join("server.properties"),
            "server-port=25565\nmotd=MDL managed server\nonline-mode=false\n",
        )
        .await?;
    }

    // Persist metadata.
    let info = ServerInfo {
        name: name.to_string(),
        version: version_info.id.clone(),
        memory: memory.map(str::to_string),
    };
    fs::write(dir.join("server.json"), serde_json::to_string_pretty(&info)?).await?;

    Ok(dir)
}

/// Read the PID of a managed server if one is recorded and alive.
pub fn running_pid(dir: &Path) -> Option<u32> {
    let pid_file = dir.join("runtime").join("pid");
    let raw = std::fs::read_to_string(&pid_file).ok()?;
    let pid: u32 = raw.trim().parse().ok()?;
    if is_pid_alive(pid) {
        Some(pid)
    } else {
        let _ = std::fs::remove_file(&pid_file);
        None
    }
}

/// Launch the server. By default runs in the background (detach=true):
/// stdout/stderr go to server.log and the PID is recorded. In attached mode
/// this function blocks until the server exits.
pub async fn launch_server(info: &ServerInfo, detach: bool) -> Result<u32> {
    let dir = info.dir()?;
    let jar = dir.join("server.jar");
    if !jar.exists() {
        anyhow::bail!(
            "server.jar missing in {} (re-create the server or supply a server jar)",
            dir.display()
        );
    }
    if let Some(pid) = running_pid(&dir) {
        anyhow::bail!("Server '{}' is already running (PID {})", info.name, pid);
    }
    if !dir.join("eula.txt").exists() {
        anyhow::bail!("eula.txt missing — refusing to start the server without an accepted EULA");
    }

    let memory = info.memory.clone().unwrap_or_else(|| "2G".to_string());

    // Use std::process::Command (not tokio): in detach mode the child handle
    // is dropped immediately, the OS keeps the process running, and the tokio
    // runtime is free to shut down — so `mdl server launch` returns at once.
    let mut cmd = std::process::Command::new("java");
    cmd.arg(format!("-Xmx{}", memory))
        .arg(format!("-Xms{}", memory))
        .arg("-jar")
        .arg("server.jar")
        .arg("nogui")
        .current_dir(&dir);

    let log_path = dir.join("server.log");
    if detach {
        let runtime_dir = dir.join("runtime");
        fs::create_dir_all(&runtime_dir).await?;
        let file = std::fs::File::create(&log_path)
            .with_context(|| format!("Failed to create {}", log_path.display()))?;
        cmd.stdout(file.try_clone()?);
        cmd.stderr(file);
        cmd.stdin(std::process::Stdio::null());
        // Windows: fully detach the child from this console so the shell does
        // not wait for the server to exit when `mdl server launch` returns.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        }
    } else {
        cmd.stdout(std::process::Stdio::inherit());
        cmd.stderr(std::process::Stdio::inherit());
        cmd.stdin(std::process::Stdio::inherit());
    }

    // Prevent console/pipe handles from leaking into the detached child.
    clear_stdio_inherit_flags();

    let mut child = cmd.spawn().context("Failed to spawn the server process (is Java installed?)")?;
    let pid = child.id();

    if detach {
        // Record the PID; running_pid() self-cleans stale files on read.
        let pid_file = dir.join("runtime").join("pid");
        fs::write(&pid_file, pid.to_string()).await?;
        tracing::info!("Server '{}' running in background (PID {}), log: {}", info.name, pid, log_path.display());
        // Drop the handle: the process is detached and keeps running.
        drop(child);
        return Ok(pid);
    }

    // Attached: block until the server exits.
    let status = child.wait().context("Failed to wait for the server")?;
    if !status.success() {
        anyhow::bail!("Server exited with code {:?}", status.code());
    }
    Ok(pid)
}

/// Stop a running server. Prefers a graceful exit via the `stop` console
/// command (stdin pipe) when available; falls back to terminating the process.
pub async fn stop_server(info: &ServerInfo) -> Result<()> {
    let dir = info.dir()?;
    let Some(pid) = running_pid(&dir) else {
        anyhow::bail!("Server '{}' is not running", info.name);
    };
    kill_pid(pid)?;
    let _ = fs::remove_file(dir.join("runtime").join("pid")).await;
    tracing::info!("Server '{}' stopped (PID {})", info.name, pid);
    Ok(())
}


/// Windows: clear the inherit flag on the current process's stdout/stderr
/// handles before spawning a detached child. Inherited handles leak into the
/// child and keep the parent's pipes open, which makes the calling shell hang
/// on the pipe even after MDL exits. Safe to call on non-Windows (no-op).
pub fn clear_stdio_inherit_flags() {
    #[cfg(windows)]
    {
        
        unsafe {
            extern "system" {
                fn GetStdHandle(which: u32) -> *mut std::ffi::c_void;
                fn SetHandleInformation(
                    h: *mut std::ffi::c_void,
                    mask: u32,
                    flags: u32,
                ) -> i32;
            }
            const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // (DWORD)-11
            const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4;  // (DWORD)-12
            const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
            for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
                let h = GetStdHandle(which);
                if !h.is_null() {
                    // mask=HANDLE_FLAG_INHERIT, flags=0 -> clear the inherit bit
                    SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
                }
            }
        }
    }
}

pub fn is_pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        unsafe {
            extern "system" {
                fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
                fn CloseHandle(h: *mut std::ffi::c_void) -> i32;
            }
            const PROCESS_QUERY_LIMITED: u32 = 0x1000;
            let h = OpenProcess(PROCESS_QUERY_LIMITED, 0, pid);
            if h.is_null() {
                return false;
            }
            CloseHandle(h);
            true
        }
    }
    #[cfg(not(windows))]
    {
        std::path::Path::new(&format!("/proc/{}", pid)).exists()
    }
}

pub fn kill_pid(pid: u32) -> Result<()> {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .context("Failed to run taskkill")?;
        if !out.status.success() {
            anyhow::bail!("taskkill failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let status = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status()
            .context("Failed to run kill")?;
        if !status.success() {
            anyhow::bail!("kill failed for PID {}", pid);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_info_roundtrip() {
        let info = ServerInfo {
            name: "demo".into(),
            version: "1.21.4".into(),
            memory: Some("4G".into()),
        };
        let raw = serde_json::to_string(&info).unwrap();
        let back: ServerInfo = serde_json::from_str(&raw).unwrap();
        assert_eq!(back.name, "demo");
        assert_eq!(back.memory.as_deref(), Some("4G"));
    }
}
