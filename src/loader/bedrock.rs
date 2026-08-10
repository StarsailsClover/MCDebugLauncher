// Minecraft Bedrock Edition support (Alpha 7, lifecycle extended in v26.1-alpha.3).
//
// MDL supports the Bedrock Dedicated Server (BDS) for Windows: download the
// official zip, extract into an instance directory, then manage the full
// lifecycle (launch with EULA + log capture + PID tracking, status, stop).
// The BE *client* on Windows is a UWP app that cannot be freely downloaded or
// launched by a third-party launcher, so client support is limited to
// injection-based loaders (see util::injector, the groundwork for Aprism BE
// Native).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Candidate Windows BDS versions to probe, newest first. The official page
/// (https://www.minecraft.net/en-us/download/server/bedrock) is SPA-heavy, so
/// MDL probes the stable direct-link pattern for a known-good version.
pub const BDS_CANDIDATE_VERSIONS: &[&str] = &[
    "1.26.43.1",
    "1.21.95.01",
    "1.21.90.03",
    "1.21.70.03",
];

pub fn bds_url_for(version: &str) -> String {
    format!(
        "https://www.minecraft.net/bedrockdedicatedserver/bin-win/bedrock-server-{}.zip",
        version
    )
}

/// Probe candidate versions and return the newest one that responds 200.
/// Falls back to the first candidate when probing is not possible.
pub async fn latest_bds_url() -> (String, String) {
    if let Ok(client) = crate::util::http::create_http_client() {
        for v in BDS_CANDIDATE_VERSIONS {
            let url = bds_url_for(v);
            if let Ok(resp) = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                client.head(&url).send(),
            )
            .await
            {
                if let Ok(r) = resp {
                    if r.status().is_success() {
                        return (v.to_string(), url);
                    }
                }
            }
        }
    }
    let v = BDS_CANDIDATE_VERSIONS[0];
    (v.to_string(), bds_url_for(v))
}

/// Download and extract the Bedrock Dedicated Server into `dir`.
pub async fn install_bds(dir: &Path) -> Result<PathBuf> {
    let (version, url) = latest_bds_url().await;
    std::fs::create_dir_all(dir)?;

    let zip_path = dir.join(format!("bedrock-server-{}.zip", version));
    if !zip_path.exists() {
        crate::version::downloader::download_file(&url, &zip_path, None).await?;
    }

    let extract_dir = dir.join("server");
    crate::util::archive::extract_zip(&zip_path, &extract_dir).await?;
    Ok(extract_dir)
}

/// Write `eula=true` so the BDS does not exit on first run. BDS generates an
/// `eula.txt` on its first launch and refuses to start until it is accepted;
/// MDL accepts it up front (the operator opted into running a server).
pub fn accept_eula(server_dir: &Path) -> Result<()> {
    let eula = server_dir.join("eula.txt");
    std::fs::write(&eula, "eula=true
").context("Failed to write eula.txt")?;
    Ok(())
}

/// Directory holding BDS runtime state (PID file).
pub fn runtime_dir(server_dir: &Path) -> PathBuf {
    server_dir.join("runtime")
}

/// Read the PID of a running BDS, if any (validates liveness).
pub fn running_bds_pid(server_dir: &Path) -> Option<u32> {
    let pid_file = runtime_dir(server_dir).join("pid");
    let raw = std::fs::read_to_string(&pid_file).ok()?;
    let pid: u32 = raw.trim().parse().ok()?;
    if crate::loader::server::is_pid_alive(pid) {
        Some(pid)
    } else {
        let _ = std::fs::remove_file(&pid_file);
        None
    }
}

/// Launch the extracted BDS: accept the EULA, capture output to
/// `bedrock_server.log`, record the PID, and return the child PID.
pub fn launch_bds(server_dir: &Path) -> Result<u32> {
    let exe = server_dir.join("bedrock_server.exe");
    if !exe.exists() {
        anyhow::bail!("bedrock_server.exe not found in {}", server_dir.display());
    }
    if running_bds_pid(server_dir).is_some() {
        anyhow::bail!("BDS is already running for {}", server_dir.display());
    }
    accept_eula(server_dir)?;

    // Capture server output so it can be inspected after the fact.
    let log_path = server_dir.join("bedrock_server.log");
    let log = std::fs::File::create(&log_path)
        .with_context(|| format!("Failed to create {}", log_path.display()))?;
    let log_err = log.try_clone()?;

    // Clear the inherit flag on the console/pipe handles so the BDS child
    // does not hold the launcher's pipes open (the calling shell would
    // otherwise hang on the pipe until the server exits). Same fix as the
    // JE dedicated server and the detached game launcher.
    crate::loader::server::clear_stdio_inherit_flags();

    let child = std::process::Command::new(&exe)
        .current_dir(server_dir)
        .stdin(std::process::Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .context("Failed to spawn bedrock_server.exe")?;
    let pid = child.id();

    let rt = runtime_dir(server_dir);
    std::fs::create_dir_all(&rt)?;
    std::fs::write(rt.join("pid"), pid.to_string())?;
    tracing::info!("BDS started (PID {}) - log: {}", pid, log_path.display());
    Ok(pid)
}

/// Stop a running BDS by killing its process tree. Returns the killed PID.
pub fn stop_bds(server_dir: &Path) -> Result<u32> {
    let Some(pid) = running_bds_pid(server_dir) else {
        anyhow::bail!("BDS is not running for {}", server_dir.display());
    };
    crate::loader::server::kill_pid(pid)?;
    let _ = std::fs::remove_file(runtime_dir(server_dir).join("pid"));
    tracing::info!("BDS stopped (PID {})", pid);
    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bds_url_for() {
        let url = bds_url_for("1.26.43.1");
        assert!(url.ends_with("bedrock-server-1.26.43.1.zip"));
    }
}
