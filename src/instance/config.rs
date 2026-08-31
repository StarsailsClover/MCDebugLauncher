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
    /// Instance-level Java runtime binding (v26.5-alpha.3): `aprism` or
    /// `aprism@<tag|version>` (see `mdl jdk use`). Absent/null = automatic
    /// selection (system Java, else Eclipse Adoptium provisioning). A
    /// launch-time `--jdk`/`--java-path` still overrides for that launch;
    /// an unresolvable binding degrades to the standard chain with a
    /// warning (same as the CLI fallback).
    #[serde(default)]
    pub jdk: Option<String>,
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

// GitHub@NDBlockConnect | BlockConnect@StarsailsClover

#[cfg(test)]
mod tests {
    use super::*;

    /// v26.5-alpha.3: the jdk binding must default cleanly on configs
    /// written before the field existed (serde default) and round-trip.
    #[test]
    fn test_jdk_binding_backcompat_and_roundtrip() {
        let old = r#"{"name":"x","version":"26.2","loader":null}"#;
        let cfg: InstanceConfig = serde_json::from_str(old).unwrap();
        assert!(cfg.jdk.is_none(), "old configs must parse with jdk=None");

        let mut cfg = cfg;
        cfg.jdk = Some("aprism@26.2".into());
        let text = serde_json::to_string(&cfg).unwrap();
        let back: InstanceConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(back.jdk.as_deref(), Some("aprism@26.2"));
        assert_eq!(back.name, "x");
    }
}
