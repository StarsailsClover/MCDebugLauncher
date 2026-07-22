// Agent server - HTTP/JSON-RPC server for programmatic control

use anyhow::Result;
use axum::{
    extract::{State, WebSocketUpgrade},
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

async fn execute_command(
    command: &str,
    args: &[String],
    _options: &HashMap<String, String>,
    event_tx: broadcast::Sender<ServerEvent>,
    _state: Arc<RwLock<ServerState>>,
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

            // Send launch started event
            let _ = event_tx.send(ServerEvent::LaunchStarted {
                instance: name.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });

            // Launch the instance
            let manager = InstanceManager::new()?;
            let instance = manager.get(name).await?;

            // Send progress events during launch
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

            match launcher.launch(&name, &crate::instance::launcher::LaunchOptions::default()).await {
                Ok(_) => {
                    // Note: Current launcher.launch() returns (), not a process handle
                    // For now we'll report success without PID
                    let _ = event_tx.send(ServerEvent::LaunchCompleted {
                        instance: name.to_string(),
                        pid: 0,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    });

                    let data = serde_json::json!({
                        "instance": name,
                        "status": "launched"
                    });

                    Ok((format!("Instance '{}' launched", name), Some(data)))
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
