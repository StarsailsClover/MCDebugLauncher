// Instance configuration

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceConfig {
    pub name: String,
    pub version: String,
    pub loader: Option<LoaderConfig>,
    /// Registered JavaAgents (v26.2-alpha.5). Attached as `-javaagent` at
    /// launch when `enabled` is true. The JAR is copied into the instance's
    /// `javaagents/` directory; `path` is relative to the instance root.
    #[serde(default)]
    pub javaagents: Vec<JavaAgentEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderConfig {
    pub loader_type: String,
    pub version: String,
}

/// A registered JavaAgent in an instance configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaAgentEntry {
    /// Display name (defaults to JAR filename without extension).
    pub name: String,
    /// Path to the JAR, relative to the instance root
    /// (e.g. `javaagents/my-agent.jar`).
    pub path: String,
    /// Optional parameters passed after `=` (e.g. `port=25585`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<String>,
    /// Whether the agent is attached at launch.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}
