// TCP protocol client for the MDL companion mod.
//
// The companion mod runs inside the Minecraft process and exposes a local
// TCP server (JSON lines protocol). Because input is injected from inside
// the game process, operations never touch the user's real keyboard/mouse
// and work regardless of which window has focus.
//
// Protocol: newline-delimited JSON. MDL sends one request object per line
// and the mod answers with one response object per line:
//
//   -> {"cmd":"key","key":"w","action":"press"}
//   <- {"status":"ok"}
//
//   -> {"cmd":"status"}
//   <- {"status":"ok","in_world":true,"paused":false,...}
//
//   <- {"status":"error","message":"unknown key: xyz"}

use anyhow::{Context, Result, bail};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use super::COMPANION_PORT_FILE;

/// Default timeout for a single command round-trip.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Resolve the companion port for an instance.
///
/// The companion mod writes the port it actually bound to
/// `runtime/agent.port` after startup. We prefer that file; when absent we
/// fall back to the `mdl.agent.port` JVM property the launcher passed
/// (stored in `runtime/requested_port` by the launcher), and finally to the
/// default port.
pub async fn resolve_port(instance_dir: &Path) -> Result<u16> {
    let port_file = instance_dir.join("runtime").join(COMPANION_PORT_FILE);
    if port_file.exists() {
        let content = tokio::fs::read_to_string(&port_file)
            .await
            .context("Failed to read companion port file")?;
        if let Ok(port) = content.trim().parse::<u16>() {
            return Ok(port);
        }
    }

    let requested = instance_dir.join("runtime").join("requested_port");
    if requested.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&requested).await {
            if let Ok(port) = content.trim().parse::<u16>() {
                return Ok(port);
            }
        }
    }

    Ok(super::DEFAULT_COMPANION_PORT)
}

/// Check whether the companion mod server is reachable for this instance.
pub async fn is_available(instance_dir: &Path) -> bool {
    let port = match resolve_port(instance_dir).await {
        Ok(p) => p,
        Err(_) => return false,
    };
    let addr = format!("127.0.0.1:{}", port);
    let probe = tokio::time::timeout(
        Duration::from_millis(750),
        TcpStream::connect(&addr),
    )
    .await;
    matches!(probe, Ok(Ok(_)))
}

/// Send a single command to the companion mod and return the parsed
/// response. Opens a fresh connection per call — the companion keeps no
/// session state, so this is safe and keeps each request isolated.
pub async fn send_command(instance_dir: &Path, command: Value) -> Result<Value> {
    let port = resolve_port(instance_dir).await?;
    let addr = format!("127.0.0.1:{}", port);

    let stream = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(&addr))
        .await
        .context("Timeout connecting to companion mod")?
        .with_context(|| {
            format!(
                "Cannot connect to the MDL companion mod at {}. \
                 The instance may not be running, or the companion mod \
                 (mdl-agent-companion) is not installed/enabled.",
                addr
            )
        })?;
    stream.set_nodelay(true)?;

    let (read_half, mut write_half) = stream.into_split();
    let mut payload = serde_json::to_string(&command)?;
    payload.push('\n');
    write_half.write_all(payload.as_bytes()).await?;

    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    let read_result = tokio::time::timeout(DEFAULT_TIMEOUT, reader.read_line(&mut line)).await;
    let n = match read_result {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => return Err(e).context("Failed to read companion response"),
        Err(_) => bail!("Timeout waiting for companion mod response"),
    };
    if n == 0 {
        bail!("Companion mod closed the connection without responding");
    }

    let response: Value = serde_json::from_str(line.trim())
        .context("Invalid JSON response from companion mod")?;

    if response.get("status").and_then(Value::as_str) == Some("error") {
        let msg = response
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown companion error");
        bail!("Companion mod error: {}", msg);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// High-level command helpers
// ---------------------------------------------------------------------------

/// Query game state from the companion (in-world flag, pause state, screen,
/// player position, FPS, ...).
pub async fn game_status(instance_dir: &Path) -> Result<Value> {
    send_command(instance_dir, json!({"cmd": "status"})).await
}

/// Press, release, or tap a keyboard key inside the game.
///
/// `key` uses Minecraft/KeyBind names ("w", "a", "space", "escape",
/// "inventory" (E), ...). `action` is one of press | release | tap.
/// `hold_ms` only applies to `tap` and sets how long the key stays down.
pub async fn key_input(
    instance_dir: &Path,
    key: &str,
    action: &str,
    hold_ms: Option<u64>,
) -> Result<Value> {
    let mut cmd = json!({"cmd": "key", "key": key, "action": action});
    if let Some(ms) = hold_ms {
        cmd["hold_ms"] = json!(ms);
    }
    send_command(instance_dir, cmd).await
}

/// Rotate the player's view to an absolute yaw/pitch (degrees), or adjust
/// the current rotation by a delta when `relative` is true.
pub async fn look(
    instance_dir: &Path,
    yaw: f32,
    pitch: f32,
    relative: bool,
) -> Result<Value> {
    send_command(
        instance_dir,
        json!({"cmd": "look", "yaw": yaw, "pitch": pitch, "relative": relative}),
    )
    .await
}

/// Perform a mouse action. `button` is left | right | middle; `action` is
/// press | release | tap; optional `x`/`y` are GUI coordinates (pixels at
/// the game's GUI scale, origin top-left). When omitted, the game's current
/// cursor position is used.
pub async fn mouse_input(
    instance_dir: &Path,
    button: &str,
    action: &str,
    x: Option<f64>,
    y: Option<f64>,
    hold_ms: Option<u64>,
) -> Result<Value> {
    let mut cmd = json!({"cmd": "click", "button": button, "action": action});
    if let Some(x) = x {
        cmd["x"] = json!(x);
    }
    if let Some(y) = y {
        cmd["y"] = json!(y);
    }
    if let Some(ms) = hold_ms {
        cmd["hold_ms"] = json!(ms);
    }
    send_command(instance_dir, cmd).await
}

/// Scroll the mouse wheel by `amount` steps (positive = up/away).
pub async fn scroll(instance_dir: &Path, amount: f64) -> Result<Value> {
    send_command(instance_dir, json!({"cmd": "scroll", "amount": amount})).await
}

/// Send a chat message or server command (leading "/" preserved and
/// interpreted as a command by the game).
pub async fn chat(instance_dir: &Path, message: &str) -> Result<Value> {
    send_command(instance_dir, json!({"cmd": "chat", "message": message})).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_resolve_port_prefers_port_file() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        tokio::fs::create_dir_all(&runtime).await.unwrap();

        // no files -> default
        assert_eq!(
            resolve_port(dir.path()).await.unwrap(),
            super::super::DEFAULT_COMPANION_PORT
        );

        // requested_port fallback
        tokio::fs::write(runtime.join("requested_port"), "25591")
            .await
            .unwrap();
        assert_eq!(resolve_port(dir.path()).await.unwrap(), 25591);

        // agent.port wins
        tokio::fs::write(runtime.join("agent.port"), "25592")
            .await
            .unwrap();
        assert_eq!(resolve_port(dir.path()).await.unwrap(), 25592);
    }

    #[tokio::test]
    async fn test_send_command_error_response() {
        // Stand up a fake companion that always answers with an error.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut line = String::new();
                let mut reader = BufReader::new(&mut socket);
                let _ = reader.read_line(&mut line).await;
                let _ = socket
                    .write_all(b"{\"status\":\"error\",\"message\":\"test failure\"}\n")
                    .await;
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        tokio::fs::create_dir_all(&runtime).await.unwrap();
        tokio::fs::write(runtime.join("agent.port"), port.to_string())
            .await
            .unwrap();

        let err = send_command(dir.path(), json!({"cmd":"status"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("test failure"));
    }

    #[tokio::test]
    async fn test_send_command_success_response() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut line = String::new();
                let mut reader = BufReader::new(&mut socket);
                let _ = reader.read_line(&mut line).await;
                let response = format!(
                    "{{\"status\":\"ok\",\"echo\":{}}}\n",
                    line.trim()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        tokio::fs::create_dir_all(&runtime).await.unwrap();
        tokio::fs::write(runtime.join("agent.port"), port.to_string())
            .await
            .unwrap();

        let resp = send_command(dir.path(), json!({"cmd":"status"})).await.unwrap();
        assert_eq!(resp["status"], "ok");
        assert_eq!(resp["echo"]["cmd"], "status");
    }
}
