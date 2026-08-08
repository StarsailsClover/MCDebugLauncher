// Game window discovery for Windows.
//
// MDL tries to launch Minecraft with `--title "MDL: <instance>"`, but some
// modded/older clients ignore that argument entirely. The reliable anchor is
// therefore the game process PID, which MDL records in `runtime/pid` when it
// spawns the game. Discovery is PID-first with the title prefix as a fallback.

use anyhow::{Context, Result};
use serde::Serialize;
use windows_capture::window::Window;

/// Information about a discovered game window.
#[derive(Debug, Clone, Serialize)]
pub struct GameWindowInfo {
    /// Instance name (parsed from the title when possible, else from the
    /// caller-provided mapping).
    pub instance: String,
    pub title: String,
    pub pid: Option<u32>,
    pub width: i32,
    pub height: i32,
    /// Opaque window handle (HWND) as an integer, for diagnostics.
    pub handle: u64,
}

/// A resolved game window, owning the underlying capture-capable handle.
pub struct ResolvedWindow {
    pub info: GameWindowInfo,
    pub window: Window,
}

/// The title prefix MDL gives launched game windows (best-effort; may be
/// ignored by the game, in which case PID matching is used).
pub const TITLE_PREFIX: &str = "MDL: ";

/// Extract the instance name from a window title like "MDL: name [mods]".
fn instance_from_title(title: &str) -> Option<String> {
    let rest = title.strip_prefix(TITLE_PREFIX)?;
    let name = match rest.find('[') {
        Some(idx) => &rest[..idx],
        None => rest,
    };
    let name = name.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn window_info(window: &Window, instance: String) -> Option<GameWindowInfo> {
    let title = window.title().ok()?;
    if !window.is_valid() {
        return None;
    }
    let pid = window.process_id().ok();
    let (width, height) = match window.rect() {
        Ok(r) => ((r.right - r.left) as i32, (r.bottom - r.top) as i32),
        Err(_) => (0, 0),
    };
    Some(GameWindowInfo {
        instance,
        title,
        pid,
        width,
        height,
        handle: window.as_raw_hwnd() as u64,
    })
}

/// Find the window owned by a specific process, matching PID first.
fn find_by_pid(pid: u32) -> Option<(Window, GameWindowInfo)> {
    let windows = Window::enumerate().ok()?;
    for window in windows {
        if window.process_id().ok() == Some(pid) {
            if !window.is_valid() {
                continue;
            }
            let title = window.title().unwrap_or_default();
            let instance = instance_from_title(&title).unwrap_or_else(|| format!("pid-{}", pid));
            if let Some(info) = window_info(&window, instance) {
                return Some((window, info));
            }
        }
    }
    None
}

/// Enumerate windows that look like MDL-launched Minecraft windows, matched
/// either by the title prefix or by the owning process being a known running
/// instance (passed as `known_pids`: instance name -> pid).
pub fn list_mdl_windows(known_pids: &[(String, u32)]) -> Vec<GameWindowInfo> {
    let mut results = Vec::new();
    let windows = match Window::enumerate() {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("Failed to enumerate windows: {:?}", e);
            return results;
        }
    };

    for window in windows {
        let title = match window.title() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if !window.is_valid() {
            continue;
        }
        let window_pid = window.process_id().ok();

        // Match 1: title carries the MDL prefix.
        if let Some(instance) = instance_from_title(&title) {
            if let Some(info) = window_info(&window, instance) {
                results.push(info);
            }
            continue;
        }

        // Match 2: owning process is a known running MDL instance. This
        // covers games that ignored the --title argument.
        if let Some(pid) = window_pid {
            if let Some((instance, _)) = known_pids.iter().find(|(_, p)| *p == pid) {
                if let Some(info) = window_info(&window, instance.clone()) {
                    results.push(info);
                }
            }
        }
    }
    results
}

/// Locate the game window for a specific instance.
///
/// Resolution order:
/// 1. Window owned by `pid` (the instance's recorded game process). This is
///    the reliable path — it works even when the game ignored `--title`.
/// 2. Any window whose title parses to the instance name.
pub fn find_for_instance(instance_name: &str, pid: Option<u32>) -> Result<ResolvedWindow> {
    // Path 1: PID match (robust regardless of window title).
    if let Some(pid) = pid {
        if let Some((window, mut info)) = find_by_pid(pid) {
            // Prefer the real instance name over a synthesized pid-N label.
            if info.instance.starts_with("pid-") {
                info.instance = instance_name.to_string();
            }
            return Ok(ResolvedWindow { info, window });
        }
    }

    // Path 2: title match.
    let windows = Window::enumerate().context("Failed to enumerate windows")?;
    for window in windows {
        let title = match window.title() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let Some(instance) = instance_from_title(&title) else {
            continue;
        };
        if instance != instance_name || !window.is_valid() {
            continue;
        }
        if let Some(info) = window_info(&window, instance) {
            return Ok(ResolvedWindow { info, window });
        }
    }

    anyhow::bail!(
        "No game window found for instance '{}' (pid {}). Is it running and not minimized?",
        instance_name,
        pid.map(|p| p.to_string()).unwrap_or_else(|| "?".into())
    )
}

/// Collect `(instance_name, pid)` pairs for all instances whose game process
/// is currently running, by reading each instance's `runtime/pid` file.
/// Used so window discovery can match games that ignored the `--title` flag.
pub fn collect_running_pids() -> Vec<(String, u32)> {
    let instances_dir = match crate::util::paths::get_instances_dir() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut results = Vec::new();
    let entries = match std::fs::read_dir(&instances_dir) {
        Ok(e) => e,
        Err(_) => return results,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let pid_file = entry.path().join("runtime").join("pid");
        if let Ok(content) = std::fs::read_to_string(&pid_file) {
            if let Ok(pid) = content.trim().parse::<u32>() {
                results.push((name, pid));
            }
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_from_title() {
        assert_eq!(
            instance_from_title("MDL: bc-test [Fabric API, ModX]").as_deref(),
            Some("bc-test")
        );
        assert_eq!(
            instance_from_title("MDL: vanilla-test").as_deref(),
            Some("vanilla-test")
        );
        assert_eq!(instance_from_title("Minecraft 1.21"), None);
        assert_eq!(instance_from_title("MDL: "), None);
        assert_eq!(instance_from_title(""), None);
    }
}
