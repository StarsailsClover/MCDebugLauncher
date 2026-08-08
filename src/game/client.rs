// HTTP client for the Despotes control mod (https://github.com/NDBlockConnect/Despotes).
//
// Despotes runs a local HTTP server inside the Minecraft process
// (127.0.0.1, default port 25585). MDL drives the game through it:
//
//   POST /despotes/v1/actions   -> key / type / move / look / click / use / screenshot
//   POST /despotes/v1/query     -> status / screen / inventory / pending
//   GET  /despotes/v1/screenshot-> binary image (png)
//   GET  /despotes/v1/status    -> JSON status
//
// Responses use the envelope:
//   { "ok": true,  "result": {...} }
//   { "ok": false, "error": { "code": ..., "message": ... } }
//
// Because Despotes injects input inside the game process, operations never
// touch the user's real keyboard/mouse and work without window focus.

use anyhow::{Context, Result, bail};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// Default timeout for a single command round-trip.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Resolve the Despotes HTTP port for an instance.
///
/// The launcher passes the port to the game via `-Ddespotes.port` and
/// records it in `runtime/despotes.port`. When that file is absent we fall
/// back to the Despotes default port.
pub async fn resolve_port(instance_dir: &Path) -> Result<u16> {
    let port_file = instance_dir.join("runtime").join(super::DESPOTES_PORT_FILE);
    if port_file.exists() {
        let content = tokio::fs::read_to_string(&port_file)
            .await
            .context("Failed to read Despotes port file")?;
        if let Ok(port) = content.trim().parse::<u16>() {
            return Ok(port);
        }
    }
    Ok(super::DEFAULT_DESPOTES_PORT)
}

/// Base URL of the Despotes HTTP server for this instance.
pub async fn base_url(instance_dir: &Path) -> Result<String> {
    let port = resolve_port(instance_dir).await?;
    Ok(format!("http://127.0.0.1:{}", port))
}

/// Check whether the Despotes control server is reachable for this instance.
pub async fn is_available(instance_dir: &Path) -> bool {
    let Ok(url) = base_url(instance_dir).await else {
        return false;
    };
    let client = crate::util::http::create_http_client().ok();
    let Some(client) = client else { return false };
    let probe = tokio::time::timeout(
        Duration::from_millis(1000),
        client.get(format!("{}/despotes/v1/status", url)).send(),
    )
    .await;
    matches!(probe, Ok(Ok(resp)) if resp.status().is_success())
}

async fn post_json(instance_dir: &Path, path: &str, body: &Value) -> Result<Value> {
    let url = base_url(instance_dir).await?;
    let full = format!("{}{}", url, path);

    let client = crate::util::http::create_http_client()?;
    let response = tokio::time::timeout(
        DEFAULT_TIMEOUT,
        client
            .post(&full)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send(),
    )
    .await
    .context("Timeout connecting to Despotes")?
    .with_context(|| {
        format!(
            "Cannot reach the Despotes control server at {}. Is the game \
             running with Despotes installed?",
            url
        )
    })?;

    if !response.status().is_success() {
        bail!("Despotes returned HTTP {}", response.status());
    }
    let envelope: Value = response
        .json()
        .await
        .context("Invalid JSON response from Despotes")?;

    if envelope.get("ok").and_then(Value::as_bool) == Some(false) {
        let msg = envelope
            .pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| envelope.pointer("/error/code").and_then(Value::as_str))
            .unwrap_or("unknown Despotes error");
        bail!("Despotes error: {}", msg);
    }
    Ok(envelope.get("result").cloned().unwrap_or(json!({})))
}

async fn send_action(instance_dir: &Path, command: Value) -> Result<Value> {
    post_json(instance_dir, "/despotes/v1/actions", &command).await
}

async fn send_query(instance_dir: &Path, query: Value) -> Result<Value> {
    post_json(instance_dir, "/despotes/v1/query", &query).await
}

/// Map a friendly MDL key name onto a Minecraft key identifier.
fn to_mc_key(key: &str) -> String {
    if key.starts_with("key.") {
        return key.to_string();
    }
    match key.to_ascii_lowercase().as_str() {
        "space" => "key.keyboard.space".into(),
        "escape" | "esc" => "key.keyboard.escape".into(),
        "shift" | "sneak" => "key.keyboard.left.shift".into(),
        "ctrl" | "sprint" => "key.keyboard.left.ctrl".into(),
        "enter" | "return" => "key.keyboard.enter".into(),
        other => format!("key.keyboard.{}", other),
    }
}

// ---------------------------------------------------------------------------
// High-level command helpers
// ---------------------------------------------------------------------------

/// Query game state from Despotes (in-game flag, screen, player position,
/// FPS, window focus, ...).
pub async fn game_status(instance_dir: &Path) -> Result<Value> {
    send_query(instance_dir, json!({"type": "status"})).await
}

/// Press, release, or tap a keyboard key inside the game.
///
/// `key` uses friendly names ("w", "a", "space", "escape", "e", ...) or raw
/// Minecraft key ids ("key.keyboard.w"). `action` is press | release | tap.
/// `hold_ms` applies to `tap` (converted to ticks at 20 tps).
pub async fn key_input(
    instance_dir: &Path,
    key: &str,
    action: &str,
    hold_ms: Option<u64>,
) -> Result<Value> {
    let mut cmd = json!({
        "type": "key",
        "keys": [to_mc_key(key)],
        "op": action,
    });
    if let Some(ms) = hold_ms {
        cmd["holdTicks"] = json!(ms.max(50) / 50);
    }
    send_action(instance_dir, cmd).await
}

/// Rotate the player's view to an absolute yaw/pitch (degrees), or adjust
/// the current rotation by a delta when `relative` is true.
pub async fn look(
    instance_dir: &Path,
    yaw: f32,
    pitch: f32,
    relative: bool,
) -> Result<Value> {
    send_action(
        instance_dir,
        json!({
            "type": "look",
            "mode": if relative { "delta" } else { "absolute" },
            "yaw": yaw,
            "pitch": pitch,
        }),
    )
    .await
}

/// Perform a mouse action. `button` is left | right | middle; `action` is
/// press | release | tap. When GUI coordinates `x`/`y` are given, a screen
/// click is issued; otherwise it maps to world interaction (attack / useItem /
/// pickBlock) on whatever the crosshair is over.
pub async fn mouse_input(
    instance_dir: &Path,
    button: &str,
    action: &str,
    x: Option<f64>,
    y: Option<f64>,
    hold_ms: Option<u64>,
) -> Result<Value> {
    if let (Some(x), Some(y)) = (x, y) {
        let button_code = match button {
            "right" => 1,
            "middle" => 2,
            _ => 0,
        };
        let op = match action {
            "press" => "press",
            "release" => "release",
            _ => "click",
        };
        let mut cmd = json!({
            "type": "click",
            "x": x,
            "y": y,
            "button": button_code,
            "op": op,
        });
        if op == "click" {
            if let Some(ms) = hold_ms {
                cmd["holdTicks"] = json!(ms.max(50) / 50);
            }
        }
        return send_action(instance_dir, cmd).await;
    }

    // World interaction without coordinates.
    let what = match button {
        "right" => "useItem",
        "middle" => "pickBlock",
        _ => "attack",
    };
    if action == "release" {
        // No release semantics for world use; treat as a no-op tap.
    }
    send_action(
        instance_dir,
        json!({ "type": "use", "what": what }),
    )
    .await
}

/// Scroll the hotbar by `amount` steps (positive = up) by selecting the
/// matching hotbar slot, mirroring the vanilla wheel.
pub async fn scroll(instance_dir: &Path, amount: f64) -> Result<Value> {
    let inv = send_query(instance_dir, json!({"type": "inventory"})).await?;
    let current = inv.get("selectedSlot").and_then(Value::as_u64).unwrap_or(0) as i64;
    let size = 9i64;
    let delta = -amount.round() as i64; // wheel up (positive) = previous slot
    let next = ((current + delta).rem_euclid(size)) as u32 + 1;
    key_input(instance_dir, &next.to_string(), "tap", None).await
}

/// Send a chat message or server command. Leading "/" is interpreted as a
/// command by the game.
pub async fn chat(instance_dir: &Path, message: &str) -> Result<Value> {
    send_action(
        instance_dir,
        json!({
            "type": "type",
            "target": if message.starts_with('/') { "command" } else { "chat" },
            "text": message,
            "submit": true,
        }),
    )
    .await
}

/// Fetch a screenshot from Despotes' in-game framebuffer capture. Returns
/// the raw PNG bytes.
#[cfg(any())] // superseded by capture_image below; kept for API parity
pub async fn _screenshot_raw(instance_dir: &Path) -> Result<Vec<u8>> {
    capture_image(instance_dir).await
}

/// Fetch a screenshot (PNG bytes) from the Despotes framebuffer capture.
pub async fn capture_image(instance_dir: &Path) -> Result<Vec<u8>> {
    let url = base_url(instance_dir).await?;
    let client = crate::util::http::create_http_client()?;
    let response = tokio::time::timeout(
        DEFAULT_TIMEOUT,
        client
            .get(format!("{}/despotes/v1/screenshot", url))
            .send(),
    )
    .await
    .context("Timeout fetching Despotes screenshot")?
    .context("Failed to fetch Despotes screenshot")?;
    if !response.status().is_success() {
        bail!("Despotes screenshot returned HTTP {}", response.status());
    }
    Ok(response.bytes().await?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn test_to_mc_key() {
        assert_eq!(to_mc_key("w"), "key.keyboard.w");
        assert_eq!(to_mc_key("space"), "key.keyboard.space");
        assert_eq!(to_mc_key("escape"), "key.keyboard.escape");
        assert_eq!(to_mc_key("key.keyboard.e"), "key.keyboard.e");
        assert_eq!(to_mc_key("1"), "key.keyboard.1");
    }

    #[tokio::test]
    async fn test_resolve_port_default_and_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_port(dir.path()).await.unwrap(),
            super::super::DEFAULT_DESPOTES_PORT
        );
        let runtime = dir.path().join("runtime");
        tokio::fs::create_dir_all(&runtime).await.unwrap();
        tokio::fs::write(runtime.join("despotes.port"), "25592")
            .await
            .unwrap();
        assert_eq!(resolve_port(dir.path()).await.unwrap(), 25592);
    }

    /// Spin up a one-shot HTTP server that replies with `body` to any POST.
    async fn one_shot_http(body: &'static str) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        port
    }

    #[tokio::test]
    async fn test_send_action_success_envelope() {
        let port = one_shot_http(r#"{"ok":true,"result":{"executed":"key"}}"#).await;
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        tokio::fs::create_dir_all(&runtime).await.unwrap();
        tokio::fs::write(runtime.join("despotes.port"), port.to_string())
            .await
            .unwrap();

        let resp = key_input(dir.path(), "w", "tap", None).await.unwrap();
        assert_eq!(resp["executed"], "key");
    }

    #[tokio::test]
    async fn test_send_action_error_envelope() {
        let port = one_shot_http(
            r#"{"ok":false,"error":{"code":"NOT_IN_GAME","message":"not in game"}}"#,
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        tokio::fs::create_dir_all(&runtime).await.unwrap();
        tokio::fs::write(runtime.join("despotes.port"), port.to_string())
            .await
            .unwrap();

        let err = key_input(dir.path(), "w", "tap", None).await.unwrap_err();
        assert!(err.to_string().contains("not in game"));
    }
}
