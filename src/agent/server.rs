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
            // Alpha 6 game control endpoints
            .route("/api/v1/game/windows", get(handle_game_windows))
            .route("/api/v1/game/:instance/status", get(handle_game_status))
            .route("/api/v1/game/:instance/screenshot", get(handle_game_screenshot))
            .route("/api/v1/game/:instance/input", post(handle_game_input))
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
                data,
            };
            (StatusCode::OK, Json(response))
        }
        Err(e) => {
            let response = ExecuteResponse {
                status: "error".to_string(),
                exit_code: 1,
                stdout: format!("Error: {}", e),
                data: None,
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response))
        }
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
            // API always launches detached: the HTTP call returns as soon as
            // the game process starts, reporting its real PID. The previous
            // implementation blocked until the game exited and reported pid 0.
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

            // Send launch started event
            let _ = event_tx.send(ServerEvent::LaunchStarted {
                instance: name.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });

            let manager = InstanceManager::new()?;
            let _instance = manager.get(name).await?;

            let _ = event_tx.send(ServerEvent::LaunchProgress {
                instance: name.to_string(),
                stage: "preparing".to_string(),
                progress: 0.2,
                message: "Loading instance configuration".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });

            let launcher = crate::instance::launcher::InstanceLauncher::new()?;

            let _ = event_tx.send(ServerEvent::LaunchProgress {
                instance: name.to_string(),
                stage: "downloading".to_string(),
                progress: 0.5,
                message: "Verifying libraries and assets".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });

            match launcher.launch(name, &launch_options).await {
                Ok(outcome) => {
                    let _ = event_tx.send(ServerEvent::LaunchCompleted {
                        instance: name.to_string(),
                        pid: outcome.pid,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    });

                    // Track the running instance and emit instance_stopped
                    // when the game process exits.
                    {
                        let mut s = state.write().await;
                        s.running_instances.insert(
                            name.to_string(),
                            InstanceProcess {
                                pid: outcome.pid,
                                started: chrono::Utc::now().to_rfc3339(),
                            },
                        );
                    }
                    let monitor_state = state.clone();
                    let monitor_tx = event_tx.clone();
                    let monitor_name = name.to_string();
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

                    let data = serde_json::json!({
                        "instance": name,
                        "status": "launched",
                        "pid": outcome.pid,
                        "detached": outcome.detached,
                        "agent": launch_options.agent
                    });

                    Ok((format!("Instance '{}' launched (PID {})", name, outcome.pid), Some(data)))
                }
                Err(e) => {
                    let _ = event_tx.send(ServerEvent::LaunchFailed {
                        instance: name.to_string(),
                        error: e.to_string(),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    });

                    Err(e)
                }
            }
        }
        _ => {
            anyhow::bail!("Unknown command: {}", command)
        }
    }
}
