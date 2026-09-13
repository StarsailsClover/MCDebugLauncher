// Instance process lifecycle: graceful stop with force fallback
// (v26.6-alpha.2).
//
// Closes the lifecycle story v26.2 started (idle watchdog, launch lock):
// agents and operators can now stop a running game without reaching for
// taskkill by hand. Semantics mirror `mdl server stop`:
//   - graceful first: WM_CLOSE to the game window (Windows — Minecraft runs
//     its normal close path and saves the world) or SIGTERM (unix), then
//     poll for exit
//   - force fallback: taskkill /T /F (Windows) or SIGKILL (unix) when the
//     grace window lapses, or immediately under --force
//
// Security: local, explicit, user-invoked; the instance name resolves
// through the validated InstanceManager entry.

// GitHub@NDBlockConnect | BlockConnect@StarsailsClover

use anyhow::{Context, Result};
use std::path::Path;
use std::time::{Duration, Instant};

/// Grace window after the graceful signal before force-killing. Long enough
/// for Minecraft to save a small world, short enough for agent loops.
const GRACE_SECS: u64 = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct StopOutcome {
    pub pid: u32,
    /// True when the graceful signal alone was enough.
    pub graceful: bool,
}

/// Stop the instance's game process: graceful first, force fallback.
/// `force` skips the graceful phase entirely.
pub async fn stop_instance(instance_dir: &Path, force: bool) -> Result<StopOutcome> {
    let pid = crate::loader::server::running_pid(instance_dir)
        .ok_or_else(|| anyhow::anyhow!("Instance is not running (no live pid)"))?;

    if !force && graceful_signal(pid) {
        // Poll for exit within the grace window.
        let deadline = Instant::now() + Duration::from_secs(GRACE_SECS);
        while Instant::now() < deadline {
            if !crate::loader::server::is_pid_alive(pid) {
                cleanup_pid_file(instance_dir).await;
                return Ok(StopOutcome { pid, graceful: true });
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    // Force fallback (also the immediate path under --force).
    crate::loader::server::kill_pid(pid)
        .with_context(|| format!("Failed to force-kill game process {pid}"))?;
    cleanup_pid_file(instance_dir).await;
    Ok(StopOutcome { pid, graceful: false })
}

/// Send the graceful close signal for this platform. Returns false when
/// there is nothing to signal (no window on Windows).
#[allow(unused_variables)]
fn graceful_signal(pid: u32) -> bool {
    #[cfg(windows)]
    {
        crate::game::window::post_close_to_pid(pid)
    }
    #[cfg(not(windows))]
    {
        // SIGTERM lets the JVM run its shutdown hooks (world save).
        std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

async fn cleanup_pid_file(instance_dir: &Path) {
    let _ = tokio::fs::remove_file(instance_dir.join("runtime").join("pid")).await;
}
