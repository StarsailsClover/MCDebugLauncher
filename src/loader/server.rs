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
    /// RCON port written into server.properties at create time
    /// (v26.2-alpha.7). None for servers created before alpha.7.
    #[serde(default)]
    pub rcon_port: Option<u16>,
    /// Generated RCON password (v26.2-alpha.7). Kept in server.json so MDL
    /// can drive the console programmatically without user secrets.
    #[serde(default)]
    pub rcon_password: Option<String>,
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
    crate::util::jsonio::parse_sync(&path, "server metadata")
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
    crate::util::validate::validate_name(name)?;
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

    // v26.2-alpha.7: enable RCON with a generated password so MDL can stop
    // the server gracefully (world save) and run console commands for
    // automated testing (`mdl server cmd`).
    let rcon_port: u16 = 25575;
    let rcon_password = generate_rcon_password();

    // Minimal default properties; users can edit freely.
    if !dir.join("server.properties").exists() {
        fs::write(
            dir.join("server.properties"),
            format!(
                "server-port=25565\nmotd=MDL managed server\nonline-mode=false\n\
                 enable-rcon=true\nrcon.port={}\nrcon.password={}\n",
                rcon_port, rcon_password
            ),
        )
        .await?;
    }

    // Persist metadata.
    let info = ServerInfo {
        name: name.to_string(),
        version: version_info.id.clone(),
        memory: memory.map(str::to_string),
        rcon_port: Some(rcon_port),
        rcon_password: Some(rcon_password),
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

/// Wait until the server finishes booting by tailing server.log for the
/// vanilla readiness line: "Done (X.XXXs)! For help, type \"help\"".
/// Returns Ok(()) when ready, Err on timeout. Poll interval is 1s.
pub async fn wait_for_ready(dir: &Path, timeout_secs: u64) -> Result<()> {
    use tokio::io::AsyncSeekExt;
    let log_path = dir.join("server.log");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let mut offset: u64 = 0;
    let mut buf = Vec::new();

    loop {
        if let Ok(mut f) = fs::File::open(&log_path).await {
            // Read only newly appended bytes each round.
            if f.seek(std::io::SeekFrom::Start(offset)).await.is_ok() {
                buf.clear();
                if tokio::io::AsyncReadExt::read_to_end(&mut f, &mut buf).await.is_ok() {
                    offset += buf.len() as u64;
                    let text = String::from_utf8_lossy(&buf);
                    for line in text.lines() {
                        if line.contains("Done (") && line.contains(")!") {
                            tracing::info!("Server ready: {}", line.trim());
                            return Ok(());
                        }
                    }
                }
            }
        }

        // Bail out early if the process died before becoming ready.
        if let Some(pid_raw) = read_pid_file(dir) {
            if !is_pid_alive(pid_raw) {
                anyhow::bail!("Server process exited before reaching ready state");
            }
        } else {
            anyhow::bail!("Server PID file disappeared before ready state");
        }

        if std::time::Instant::now() >= deadline {
            anyhow::bail!("Timed out after {}s waiting for server 'Done' line", timeout_secs);
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

fn read_pid_file(dir: &Path) -> Option<u32> {
    let raw = std::fs::read_to_string(dir.join("runtime").join("pid")).ok()?;
    raw.trim().parse().ok()
}

/// RCON endpoint for a managed server, if configured.
pub fn rcon_addr(info: &ServerInfo) -> Option<String> {
    let port = info.rcon_port?;
    let password = info.rcon_password.as_deref()?;
    if password.is_empty() {
        return None;
    }
    Some(format!("127.0.0.1:{}|{}", port, password))
}

/// Run one console command on a managed server via RCON.
pub async fn run_console_command(info: &ServerInfo, command: &str) -> Result<String> {
    let addr = rcon_addr(info)
        .ok_or_else(|| anyhow::anyhow!(
            "Server '{}' has no RCON config (re-create it or enable-rcon manually in server.properties)",
            info.name
        ))?;
    let (endpoint, password) = addr.split_once('|').context("Invalid RCON address")?;
    crate::loader::rcon::run_command(endpoint, password, command).await
}

/// Stop a running server. v26.2-alpha.7 prefers a graceful shutdown via the
/// RCON `stop` command (world save + clean exit), waiting up to 20s, and
/// falls back to forceful termination when RCON is unavailable or times out.
pub async fn stop_server(info: &ServerInfo) -> Result<()> {
    let dir = info.dir()?;
    let Some(pid) = running_pid(&dir) else {
        anyhow::bail!("Server '{}' is not running", info.name);
    };

    // Graceful path: RCON stop, then poll for exit.
    if rcon_addr(info).is_some() {
        match run_console_command(info, "stop").await {
            Ok(_) => {
                tracing::info!("Sent graceful stop via RCON; waiting for exit...");
                for _ in 0..20 {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    if !is_pid_alive(pid) {
                        let _ = fs::remove_file(dir.join("runtime").join("pid")).await;
                        tracing::info!("Server '{}' stopped gracefully", info.name);
                        return Ok(());
                    }
                }
                tracing::warn!("Server did not exit within 20s of RCON stop; falling back to kill");
            }
            Err(e) => {
                tracing::warn!("RCON stop failed ({}); falling back to kill", e);
            }
        }
    }

    // Forceful fallback.
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

/// Generate a random-looking RCON password (24 hex chars). Entropy comes
/// from the current time (nanos), the process id and an atomic counter,
/// hashed through SHA1 — sufficient for a localhost-only control channel
/// that never leaves the machine. Public since v26.3-alpha.5 for
/// `mdl server rotate-rcon`.
pub fn generate_rcon_password() -> String {
    use sha1::{Digest, Sha1};
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let mut hasher = Sha1::new();
    hasher.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos().to_le_bytes())
            .unwrap_or([0u8; 16]),
    );
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    let digest = hasher.finalize();
    // Hex-encode the first 12 bytes -> 24 hex chars.
    let hex: String = digest[..12].iter().map(|b| format!("{:02x}", b)).collect();
    hex
}

/// Rotate a managed server's RCON password (v26.3-alpha.5): generates a
/// fresh password, updates server.properties AND server.json atomically
/// enough for MDL's purposes (both writes happen back-to-back; the props
/// file is what the running server reads at boot).
///
/// Returns the new password so the CLI can decide whether to display it.
pub fn rotate_rcon_password(info: &mut ServerInfo) -> Result<String> {
    let new_pw = generate_rcon_password();
    let dir = info.dir()?;
    let props_path = dir.join("server.properties");
    let mut props = crate::loader::props::PropertiesFile::load(&props_path)
        .with_context(|| {
            "server.properties missing — enable RCON manually or re-create the server".to_string()
        })?;
    props.set("enable-rcon", "true");
    if info.rcon_port.is_none() {
        info.rcon_port = Some(25575);
        props.set("rcon.port", "25575");
    }
    props.set("rcon.password", &new_pw);
    props.save(&props_path)?;

    info.rcon_port = Some(props.get("rcon.port").and_then(|v| v.parse().ok()).unwrap_or(25575));
    info.rcon_password = Some(new_pw.clone());
    let meta = dir.join("server.json");
    std::fs::write(&meta, serde_json::to_string_pretty(info)?)
        .with_context(|| format!("Failed to update {}", meta.display()))?;
    Ok(new_pw)
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
            rcon_port: Some(25575),
            rcon_password: Some("abc123".into()),
        };
        let raw = serde_json::to_string(&info).unwrap();
        let back: ServerInfo = serde_json::from_str(&raw).unwrap();
        assert_eq!(back.name, "demo");
        assert_eq!(back.memory.as_deref(), Some("4G"));
        assert_eq!(back.rcon_port, Some(25575));
        assert_eq!(back.rcon_password.as_deref(), Some("abc123"));
    }

    #[test]
    fn test_server_info_backward_compat() {
        // Pre-alpha.7 server.json lacks the RCON fields; defaults must apply.
        let raw = r#"{"name":"old","version":"1.21.4","memory":null}"#;
        let back: ServerInfo = serde_json::from_str(raw).unwrap();
        assert!(back.rcon_port.is_none());
        assert!(back.rcon_password.is_none());
    }

    #[test]
    fn test_generate_rcon_password() {
        let a = generate_rcon_password();
        let b = generate_rcon_password();
        assert_eq!(a.len(), 24);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_wait_for_ready_timeout_on_missing_log() {
        // No log file + no pid file -> immediate error, not a hang.
        let dir = tempfile::tempdir().unwrap();
        let result = tokio_test::block_on(wait_for_ready(dir.path(), 2));
        assert!(result.is_err());
    }
}
