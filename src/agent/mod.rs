// Agent interface module
// Provides HTTP/WebSocket API for programmatic control by AI agents.
//
// v26.2-alpha.3: the old `commands` and `events` modules (dead placeholders)
// have been removed. The actual command execution lives in
// `server::execute_command`, and the actual event type is `ServerEvent`
// in `server.rs`. Both are the single source of truth for what the agent
// API can do.

pub mod server;
pub mod capabilities;
pub mod orchestration;
