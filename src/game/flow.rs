//! Declarative orchestration flow primitives (v26.5-alpha.9).
//!
//! This module owns parsing, validation and condition evaluation only. The
//! executor remains in the CLI/agent layer and submits existing Despotes
//! actions, so the DSL does not create a second game-control protocol.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// GitHub@NDBlockConnect | BlockConnect@StarsailsClover

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Flow {
    pub name: String,
    pub steps: Vec<FlowStep>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FlowStep {
    WaitReady { #[serde(default = "default_ready_timeout")] timeout_secs: u64 },
    WaitCondition {
        #[serde(rename = "if")]
        r#if: Value,
        #[serde(default = "default_condition_timeout")] timeout_secs: u64,
        #[serde(default = "default_poll_ms")] poll_ms: u64,
    },
    Action { command: Value },
    Schedule {
        op: String,
        #[serde(default)] name: Option<String>,
        #[serde(default, rename = "periodTicks")] period_ticks: Option<u64>,
        #[serde(default)] commands: Vec<Value>,
    },
    Macro {
        op: String,
        #[serde(default)] name: Option<String>,
        #[serde(default)] step: Option<Value>,
    },
    Sleep { secs: f64 },
}

fn default_ready_timeout() -> u64 { 120 }
fn default_condition_timeout() -> u64 { 60 }
fn default_poll_ms() -> u64 { 500 }

pub fn parse_and_validate(raw: &str) -> Result<Flow> {
    let flow: Flow = serde_json::from_str(raw).map_err(|e| anyhow::anyhow!("invalid flow JSON: {e}"))?;
    validate(&flow)?;
    Ok(flow)
}

pub fn validate(flow: &Flow) -> Result<()> {
    if flow.name.trim().is_empty() || flow.name.len() > 128 {
        bail!("flow name must be 1-128 characters");
    }
    if flow.steps.is_empty() || flow.steps.len() > 256 {
        bail!("flow steps must contain 1-256 entries");
    }
    for (i, step) in flow.steps.iter().enumerate() {
        match step {
            FlowStep::WaitReady { timeout_secs } | FlowStep::WaitCondition { timeout_secs, .. }
                if *timeout_secs == 0 || *timeout_secs > 86_400 =>
                bail!("step {i}: timeout_secs must be 1-86400"),
            FlowStep::WaitCondition { poll_ms, .. } if *poll_ms == 0 || *poll_ms > 60_000 =>
                bail!("step {i}: poll_ms must be 1-60000"),
            FlowStep::Sleep { secs } if !secs.is_finite() || *secs < 0.0 || *secs > 86_400.0 =>
                bail!("step {i}: secs must be finite and within 0-86400"),
            FlowStep::Action { command } if !command.is_object() =>
                bail!("step {i}: action command must be an object"),
            FlowStep::Schedule { op, .. } if !matches!(op.as_str(), "add" | "status" | "remove") =>
                bail!("step {i}: unsupported schedule op {op:?}"),
            FlowStep::Macro { op, .. } if !matches!(op.as_str(), "start-recording" | "record-step" | "stop-recording" | "play" | "stop" | "delete" | "status") =>
                bail!("step {i}: unsupported macro op {op:?}"),
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompareOp { Exists, Eq, Ne, Gt, Lt, Contains }

pub fn condition_matches(value: &Value, field: &str, op: CompareOp, expected: Option<&Value>) -> bool {
    let actual = field.split('.').fold(Some(value), |cur, key| cur?.get(key));
    match op {
        CompareOp::Exists => actual.is_some(),
        CompareOp::Eq => actual == expected,
        CompareOp::Ne => actual != expected,
        CompareOp::Gt => numeric(actual) > numeric(expected),
        CompareOp::Lt => numeric(actual) < numeric(expected),
        CompareOp::Contains => match (actual, expected) {
            (Some(Value::String(a)), Some(Value::String(b))) => a.contains(b),
            (Some(Value::Array(a)), Some(b)) => a.iter().any(|x| x == b),
            _ => false,
        },
    }
}

fn numeric(value: Option<&Value>) -> f64 {
    value.and_then(Value::as_f64).unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // GitHub@NDBlockConnect | BlockConnect@StarsailsClover
    #[test]
    fn validates_and_defaults_flow_steps() {
        let f = parse_and_validate(r#"{"name":"demo","steps":[{"type":"wait-ready"},{"type":"sleep","secs":1.5}]}"#).unwrap();
        assert_eq!(f.steps.len(), 2);
        assert!(parse_and_validate(r#"{"name":"","steps":[]}"#).is_err());
        assert!(parse_and_validate(r#"{"name":"x","steps":[{"type":"sleep","secs":-1}]}"#).is_err());
    }

    /// Field regression (v26.5-alpha.9): flow files written by Windows
    /// editors carry a UTF-8 BOM; the CLI must strip it before parsing
    /// (same class as the v26.3 config BOM tolerance).
    #[test]
    fn cli_path_strips_bom_before_parse() {
        let raw = b"\xEF\xBB\xBF{\"name\":\"demo\",\"steps\":[{\"type\":\"sleep\",\"secs\":1}]}";
        let stripped = crate::util::jsonio::strip_bom(raw);
        let text = String::from_utf8(stripped.to_vec()).unwrap();
        assert!(parse_and_validate(&text).is_ok());
        // Sanity: the raw BOM bytes really would have failed.
        assert!(parse_and_validate(&String::from_utf8(raw.to_vec()).unwrap()).is_err());
    }

    #[test]
    fn evaluates_six_condition_operators() {
        let value = json!({"result":{"inGame":true,"name":"alpha","n":4},"items":["a","b"]});
        assert!(condition_matches(&value, "result.inGame", CompareOp::Exists, None));
        assert!(condition_matches(&value, "result.n", CompareOp::Eq, Some(&json!(4))));
        assert!(condition_matches(&value, "result.n", CompareOp::Ne, Some(&json!(3))));
        assert!(condition_matches(&value, "result.n", CompareOp::Gt, Some(&json!(3))));
        assert!(condition_matches(&value, "result.n", CompareOp::Lt, Some(&json!(5))));
        assert!(condition_matches(&value, "result.name", CompareOp::Contains, Some(&json!("lph"))));
        assert!(condition_matches(&value, "items", CompareOp::Contains, Some(&json!("b"))));
        assert!(!condition_matches(&value, "result.missing", CompareOp::Exists, None));
    }
}
