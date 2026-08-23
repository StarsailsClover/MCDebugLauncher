// Machine-readable capability manifest for AI/agent consumers
// (v26.1-alpha.1).
//
// MDL is a launcher built to be driven by LLM agents. So an agent can
// discover what MDL can do WITHOUT scraping help text, this module emits a
// single, stable, structured description of:
//   - every `/api/v1/*` REST endpoint (method, path, purpose),
//   - every `execute` command the agent API accepts (arguments + options),
//   - every game input type accepted by POST /game/:instance/input,
//   - the WebSocket event stream and its event kinds.
//
// The shape is additive-only: consumers must ignore unknown fields, and MDL
// only ever adds fields/endpoints here, never removes or renames them.

use serde::Serialize;

/// One REST endpoint exposed by the agent server.
#[derive(Debug, Clone, Serialize)]
pub struct Endpoint {
    pub method: &'static str,
    pub path: &'static str,
    pub purpose: &'static str,
}

/// One `execute` command accepted by POST /api/v1/execute.
#[derive(Debug, Clone, Serialize)]
pub struct ExecCommand {
    pub command: &'static str,
    pub description: &'static str,
    /// Positional arguments, in order.
    pub args: Vec<ArgSpec>,
    /// Recognized keys in the `options` map.
    pub options: Vec<OptionSpec>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArgSpec {
    pub name: &'static str,
    pub required: bool,
    pub description: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct OptionSpec {
    pub key: &'static str,
    pub values: &'static str,
    pub description: &'static str,
}

/// One game input type accepted by POST /game/:instance/input.
#[derive(Debug, Clone, Serialize)]
pub struct GameInput {
    #[serde(rename = "type")]
    pub input_type: &'static str,
    pub description: &'static str,
    pub fields: Vec<ArgSpec>,
}

/// Full capability manifest.
#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub launcher: &'static str,
    pub version: &'static str,
    pub schema: &'static str,
    /// Control-plane REST endpoints.
    pub endpoints: Vec<Endpoint>,
    /// Commands runnable through POST /api/v1/execute.
    pub execute_commands: Vec<ExecCommand>,
    /// In-game control inputs (require the Despotes control mod).
    pub game_inputs: Vec<GameInput>,
    /// WebSocket event stream contract.
    pub events: EventsSpec,
    /// Machine-readable error codes that `POST /api/v1/execute` may return in
    /// the `error_code` field on failure (v26.1-alpha.2). Additive: unknown
    /// codes must be treated as "internal".
    pub error_codes: Vec<ErrorCodeSpec>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorCodeSpec {
    pub code: &'static str,
    pub http_status: u16,
    pub description: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventsSpec {
    pub websocket_path: &'static str,
    pub kinds: Vec<&'static str>,
}

/// Build the manifest for the current build. Pure function: no IO, so it is
/// usable from both the HTTP endpoint and the CLI.
pub fn manifest() -> Capabilities {
    Capabilities {
        launcher: "mdl",
        version: env!("CARGO_PKG_VERSION"),
        schema: "mdl.capabilities/v1",
        endpoints: vec![
            Endpoint { method: "GET",  path: "/api/v1/status",                   purpose: "Launcher version, uptime and running instances" },
            Endpoint { method: "POST", path: "/api/v1/execute",                  purpose: "Run a launcher command; body = {command, args, options}" },
            Endpoint { method: "GET",  path: "/api/v1/events",                   purpose: "WebSocket stream of launcher/game events" },
            Endpoint { method: "GET",  path: "/api/v1/capabilities",             purpose: "This machine-readable capability manifest" },
            Endpoint { method: "GET",  path: "/api/v1/game/windows",             purpose: "List Minecraft game windows MDL can see" },
            Endpoint { method: "GET",  path: "/api/v1/game/:instance/status",    purpose: "In-game status of an instance via Despotes" },
            Endpoint { method: "GET",  path: "/api/v1/game/:instance/screenshot",purpose: "PNG screenshot (Despotes framebuffer, WGC fallback)" },
            Endpoint { method: "GET",  path: "/api/v1/game/:instance/ready",     purpose: "Whether the game broadcast ready (in world)" },
            Endpoint { method: "GET",  path: "/api/v1/game/:instance/idle-status",purpose: "Idle watchdog status (last output age, threshold, remaining)" },
            Endpoint { method: "POST", path: "/api/v1/game/:instance/input",     purpose: "Inject an in-game input (key/click/look/chat/scroll)" },
            // v26.3-alpha.1: instance-scoped observability
            Endpoint { method: "GET",  path: "/api/v1/instance/:instance/metrics", purpose: "Launch metrics for an instance (?history=true for full history)" },
            Endpoint { method: "GET",  path: "/api/v1/instance/:instance/disk",    purpose: "Disk usage with a top-level breakdown" },
        ],
        execute_commands: vec![
            ExecCommand {
                command: "list",
                description: "List all instances",
                args: vec![],
                options: vec![
                    OptionSpec { key: "format", values: "text|json", description: "Output format (default text)" },
                ],
            },
            ExecCommand {
                command: "create",
                description: "Create a new instance (downloads the version)",
                args: vec![
                    ArgSpec { name: "name", required: true, description: "Instance name" },
                    ArgSpec { name: "version", required: false, description: "Minecraft version (default release)" },
                ],
                options: vec![],
            },
            ExecCommand {
                command: "info",
                description: "Show one instance's configuration (name, version, loader, path)",
                args: vec![
                    ArgSpec { name: "name", required: true, description: "Instance name" },
                ],
                options: vec![],
            },
            ExecCommand {
                command: "launch",
                description: "Launch an instance (always detached when run via the agent API)",
                args: vec![
                    ArgSpec { name: "name", required: true, description: "Instance name" },
                ],
                options: vec![
                    OptionSpec { key: "username", values: "<name>", description: "Offline username" },
                    OptionSpec { key: "server", values: "host[:port]", description: "Auto-connect to a server" },
                    OptionSpec { key: "fullscreen", values: "true", description: "Launch fullscreen" },
                    OptionSpec { key: "width", values: "<px>", description: "Window width" },
                    OptionSpec { key: "height", values: "<px>", description: "Window height" },
                    OptionSpec { key: "agent", values: "true", description: "Enable in-game Despotes control" },
                    OptionSpec { key: "agent-port", values: "<port>", description: "Despotes control port (default 25585)" },
                    OptionSpec { key: "java-path", values: "<path>", description: "Custom java executable" },
                    OptionSpec { key: "memory", values: "4G|2048M", description: "Memory allocation" },
                    OptionSpec { key: "aprism", values: "true", description: "Attach the Aprism JE javaagent" },
                    OptionSpec { key: "enter-test-world", values: "true", description: "Auto-enter test world when ready" },
                    OptionSpec { key: "no-queue", values: "true", description: "Skip the instance launch queue" },
                    OptionSpec { key: "idle-timeout", values: "<seconds>", description: "Idle watchdog timeout (default 60s; 0=use default)" },
                    OptionSpec { key: "no-idle-timeout", values: "true", description: "Disable the idle watchdog entirely" },
                    OptionSpec { key: "oom-protect", values: "true|false", description: "Enable OOM self-protection: kill stale MC processes, trim working sets (default: true)" },
                    OptionSpec { key: "oom-aggressive", values: "true", description: "Aggressive OOM protection: also purge system standby list (requires admin)" },
                    OptionSpec { key: "oom-confirm", values: "auto|always|never", description: "Second-confirmation policy before killing stale processes (default auto)" },
                    OptionSpec { key: "oom-list-only", values: "true", description: "List OOM sweep candidates without terminating" },
                    OptionSpec { key: "javaagent", values: "<jar>[,<jar=params>...]", description: "Ad-hoc JavaAgent JAR(s) to attach at launch (comma-separated)" },
                ],
            },
            ExecCommand {
                command: "stop",
                description: "Stop a running instance (kills its game process tree)",
                args: vec![
                    ArgSpec { name: "name", required: true, description: "Instance name" },
                ],
                options: vec![],
            },
            // v26.3-alpha.1: observability + lifecycle mappings.
            ExecCommand {
                command: "metrics",
                description: "Launch metrics for an instance (latest or full history)",
                args: vec![
                    ArgSpec { name: "instance", required: true, description: "Instance name" },
                ],
                options: vec![
                    OptionSpec { key: "history", values: "true", description: "Return the recorded history instead of the latest launch" },
                ],
            },
            ExecCommand {
                command: "disk",
                description: "Disk usage of an instance with a top-level breakdown",
                args: vec![
                    ArgSpec { name: "instance", required: true, description: "Instance name" },
                ],
                options: vec![],
            },
            ExecCommand {
                command: "inject-agent",
                description: "Hot-attach a Java agent JAR into the running game JVM",
                args: vec![
                    ArgSpec { name: "instance", required: true, description: "Instance name (must be running)" },
                    ArgSpec { name: "jar", required: true, description: "Agent JAR path or registered javaagent name" },
                ],
                options: vec![
                    OptionSpec { key: "params", values: "<text>", description: "Agent options string (after '=' in -javaagent syntax)" },
                    OptionSpec { key: "java-path", values: "<path>", description: "Java executable used to run the attach helper" },
                ],
            },
            ExecCommand {
                command: "server-cmd",
                description: "Run a console command on a managed server via RCON",
                args: vec![
                    ArgSpec { name: "server", required: true, description: "Server name" },
                    ArgSpec { name: "command", required: true, description: "Console command text (remaining args joined)" },
                ],
                options: vec![],
            },
        ],
        game_inputs: vec![
            GameInput {
                input_type: "key",
                description: "Press/hold keyboard keys (names like key.keyboard.w)",
                fields: vec![
                    ArgSpec { name: "key", required: true, description: "Key name" },
                    ArgSpec { name: "action", required: false, description: "tap|hold (default tap)" },
                    ArgSpec { name: "hold_ms", required: false, description: "Milliseconds to hold when action=hold" },
                ],
            },
            GameInput {
                input_type: "look",
                description: "Rotate the player view (degrees; relative=false means absolute)",
                fields: vec![
                    ArgSpec { name: "yaw", required: true, description: "Yaw in degrees" },
                    ArgSpec { name: "pitch", required: true, description: "Pitch in degrees" },
                    ArgSpec { name: "relative", required: false, description: "true=delta, false=absolute (default false)" },
                ],
            },
            GameInput {
                input_type: "click",
                description: "Mouse click at GUI coordinates",
                fields: vec![
                    ArgSpec { name: "x", required: false, description: "X coordinate" },
                    ArgSpec { name: "y", required: false, description: "Y coordinate" },
                    ArgSpec { name: "button", required: false, description: "left|right|middle (default left)" },
                    ArgSpec { name: "action", required: false, description: "tap|hold (default tap)" },
                    ArgSpec { name: "hold_ms", required: false, description: "Milliseconds to hold when action=hold" },
                ],
            },
            GameInput {
                input_type: "scroll",
                description: "Scroll the hotbar",
                fields: vec![
                    ArgSpec { name: "amount", required: true, description: "Scroll amount (negative scrolls down)" },
                ],
            },
            GameInput {
                input_type: "chat",
                description: "Send a chat message or /command",
                fields: vec![
                    ArgSpec { name: "message", required: true, description: "Message or command text" },
                ],
            },
        ],
        events: EventsSpec {
            websocket_path: "/api/v1/events",
            kinds: vec![
                "launch_started",
                "launch_progress",
                "launch_completed",
                "launch_failed",
                "log_line",
                "instance_stopped",
                "game_ready",
                "game_idle_timeout",
            ],
        },
        error_codes: vec![
            ErrorCodeSpec { code: "UNKNOWN_COMMAND",  http_status: 400, description: "The execute command name is not recognized" },
            ErrorCodeSpec { code: "BAD_REQUEST",      http_status: 400, description: "A required argument is missing or invalid" },
            ErrorCodeSpec { code: "NOT_FOUND",        http_status: 404, description: "The referenced instance does not exist" },
            ErrorCodeSpec { code: "ALREADY_EXISTS",   http_status: 409, description: "An instance with that name already exists" },
            ErrorCodeSpec { code: "NOT_RUNNING",      http_status: 409, description: "The instance is not running (stop was requested)" },
            ErrorCodeSpec { code: "BUSY",             http_status: 409, description: "Another instance holds the launch lock" },
            ErrorCodeSpec { code: "NOT_IMPLEMENTED",   http_status: 501, description: "The endpoint is not supported on this platform" },
            ErrorCodeSpec { code: "SERVICE_UNAVAILABLE", http_status: 503, description: "The required component (Despotes, game process) is not available" },
            ErrorCodeSpec { code: "BAD_GATEWAY",      http_status: 502, description: "An upstream request (e.g. to the Despotes mod) failed" },
            ErrorCodeSpec { code: "GONE",             http_status: 410, description: "The game process is no longer running (idle-status after exit)" },
            ErrorCodeSpec { code: "INTERNAL",         http_status: 500, description: "Unclassified internal error" },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_is_valid_and_complete() {
        let caps = manifest();
        assert_eq!(caps.launcher, "mdl");
        assert_eq!(caps.schema, "mdl.capabilities/v1");
        // Every core control endpoint must be present.
        for path in [
            "/api/v1/status",
            "/api/v1/execute",
            "/api/v1/capabilities",
            "/api/v1/game/:instance/status",
            "/api/v1/game/:instance/input",
            "/api/v1/game/:instance/idle-status",
            "/api/v1/instance/:instance/metrics",
            "/api/v1/instance/:instance/disk",
        ] {
            assert!(
                caps.endpoints.iter().any(|e| e.path == path),
                "missing endpoint {}",
                path
            );
        }
        // All execute commands must be declared (v26.1-alpha.2 adds stop;
        // v26.3-alpha.1 adds metrics/disk/inject-agent/server-cmd).
        let cmds: Vec<&str> = caps.execute_commands.iter().map(|c| c.command).collect();
        for c in ["list", "create", "info", "launch", "stop", "metrics", "disk", "inject-agent", "server-cmd"] {
            assert!(cmds.contains(&c), "missing execute command {}", c);
        }
        // Machine-readable error codes must be declared and non-empty.
        assert!(!caps.error_codes.is_empty(), "error_codes must be declared");
        for ec in &caps.error_codes {
            assert!(!ec.code.is_empty());
            assert!(ec.http_status >= 400);
        }
        // All five game input types must be declared.
        let types: Vec<&str> = caps.game_inputs.iter().map(|g| g.input_type).collect();
        for t in ["key", "look", "click", "scroll", "chat"] {
            assert!(types.contains(&t), "missing game input {}", t);
        }
        // Event stream contract must include all actual ServerEvent kinds.
        for k in [
            "launch_started",
            "launch_progress",
            "launch_completed",
            "launch_failed",
            "log_line",
            "instance_stopped",
            "game_ready",
            "game_idle_timeout",
        ] {
            assert!(caps.events.kinds.contains(&k), "missing event kind {}", k);
        }
        // Launch command must declare the idle-timeout option (v26.2-alpha.1).
        let launch_cmd = caps.execute_commands.iter().find(|c| c.command == "launch").unwrap();
        let launch_opts: Vec<&str> = launch_cmd.options.iter().map(|o| o.key).collect();
        assert!(launch_opts.contains(&"idle-timeout"), "launch missing idle-timeout option");
        assert!(launch_opts.contains(&"no-idle-timeout"), "launch missing no-idle-timeout option");
        // v26.3-alpha.2 OOM confirmation options.
        assert!(launch_opts.contains(&"oom-confirm"), "launch missing oom-confirm option");
        assert!(launch_opts.contains(&"oom-list-only"), "launch missing oom-list-only option");
        assert!(launch_opts.contains(&"oom-protect"), "launch missing oom-protect option");
        assert!(launch_opts.contains(&"oom-aggressive"), "launch missing oom-aggressive option");
        assert!(launch_opts.contains(&"javaagent"), "launch missing javaagent option");
    }

    #[test]
    fn test_manifest_serializes_to_json() {
        let caps = manifest();
        let json = serde_json::to_string(&caps).expect("serialize");
        assert!(json.contains("mdl.capabilities/v1"));
        assert!(json.contains("game_inputs"));
        // Round-trip through Value to guarantee it is well-formed JSON.
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert!(v.get("endpoints").is_some());
    }

    #[test]
    fn test_execute_commands_have_required_args() {
        let caps = manifest();
        for cmd in &caps.execute_commands {
            // Every declared required arg must carry a non-empty name.
            for arg in cmd.args.iter().filter(|a| a.required) {
                assert!(!arg.name.is_empty(), "empty required arg in {}", cmd.command);
            }
            assert!(!cmd.description.is_empty(), "empty description in {}", cmd.command);
        }
    }
}
