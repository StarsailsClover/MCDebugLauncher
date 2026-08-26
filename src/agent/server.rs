// Agent server - HTTP/JSON-RPC server for programmatic control

use anyhow::Result;
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum::extract::ws::{WebSocket, Message};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use std::collections::HashMap;
use futures_util::{SinkExt, StreamExt};

#[derive(Clone)]
pub struct AgentServer {
    state: Arc<RwLock<ServerState>>,
    event_tx: broadcast::Sender<ServerEvent>,
}

struct ServerState {
    uptime_start: std::time::Instant,
    running_instances: HashMap<String, InstanceProcess>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    LaunchStarted { instance: String, timestamp: String },
    LaunchProgress { instance: String, stage: String, progress: f32, message: String, timestamp: String },
    LaunchCompleted { instance: String, pid: u32, timestamp: String },
    LaunchFailed { instance: String, error: String, timestamp: String },
    LogLine { instance: String, level: String, message: String, timestamp: String },
    InstanceStopped { instance: String, exit_code: Option<i32>, timestamp: String },
    /// Alpha 8: broadcast when the game has finished booting and is ready
    /// (either in-game or at a menu), detected via the Despotes control mod.
    GameReady { instance: String, pid: u32, in_world: bool, timestamp: String },
    /// v26.2-alpha.1: broadcast when the idle watchdog terminates a game
    /// process after a configurable period of no log output.
    GameIdleTimeout { instance: String, pid: u32, idle_seconds: u64, last_line: String, timestamp: String },
}

#[derive(Debug, Clone, Serialize)]
struct InstanceProcess {
    pid: u32,
    started: String,
}

#[derive(Debug, Deserialize)]
struct ExecuteRequest {
    command: String,
    args: Vec<String>,
    #[serde(default)]
    options: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct ExecuteResponse {
    status: String,
    exit_code: i32,
    stdout: String,
    /// Machine-readable error classification (v26.1-alpha.2). Present only on
    /// failure; lets an agent branch on the failure kind instead of parsing
    /// free-form English text.
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    version: String,
    uptime: u64,
    active_instances: Vec<String>,
    running_instances: HashMap<String, InstanceProcess>,
}

/// Unified input request for the /game/:instance/input endpoint.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GameInputRequest {
    Key {
        key: String,
        #[serde(default = "default_action")]
        action: String,
        #[serde(default)]
        hold_ms: Option<u64>,
    },
    Look {
        yaw: f32,
        pitch: f32,
        #[serde(default)]
        relative: bool,
    },
    Click {
        #[serde(default = "default_button")]
        button: String,
        #[serde(default = "default_action")]
        action: String,
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
        #[serde(default)]
        hold_ms: Option<u64>,
    },
    Scroll {
        amount: f64,
    },
    Chat {
        message: String,
    },
    // Despotes v26.9 automation primitives (v26.4-alpha.8)
    /// Periodic action sequence: op = add | status | remove
    Schedule {
        op: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default, rename = "periodTicks")]
        period_ticks: Option<u64>,
        #[serde(default)]
        commands: Vec<serde_json::Value>,
    },
    /// Macro lifecycle: start-recording | record-step | stop-recording |
    /// play | stop | delete | status
    Macro {
        op: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        step: Option<serde_json::Value>,
    },
    /// Conditional branch: run the if-query, compare via dot-path field,
    /// execute then/else inline
    Condition {
        #[serde(rename = "if")]
        if_query: serde_json::Value,
        then_branch: Vec<serde_json::Value>,
        #[serde(default, rename = "else")]
        else_branch: Option<Vec<serde_json::Value>>,
    },
    /// Raw protocol passthrough for forward compatibility
    RawAction {
        command: serde_json::Value,
    },
}

fn default_action() -> String {
    "tap".to_string()
}

fn default_button() -> String {
    "left".to_string()
}

/// Screenshot query parameters.
#[derive(Debug, Deserialize)]
struct ScreenshotQuery {
    /// Return base64-encoded PNG inside JSON instead of raw image bytes.
    #[serde(default)]
    base64: bool,
    /// Seconds to wait for a frame (default 5).
    #[serde(default = "default_timeout")]
    timeout: u64,
}

fn default_timeout() -> u64 {
    5
}

impl AgentServer {
    pub fn new() -> Self {
        let state = ServerState {
            uptime_start: std::time::Instant::now(),
            running_instances: HashMap::new(),
        };

        let (event_tx, _) = broadcast::channel(1024);

        Self {
            state: Arc::new(RwLock::new(state)),
            event_tx,
        }
    }

    pub fn event_sender(&self) -> broadcast::Sender<ServerEvent> {
        self.event_tx.clone()
    }

    pub async fn start(&self, port: u16, bind_address: &str) -> Result<()> {
        let app = Router::new()
            .route("/api/v1/status", get(handle_status))
            .route("/api/v1/execute", post(handle_execute))
            .route("/api/v1/events", get(handle_websocket))
            // v26.1-alpha.1: machine-readable capability manifest for AI agents
            .route("/api/v1/capabilities", get(handle_capabilities))
            // Alpha 6 game control endpoints
            .route("/api/v1/game/windows", get(handle_game_windows))
            .route("/api/v1/game/:instance/status", get(handle_game_status))
            .route("/api/v1/game/:instance/screenshot", get(handle_game_screenshot))
            .route("/api/v1/game/:instance/ready", get(handle_game_ready))
            // v26.2-alpha.1: idle watchdog status
            .route("/api/v1/game/:instance/idle-status", get(handle_idle_status))
            .route("/api/v1/game/:instance/input", post(handle_game_input))
.route("/api/v1/game/:instance/redstone", post(handle_game_redstone))
            // v26.3-alpha.1: instance-scoped observability
            .route("/api/v1/instance/:instance/metrics", get(handle_instance_metrics))
            .route("/api/v1/instance/:instance/disk", get(handle_instance_disk))
            .with_state((self.state.clone(), self.event_tx.clone()));

        let addr = format!("{}:{}", bind_address, port);
        tracing::info!("Starting agent server on {}", addr);

        let listener = tokio::net::TcpListener::bind(&addr).await?;

        axum::serve(listener, app)
            .await
            .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;

        Ok(())
    }
}

/// v26.1-alpha.1: machine-readable capability manifest. Lets an AI agent
/// discover MDL's full command surface (REST endpoints, execute commands,
/// game inputs, WebSocket events) without parsing help text. The schema is
/// additive-only (see agent/capabilities.rs).
async fn handle_capabilities() -> impl IntoResponse {
    Json(crate::agent::capabilities::manifest())
}

async fn handle_status(
    State((state, _)): State<(Arc<RwLock<ServerState>>, broadcast::Sender<ServerEvent>)>,
) -> impl IntoResponse {
    let state = state.read().await;

    let uptime = state.uptime_start.elapsed().as_secs();
    let active_instances: Vec<String> = state.running_instances.keys().cloned().collect();

    let response = StatusResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime,
        active_instances: active_instances.clone(),
        running_instances: state.running_instances.clone(),
    };

    Json(response)
}

async fn handle_websocket(
    ws: WebSocketUpgrade,
    State((_, event_tx)): State<(Arc<RwLock<ServerState>>, broadcast::Sender<ServerEvent>)>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_websocket_connection(socket, event_tx))
}

async fn handle_websocket_connection(
    socket: WebSocket,
    event_tx: broadcast::Sender<ServerEvent>,
) {
    let (mut sender, mut receiver) = socket.split();
    let mut event_rx = event_tx.subscribe();

    // Task to forward events from broadcast to WebSocket
    let mut send_task = tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            let json = match serde_json::to_string(&event) {
                Ok(j) => j,
                Err(e) => {
                    tracing::error!("Failed to serialize event: {}", e);
                    continue;
                }
            };

            if sender.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
    });

    // Task to handle incoming WebSocket messages (ping/pong)
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Close(_) => break,
                Message::Ping(data) => {
                    // WebSocket library handles pong automatically
                    tracing::debug!("Received ping: {:?}", data);
                }
                _ => {}
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }

    tracing::info!("WebSocket connection closed");
}

async fn handle_execute(
    State((state, event_tx)): State<(Arc<RwLock<ServerState>>, broadcast::Sender<ServerEvent>)>,
    Json(payload): Json<ExecuteRequest>,
) -> impl IntoResponse {
    tracing::info!("Executing command: {} {:?}", payload.command, payload.args);

    // Execute the command and capture output
    match execute_command(&payload.command, &payload.args, &payload.options, event_tx, state).await {
        Ok((stdout, data)) => {
            let response = ExecuteResponse {
                status: "success".to_string(),
                exit_code: 0,
                stdout,
                error_code: None,
                data,
            };
            (StatusCode::OK, Json(response))
        }
        Err(e) => {
            // v26.1-alpha.2: classify the failure into a machine-readable
            // error_code so agents can branch without parsing English text.
            let msg = format!("{}", e);
            let (code, http) = classify_error(&msg);
            let response = ExecuteResponse {
                status: "error".to_string(),
                exit_code: 1,
                stdout: format!("Error: {}", msg),
                error_code: Some(code.to_string()),
                data: None,
            };
            (http, Json(response))
        }
    }
}

/// Map an execute-command failure to a stable, machine-readable error code
/// and the most fitting HTTP status. Codes are additive: new kinds may appear
/// in future versions; consumers must treat unknown codes as "internal".
fn classify_error(msg: &str) -> (&'static str, StatusCode) {
    let m = msg.to_ascii_lowercase();
    if m.contains("unknown command") {
        ("UNKNOWN_COMMAND", StatusCode::BAD_REQUEST)
    } else if m.starts_with("usage:") {
        // v26.4-alpha.1 (finding F2): argument-count/shape errors on
        // commands like inject-agent/server-cmd are client mistakes and
        // must map to BAD_REQUEST, not INTERNAL.
        ("BAD_REQUEST", StatusCode::BAD_REQUEST)
    } else if m.contains("not found") || m.contains("no instance named") || m.contains("does not exist") {
        ("NOT_FOUND", StatusCode::NOT_FOUND)
    } else if m.contains("already exists") {
        ("ALREADY_EXISTS", StatusCode::CONFLICT)
    } else if m.contains("is not running") || m.contains("not running") {
        ("NOT_RUNNING", StatusCode::CONFLICT)
    } else if m.contains("name required") || m.contains("required") {
        ("BAD_REQUEST", StatusCode::BAD_REQUEST)
    } else if m.contains("in use") || m.contains("another instance") {
        ("BUSY", StatusCode::CONFLICT)
    } else {
        ("INTERNAL", StatusCode::INTERNAL_SERVER_ERROR)
    }
}

/// Check whether a process with `pid` is alive (used for instance monitoring).
fn pid_alive(pid: u32) -> bool {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes();
    sys.process(sysinfo::Pid::from_u32(pid)).is_some()
}

/// Resolve an instance's directory by name.
async fn resolve_instance_dir(instance: &str) -> Result<std::path::PathBuf> {
    use crate::instance::InstanceManager;
    let manager = InstanceManager::new()?;
    let inst = manager.get(instance).await?;
    Ok(inst.path)
}

// ---------------------------------------------------------------------------
// Game control endpoints (Alpha 6)
// ---------------------------------------------------------------------------

async fn handle_game_windows() -> impl IntoResponse {
    #[cfg(windows)]
    {
        let windows = crate::game::window::list_mdl_windows(&crate::game::window::collect_running_pids());
        Json(serde_json::json!({
            "status": "success",
            "data": { "windows": windows, "count": windows.len() }
        }))
        .into_response()
    }
    #[cfg(not(windows))]
    {
        (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "status": "error",
                "error": "Game window discovery is currently supported only on Windows"
            })),
        )
            .into_response()
    }
}

async fn handle_game_status(Path(instance): Path<String>) -> impl IntoResponse {
    let dir = match resolve_instance_dir(&instance).await {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"status": "error", "error": e.to_string()})),
            );
        }
    };

    if !crate::game::client::is_available(&dir).await {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "error",
                "error": format!(
                    "Agent control unavailable for '{}'. Launch it with: mdl launch {} --detach --agent",
                    instance, instance
                )
            })),
        );
    }

    match crate::game::client::game_status(&dir).await {
        Ok(response) => (StatusCode::OK, Json(response)),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"status": "error", "error": e.to_string()})),
        ),
    }
}

/// Alpha 8: report whether the game has finished booting (ready) by
/// querying the Despotes control mod. Returns 503 while not ready.
async fn handle_game_ready(Path(instance): Path<String>) -> impl IntoResponse {
    let dir = match resolve_instance_dir(&instance).await {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"status": "error", "error": e.to_string()})),
            );
        }
    };
    if !crate::game::client::is_available(&dir).await {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "not_ready", "ready": false})),
        );
    }
    match crate::game::client::game_status(&dir).await {
        Ok(st) => {
            let in_world = st.get("inGame").and_then(|v| v.as_bool()).unwrap_or(false);
            let screen = st.get("screenOpen").and_then(|v| v.as_bool()).unwrap_or(false);
            let ready = in_world || screen;
            (
                if ready { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE },
                Json(serde_json::json!({ "status": "success", "ready": ready, "in_world": in_world, "detail": st })),
            )
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"status": "error", "error": e.to_string()})),
        ),
    }
}

/// v26.2-alpha.1: report the idle-watchdog status for a running instance.
/// Returns the last-output age, threshold, and remaining time before
/// termination. Returns 404 if the instance is not found, or 503 if no
/// game process is running for the instance.
async fn handle_idle_status(Path(instance): Path<String>) -> impl IntoResponse {
    let dir = match resolve_instance_dir(&instance).await {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"status": "error", "error": e.to_string()})),
            );
        }
    };

    // Read the PID from the runtime pid file.
    let pid: Option<u32> = tokio::fs::read_to_string(dir.join("runtime").join("pid"))
        .await
        .ok()
        .and_then(|c| c.trim().parse().ok());

    let pid = match pid {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "status": "error",
                    "error": format!("No running game process for instance '{}'", instance),
                    "error_code": "NOT_RUNNING",
                })),
            );
        }
    };

    // Read the idle timeout marker (if the watchdog fired).
    let marker_path = dir.join("runtime").join("idle_timeout");
    let fired = tokio::fs::read_to_string(&marker_path).await.ok();

    // Read the launch log's last modification time as a proxy for last output.
    let log_path = dir.join("logs").join("launch_detached.log");
    let last_output_age_secs = match std::fs::metadata(&log_path) {
        Ok(meta) => {
            if let Ok(modified) = meta.modified() {
                modified
                    .elapsed()
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            } else {
                0
            }
        }
        Err(_) => 0,
    };

    // Check if the process is still alive.
    let alive = pid_alive(pid);

    let response = serde_json::json!({
        "instance": instance,
        "pid": pid,
        "process_alive": alive,
        "last_output_age_secs": last_output_age_secs,
        "idle_timeout_fired": fired.is_some(),
        "idle_timeout_event": fired,
    });

    (
        if alive { StatusCode::OK } else { StatusCode::GONE },
        Json(serde_json::json!({ "status": "success", "data": response })),
    )
}

/// v26.3-alpha.1: launch metrics for an instance (latest by default,
/// ?history=true for the recorded history). Local-only data.
#[derive(Debug, Deserialize)]
struct InstanceMetricsQuery {
    #[serde(default)]
    history: bool,
}

async fn handle_instance_metrics(
    Path(instance): Path<String>,
    axum::extract::Query(q): axum::extract::Query<InstanceMetricsQuery>,
) -> impl IntoResponse {
    let dir = match resolve_instance_dir(&instance).await {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"status": "error", "error": e.to_string()})),
            );
        }
    };
    let launches = if q.history {
        crate::util::metrics::load_history(&dir)
    } else {
        crate::util::metrics::load_latest(&dir).into_iter().collect::<Vec<_>>()
    };
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "data": { "instance": instance, "launches": launches }
        })),
    )
}

/// v26.3-alpha.1: disk usage of an instance with a top-level breakdown
/// (identical numbers to `mdl status <name> --disk`).
async fn handle_instance_disk(Path(instance): Path<String>) -> impl IntoResponse {
    use crate::util::disk::{dir_size, format_bytes};
    let dir = match resolve_instance_dir(&instance).await {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"status": "error", "error": e.to_string()})),
            );
        }
    };

    let mut breakdown: Vec<(String, u64)> = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
        while let Some(entry) = entries.next_entry().await.ok().flatten() {
            let p = entry.path();
            let size = if p.is_dir() { dir_size(&p).await } else { entry.metadata().await.map(|m| m.len()).unwrap_or(0) };
            breakdown.push((
                p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                size,
            ));
        }
    }
    breakdown.sort_by(|a, b| b.1.cmp(&a.1));
    let total: u64 = breakdown.iter().map(|(_, s)| *s).sum();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "data": {
                "instance": instance,
                "bytes_total": total,
                "human_total": format_bytes(total),
                "breakdown": breakdown.iter().map(|(n, b)| serde_json::json!({
                    "path": n, "bytes": b, "human": format_bytes(*b)
                })).collect::<Vec<_>>()
            }
        })),
    )
}

async fn handle_game_screenshot(
    Path(instance): Path<String>,
    axum::extract::Query(query): axum::extract::Query<ScreenshotQuery>,
) -> impl IntoResponse {
    #[cfg(not(windows))]
    {
        let _ = (instance, query);
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "status": "error",
                "error": "Screenshot capture is currently supported only on Windows"
            })),
        )
            .into_response();
    }

    #[cfg(windows)]
    {
        let dir = match resolve_instance_dir(&instance).await {
            Ok(d) => d,
            Err(e) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"status": "error", "error": e.to_string()})),
                )
                    .into_response();
            }
        };

        let pid: Option<u32> = tokio::fs::read_to_string(dir.join("runtime").join("pid"))
            .await
            .ok()
            .and_then(|c| c.trim().parse().ok());

        let start = std::time::Instant::now();
        match crate::game::capture::capture_instance_png(&instance, pid, query.timeout) {
            Ok(image) => {
                let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                if query.base64 {
                    use base64::Engine;
                    let encoded = base64::engine::general_purpose::STANDARD
                        .encode(&image.png_bytes);
                    Json(serde_json::json!({
                        "status": "success",
                        "data": {
                            "format": "png",
                            "width": image.width,
                            "height": image.height,
                            "size_bytes": image.png_bytes.len(),
                            "capture_ms": elapsed_ms,
                            "base64": encoded
                        }
                    }))
                    .into_response()
                } else {
                    let mut response = axum::http::Response::new(axum::body::Body::from(
                        image.png_bytes,
                    ));
                    response.headers_mut().insert(
                        axum::http::header::CONTENT_TYPE,
                        axum::http::HeaderValue::from_static("image/png"),
                    );
                    response.into_response()
                }
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"status": "error", "error": e.to_string()})),
            )
                .into_response(),
        }
    }
}

async fn handle_game_input(
    Path(instance): Path<String>,
    Json(payload): Json<GameInputRequest>,
) -> impl IntoResponse {
    let dir = match resolve_instance_dir(&instance).await {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"status": "error", "error": e.to_string()})),
            );
        }
    };

    let result = match payload {
        GameInputRequest::Key { key, action, hold_ms } => {
            crate::game::client::key_input(&dir, &key, &action, hold_ms).await
        }
        GameInputRequest::Look { yaw, pitch, relative } => {
            crate::game::client::look(&dir, yaw, pitch, relative).await
        }
        GameInputRequest::Click { button, action, x, y, hold_ms } => {
            crate::game::client::mouse_input(&dir, &button, &action, x, y, hold_ms).await
        }
        GameInputRequest::Scroll { amount } => crate::game::client::scroll(&dir, amount).await,
        GameInputRequest::Chat { message } => crate::game::client::chat(&dir, &message).await,
        GameInputRequest::Schedule { op, name, period_ticks, commands } => {
            let op = op.to_ascii_lowercase();
            let payload = match op.as_str() {
                "add" => {
                    if name.is_none() || period_ticks.is_none() || commands.is_empty() {
                        Err(anyhow::anyhow!(
                            "schedule add requires name, periodTicks and at least one command"
                        ))
                    } else {
                        Ok(crate::game::client::schedule_payload(
                            "add",
                            name.as_deref(),
                            period_ticks,
                            Some(serde_json::Value::Array(commands)),
                        ))
                    }
                }
                "status" => Ok(crate::game::client::schedule_payload("status", None, None, None)),
                "remove" => match name {
                    Some(n) => Ok(crate::game::client::schedule_payload("remove", Some(n.as_str()), None, None)),
                    None => Err(anyhow::anyhow!("schedule remove requires a name")),
                },
                other => Err(anyhow::anyhow!(
                    "Unknown schedule op '{other}'. Supported: add / status / remove"
                )),
            };
            match payload {
                Ok(p) => crate::game::client::automation_action(&dir, p).await,
                Err(e) => Err(e),
            }
        }
        GameInputRequest::Macro { op, name, step } => {
            const OPS: &[&str] = &[
                "start-recording", "record-step", "stop-recording",
                "play", "stop", "delete", "status",
            ];
            let validated = if !OPS.contains(&op.as_str()) {
                Err(anyhow::anyhow!("Unknown macro op '{}'. Supported: {}", op, OPS.join(", ")))
            } else {
                let needs_name = !matches!(op.as_str(), "stop-recording" | "status" | "stop");
                if needs_name && name.is_none() {
                    Err(anyhow::anyhow!("macro {} requires a name", op))
                } else if op == "record-step" && step.is_none() {
                    Err(anyhow::anyhow!("macro record-step requires a step action"))
                } else {
                    Ok(crate::game::client::macro_payload(&op, name.as_deref(), step))
                }
            };
            match validated {
                Ok(p) => crate::game::client::automation_action(&dir, p).await,
                Err(e) => Err(e),
            }
        }
        GameInputRequest::Condition { if_query, then_branch, else_branch } => {
            crate::game::client::automation_action(
                &dir,
                crate::game::client::condition_payload(if_query, serde_json::Value::Array(then_branch), else_branch.map(serde_json::Value::Array)),
            )
            .await
        }
        GameInputRequest::RawAction { command } => {
            crate::game::client::automation_action(&dir, command).await
        }
    };

    match result {
        Ok(response) => (StatusCode::OK, Json(response)),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"status": "error", "error": e.to_string()})),
        ),
    }
}

// ---------------------------------------------------------------------------
// Command execution
// ---------------------------------------------------------------------------

/// Redstone signal query (Despotes v26.9). Body is optional; when it omits
/// coordinates the agent probes the crosshair target block.
#[derive(Debug, Deserialize, Default)]
struct GameRedstoneRequest {
    #[serde(default)]
    x: Option<i32>,
    #[serde(default)]
    y: Option<i32>,
    #[serde(default)]
    z: Option<i32>,
}

async fn handle_game_redstone(
    Path(instance): Path<String>,
    payload: Option<Json<GameRedstoneRequest>>,
) -> impl IntoResponse {
    let dir = match resolve_instance_dir(&instance).await {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"status": "error", "error": e.to_string()})),
            );
        }
    };
    let (x, y, z) = payload
        .map(|Json(p)| (p.x, p.y, p.z))
        .unwrap_or((None, None, None));
    match crate::game::client::redstone_query(&dir, x, y, z).await {
        Ok(response) => (StatusCode::OK, Json(response)),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"status": "error", "error": e.to_string()})),
        ),
    }
}

async fn execute_command(
    command: &str,
    args: &[String],
    options: &HashMap<String, String>,
    event_tx: broadcast::Sender<ServerEvent>,
    state: Arc<RwLock<ServerState>>,
) -> Result<(String, Option<serde_json::Value>)> {
    use crate::instance::InstanceManager;

    match command {
        "list" => {
            let manager = InstanceManager::new()?;
            let instances = manager.list().await?;

            let data: Vec<_> = instances.iter().map(|inst| {
                serde_json::json!({
                    "name": inst.name,
                    "version": inst.config.version,
                    "loader": inst.config.loader.as_ref().map(|l| {
                        serde_json::json!({
                            "type": l.loader_type,
                            "version": l.version
                        })
                    }),
                    "path": inst.path.display().to_string()
                })
            }).collect();

            let json_data = serde_json::json!({
                "instances": data,
                "count": instances.len()
            });

            Ok((format!("Found {} instances", instances.len()), Some(json_data)))
        }
        "create" => {
            if args.is_empty() {
                anyhow::bail!("Instance name required");
            }

            let name = &args[0];
            let version = args.get(1).map(|s| s.as_str()).unwrap_or("release");

            let config = crate::instance::config::InstanceConfig {
                name: name.to_string(),
                version: version.to_string(),
                loader: None,
                javaagents: Vec::new(),
            };

            let manager = InstanceManager::new()?;
            let instance = manager.create(config, true).await?;

            let data = serde_json::json!({
                "name": instance.name,
                "version": instance.config.version,
                "path": instance.path.display().to_string()
            });

            Ok((format!("Instance '{}' created", name), Some(data)))
        }
        "info" => {
            if args.is_empty() {
                anyhow::bail!("Instance name required");
            }

            let name = &args[0];
            let manager = InstanceManager::new()?;
            let instance = manager.get(name).await?;

            let data = serde_json::json!({
                "name": instance.name,
                "version": instance.config.version,
                "loader": instance.config.loader,
                "path": instance.path.display().to_string()
            });

            Ok((format!("Instance: {}", name), Some(data)))
        }
        "launch" => {
            if args.is_empty() {
                anyhow::bail!("Instance name required");
            }

            let name = &args[0];

            // Build launch options from the request's options map. The agent
            // API always launches detached.
            let mut launch_options = crate::instance::launcher::LaunchOptions::default();
            launch_options.detach = true;
            if let Some(username) = options.get("username") {
                launch_options.username = Some(username.clone());
            }
            if let Some(server) = options.get("server") {
                launch_options.server = Some(server.clone());
            }
            if options.get("fullscreen").map(|v| v == "true").unwrap_or(false) {
                launch_options.fullscreen = true;
            }
            if let Some(w) = options.get("width").and_then(|v| v.parse().ok()) {
                launch_options.width = Some(w);
            }
            if let Some(h) = options.get("height").and_then(|v| v.parse().ok()) {
                launch_options.height = Some(h);
            }
            if options.get("agent").map(|v| v == "true").unwrap_or(false) {
                launch_options.agent = true;
            }
            if let Some(port) = options.get("agent-port").and_then(|v| v.parse().ok()) {
                launch_options.agent_port = Some(port);
            }
            if let Some(java_path) = options.get("java-path") {
                launch_options.java_path = Some(java_path.clone());
            }
            // v26.4-alpha.9: AprismJDK selection. Same contract as the CLI
            // (--jdk aprism[@<tag|version>]); on resolution failure the
            // standard detection chain (Adoptium provisioning) applies, and
            // the reason is logged for the agent's event stream.
            if let Some(jdk) = options.get("jdk") {
                if options.contains_key("java-path") {
                    return Err(anyhow::anyhow!("jdk and java-path options are mutually exclusive"));
                }
                let hint = jdk
                    .strip_prefix("aprism")
                    .map(|rest| rest.trim_start_matches('@'))
                    .unwrap_or(jdk.as_str());
                match crate::loader::aprism_jdk::resolve(Some(hint)) {
                    Ok((tag, java)) => {
                        tracing::info!("launch via agent API uses AprismJDK {tag}: {}", java.display());
                        launch_options.java_path = Some(java.display().to_string());
                    }
                    Err(e) => {
                        tracing::warn!(
                            "AprismJDK unavailable ({e:#}); falling back to system Java / Eclipse Adoptium"
                        );
                    }
                }
            }
            if let Some(memory) = options.get("memory") {
                launch_options.memory = Some(memory.clone());
            }
            if options.get("aprism").map(|v| v == "true").unwrap_or(false) {
                launch_options.aprism = true;
            }
            if options.get("enter-test-world").map(|v| v == "true").unwrap_or(false) {
                launch_options.enter_test_world = true;
            }
            if options.get("no-queue").map(|v| v == "true").unwrap_or(false) {
                launch_options.no_queue = true;
            }
            // v26.2-alpha.1: idle watchdog options.
            if let Some(timeout) = options.get("idle-timeout").and_then(|v| v.parse().ok()) {
                launch_options.idle_timeout = Some(timeout);
            }
            if options.get("no-idle-timeout").map(|v| v == "true").unwrap_or(false) {
                launch_options.no_idle_timeout = true;
            }
            // v26.3-alpha.2: OOM second-confirmation options.
            if let Some(mode) = options.get("oom-confirm") {
                launch_options.oom_confirm = Some(mode.clone());
            }
            if options.get("oom-list-only").map(|v| v == "true").unwrap_or(false) {
                launch_options.oom_list_only = true;
            }
            // v26.2-alpha.4: OOM protection options.
            if let Some(val) = options.get("oom-protect") {
                launch_options.oom_protect = val == "true";
            }
            if options.get("oom-aggressive").map(|v| v == "true").unwrap_or(false) {
                launch_options.oom_aggressive = true;
            }
            // v26.2-alpha.5: ad-hoc JavaAgent attachment.
            if let Some(ja) = options.get("javaagent") {
                // Comma-separated list of `jar` or `jar=params` specs.
                for spec in ja.split(',') {
                    let spec = spec.trim();
                    if !spec.is_empty() {
                        launch_options.javaagents.push(spec.to_string());
                    }
                }
            }

            // Validate the instance exists before handing off.
            let manager = InstanceManager::new()?;
            let _instance = manager.get(name).await?;

            // Send launch started event.
            let _ = event_tx.send(ServerEvent::LaunchStarted {
                instance: name.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });

            // Alpha 8.1: the launch preparation (library verification, asset
            // checks, Java resolution, downloads) can take minutes on slow
            // networks. Previously the HTTP request waited for all of it and
            // clients hit request timeouts. The launch now runs as a
            // background task: this call returns immediately with
            // status "launching", and the real outcome is delivered via the
            // launch_progress / launch_completed / launch_failed events
            // (WebSocket stream).
            let launch_agent = launch_options.agent;
            let launch_name = name.to_string();
            let launch_state = state.clone();
            let launch_tx = event_tx.clone();
            tokio::spawn(async move {
                let _ = launch_tx.send(ServerEvent::LaunchProgress {
                    instance: launch_name.clone(),
                    stage: "preparing".to_string(),
                    progress: 0.1,
                    message: "Loading instance configuration".to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                });

                let launcher = match crate::instance::launcher::InstanceLauncher::new() {
                    Ok(l) => l,
                    Err(e) => {
                        let _ = launch_tx.send(ServerEvent::LaunchFailed {
                            instance: launch_name,
                            error: e.to_string(),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        });
                        return;
                    }
                };

                let _ = launch_tx.send(ServerEvent::LaunchProgress {
                    instance: launch_name.clone(),
                    stage: "downloading".to_string(),
                    progress: 0.3,
                    message: "Verifying libraries, assets and files".to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                });

                match launcher.launch(&launch_name, &launch_options).await {
                    Ok(outcome) => {
                        let _ = launch_tx.send(ServerEvent::LaunchCompleted {
                            instance: launch_name.clone(),
                            pid: outcome.pid,
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        });

                        // Track the running instance and emit instance_stopped
                        // when the game process exits.
                        {
                            let mut s = launch_state.write().await;
                            s.running_instances.insert(
                                launch_name.clone(),
                                InstanceProcess {
                                    pid: outcome.pid,
                                    started: chrono::Utc::now().to_rfc3339(),
                                },
                            );
                        }
                        let monitor_state = launch_state.clone();
                        let monitor_tx = launch_tx.clone();
                        let monitor_name = launch_name.clone();
                        let monitor_pid = outcome.pid;
                        tokio::spawn(async move {
                            loop {
                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                if !pid_alive(monitor_pid) {
                                    let mut s = monitor_state.write().await;
                                    s.running_instances.remove(&monitor_name);
                                    let _ = monitor_tx.send(ServerEvent::InstanceStopped {
                                        instance: monitor_name,
                                        exit_code: None,
                                        timestamp: chrono::Utc::now().to_rfc3339(),
                                    });
                                    break;
                                }
                            }
                        });

                        // Alpha 8: poll Despotes until the game reports ready and
                        // broadcast a single GameReady event.
                        let ready_state = launch_state.clone();
                        let ready_tx = launch_tx.clone();
                        let ready_name = launch_name.clone();
                        let ready_pid = outcome.pid;
                        tokio::spawn(async move {
                            let _ = (ready_state,);
                            let Ok(instances) = crate::util::paths::get_instances_dir() else { return };
                            let inst_dir = instances.join(&ready_name);
                            for _ in 0..150 {
                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                if !pid_alive(ready_pid) {
                                    return;
                                }
                                if crate::game::client::is_available(&inst_dir).await {
                                    if let Ok(st) = crate::game::client::game_status(&inst_dir).await {
                                        let in_world = st.get("inGame").and_then(|v| v.as_bool()).unwrap_or(false);
                                        let screen = st.get("screenOpen").and_then(|v| v.as_bool()).unwrap_or(false);
                                        if in_world || screen {
                                            let _ = ready_tx.send(ServerEvent::GameReady {
                                                instance: ready_name,
                                                pid: ready_pid,
                                                in_world,
                                                timestamp: chrono::Utc::now().to_rfc3339(),
                                            });
                                            return;
                                        }
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => {
                        let _ = launch_tx.send(ServerEvent::LaunchFailed {
                            instance: launch_name,
                            error: e.to_string(),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        });
                    }
                }
            });

            let data = serde_json::json!({
                "instance": name,
                "status": "launching",
                "agent": launch_agent,
                "note": "Launch runs in the background; watch launch_progress/launch_completed/launch_failed events"
            });
            Ok((format!("Instance '{}' launch started (background)", name), Some(data)))
        }
        // v26.1-alpha.2: the agent can launch instances but previously had no
        // way to stop them. `stop` kills the game process tree of a running
        // instance (resolved via its runtime/pid file) and cleans up state.
        "stop" => {
            if args.is_empty() {
                anyhow::bail!("Instance name required");
            }
            let name = &args[0];
            let manager = InstanceManager::new()?;
            let instance = manager.get(name).await?;
            let pid_file = instance.path.join("runtime").join("pid");
            let Some(pid) = crate::loader::server::running_pid(&instance.path) else {
                anyhow::bail!("Instance '{}' is not running", name);
            };
            crate::loader::server::kill_pid(pid)?;
            let _ = tokio::fs::remove_file(&pid_file).await;
            let _ = pid_file;
            // Drop it from the server's running-instance table too.
            {
                let mut s = state.write().await;
                s.running_instances.remove(name);
            }
            let _ = event_tx.send(ServerEvent::InstanceStopped {
                instance: name.to_string(),
                exit_code: None,
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
            let data = serde_json::json!({
                "instance": name,
                "pid": pid,
                "status": "stopped"
            });
            Ok((format!("Instance '{}' stopped (PID {})", name, pid), Some(data)))
        }
        // v26.3-alpha.1: observability + lifecycle mappings.
        "metrics" => {
            if args.is_empty() {
                anyhow::bail!("Instance name required");
            }
            let name = &args[0];
            let dir = resolve_instance_dir(name).await?;
            let history = options.get("history").map(|v| v == "true").unwrap_or(false);
            let launches = if history {
                crate::util::metrics::load_history(&dir)
            } else {
                crate::util::metrics::load_latest(&dir).into_iter().collect::<Vec<_>>()
            };
            Ok((
                format!("{} launch record(s) for '{}'", launches.len(), name),
                Some(serde_json::json!({ "launches": launches })),
            ))
        }
        "disk" => {
            use crate::util::disk::{dir_size, format_bytes};
            if args.is_empty() {
                anyhow::bail!("Instance name required");
            }
            let name = &args[0];
            let dir = resolve_instance_dir(name).await?;
            let mut breakdown: Vec<(String, u64)> = Vec::new();
            if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
                while let Some(entry) = entries.next_entry().await.ok().flatten() {
                    let p = entry.path();
                    let size = if p.is_dir() { dir_size(&p).await } else { entry.metadata().await.map(|m| m.len()).unwrap_or(0) };
                    breakdown.push((p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), size));
                }
            }
            breakdown.sort_by(|a, b| b.1.cmp(&a.1));
            let total: u64 = breakdown.iter().map(|(_, s)| *s).sum();
            Ok((
                format!("Disk usage of '{}': {}", name, format_bytes(total)),
                Some(serde_json::json!({
                    "bytes_total": total,
                    "human_total": format_bytes(total),
                    "breakdown": breakdown.iter().map(|(n,b)| serde_json::json!({"path":n,"bytes":b,"human":format_bytes(*b)})).collect::<Vec<_>>()
                })),
            ))
        }
        "inject-agent" => {
            if args.len() < 2 {
                anyhow::bail!("Usage: inject-agent <instance> <jar> (options: params, java-path)");
            }
            let name = &args[0];
            let jar_arg = &args[1];
            let manager = InstanceManager::new()?;
            let inst = manager.get(name).await?;

            let pid: u32 = tokio::fs::read_to_string(inst.path.join("runtime").join("pid"))
                .await
                .ok()
                .and_then(|c| c.trim().parse().ok())
                .ok_or_else(|| anyhow::anyhow!("Instance '{}' is not running", name))?;

            let jar_path = std::path::PathBuf::from(jar_arg);
            let jar_path = if jar_path.is_absolute() && jar_path.exists() {
                jar_path
            } else {
                let registered = inst.path.join("javaagents").join(jar_arg);
                let with_ext = registered.with_extension("jar");
                if registered.exists() {
                    registered
                } else if with_ext.exists() {
                    with_ext
                } else {
                    anyhow::bail!("Agent JAR not found: '{}' is not an existing path or registered javaagent", jar_arg);
                }
            };

            let java = match options.get("java-path") {
                Some(p) => std::path::PathBuf::from(p),
                None => crate::version::java::JavaRuntime::detect()
                    .map(|r| r.path)
                    .unwrap_or_else(|_| std::path::PathBuf::from("java")),
            };
            crate::game::attach::inject_agent(&java, pid, &jar_path, options.get("params").map(String::as_str)).await?;
            Ok((
                format!("Agent attached to instance '{}' (PID {})", name, pid),
                Some(serde_json::json!({"instance": name, "pid": pid, "agent": jar_path.display().to_string()})),
            ))
        }
        "server-cmd" => {
            if args.len() < 2 {
                anyhow::bail!("Usage: server-cmd <server> <command...>");
            }
            let name = &args[0];
            let command = args[1..].join(" ");
            if command.trim().is_empty() {
                anyhow::bail!("Empty console command");
            }
            let info = crate::loader::server::load_server(name)?;
            if info.dir().ok().and_then(|d| crate::loader::server::running_pid(&d)).is_none() {
                anyhow::bail!("Server '{}' is not running", name);
            }
            let response = crate::loader::server::run_console_command(&info, &command).await?;
            Ok((
                response.trim().to_string(),
                Some(serde_json::json!({"server": name, "command": command, "response": response})),
            ))
        }
        _ => {
            anyhow::bail!("Unknown command: {}", command)
        }
    }
}
