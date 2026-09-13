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
    // NOTE (v26.5-alpha.10 bug fix): `rename_all = "kebab-case"` applies ONLY
    // to the tag value, not to the variant fields. Every camelCase field the
    // documented DSL uses must therefore carry an explicit rename, otherwise
    // serde silently falls back to the `default` and the parameter is ignored
    // (a flow asking for timeoutSecs:30 quietly waited the 120s default).
    WaitReady {
        #[serde(default = "default_ready_timeout", rename = "timeoutSecs")]
        timeout_secs: u64,
    },
    WaitCondition {
        #[serde(rename = "if")]
        r#if: Value,
        #[serde(default = "default_condition_timeout", rename = "timeoutSecs")]
        timeout_secs: u64,
        #[serde(default = "default_poll_ms", rename = "pollMs")]
        poll_ms: u64,
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
    let actual = field.split('.').fold(Some(value), |cur, key| {
        let cur = cur?;
        // v26.5-alpha.10 (e2e finding): point paths must address array
        // elements too - `schedules.0.executionCount` is the natural way to
        // pin a schedule in the status response. serde_json's str-indexed
        // `get` returns None for arrays, so numeric keys index by position.
        match cur {
            Value::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => cur.get(key),
        }
    });
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

    /// Field regression (v26.5-alpha.10): `rename_all` on the enum does NOT
    /// rename variant fields, so camelCase DSL keys must be explicit. Before
    /// the fix, `timeoutSecs`/`pollMs` were silently dropped and the defaults
    /// took over (an e2e flow asking for 30s waited 120s and timed out).
    #[test]
    fn camel_case_step_fields_are_not_silently_ignored() {
        let f = parse_and_validate(
            r#"{"name":"x","steps":[
                {"type":"wait-ready","timeoutSecs":30},
                {"type":"wait-condition","timeoutSecs":45,"pollMs":250,
                 "if":{"field":"inGame","op":"exists"}},
                {"type":"schedule","op":"add","name":"s","periodTicks":40,
                 "commands":[{"type":"ping"}]}
            ]}"#,
        )
        .unwrap();

        match &f.steps[0] {
            FlowStep::WaitReady { timeout_secs } => assert_eq!(*timeout_secs, 30, "timeoutSecs ignored"),
            other => panic!("wrong step: {other:?}"),
        }
        match &f.steps[1] {
            FlowStep::WaitCondition { timeout_secs, poll_ms, .. } => {
                assert_eq!(*timeout_secs, 45, "timeoutSecs ignored");
                assert_eq!(*poll_ms, 250, "pollMs ignored");
            }
            other => panic!("wrong step: {other:?}"),
        }
        match &f.steps[2] {
            FlowStep::Schedule { period_ticks, .. } => {
                assert_eq!(*period_ticks, Some(40), "periodTicks ignored")
            }
            other => panic!("wrong step: {other:?}"),
        }
    }

    /// Defaults still apply when the optional fields are absent.
    #[test]
    fn omitted_optional_fields_fall_back_to_defaults() {
        let f = parse_and_validate(
            r#"{"name":"x","steps":[{"type":"wait-ready"},{"type":"wait-condition","if":{"field":"a"}}]}"#,
        )
        .unwrap();
        match &f.steps[0] {
            FlowStep::WaitReady { timeout_secs } => assert_eq!(*timeout_secs, 120),
            other => panic!("wrong step: {other:?}"),
        }
        match &f.steps[1] {
            FlowStep::WaitCondition { timeout_secs, poll_ms, .. } => {
                assert_eq!(*timeout_secs, 60);
                assert_eq!(*poll_ms, 500);
            }
            other => panic!("wrong step: {other:?}"),
        }
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

    /// e2e regression (v26.5-alpha.10): array indexing in point paths.
    /// Found by the alpha.10 production e2e flow - `schedules.0.executionCount`
    /// never matched because serde_json's str-indexed get() returns None for
    /// arrays.
    #[test]
    fn point_paths_index_into_arrays() {
        let value = json!({"count":1,"schedules":[
            {"name":"a10hb","executionCount":4,"periodTicks":40}
        ]});
        assert!(condition_matches(&value, "schedules.0.executionCount", CompareOp::Gt, Some(&json!(0))));
        assert!(condition_matches(&value, "schedules.0.name", CompareOp::Eq, Some(&json!("a10hb"))));
        assert!(condition_matches(&value, "schedules.1.executionCount", CompareOp::Exists, None) == false);
        assert!(condition_matches(&value, "schedules.abc", CompareOp::Exists, None) == false,
            "non-numeric key on an array must not resolve");
    }
}
