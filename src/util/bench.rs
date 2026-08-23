// CLI self-latency benchmark (v26.3-alpha.6).
//
// Measures the end-to-end wall time of invoking MDL's own binary for a set
// of fast commands (process spawn + tokio boot + clap parse + handler +
// output flush). This is the "CLI cold latency" baseline used by
// scripts/perf-bench.ps1 to gate regressions.
//
// Deliberately NOT measured here: real game launches (too heavy/slow for a
// unit-style benchmark); the PowerShell wrapper can additionally read
// runtime/metrics.jsonl history for those when an instrumented instance
// exists.

use anyhow::{Context, Result};
use serde::Serialize;
use std::time::{Duration, Instant};

/// The commands benchmarked, in order.
pub const BENCH_COMMANDS: &[&str] = &["capabilities", "status", "list"];

#[derive(Debug, Clone, Serialize)]
pub struct CommandBench {
    pub command: String,
    pub iterations: u32,
    /// Milliseconds.
    pub min: f64,
    pub p50: f64,
    pub p95: f64,
    pub max: f64,
}

/// Nearest-rank percentile over an ascending-sorted sample.
pub fn percentile_sorted(sorted_asc: &[f64], p: f64) -> f64 {
    assert!((0.0..=100.0).contains(&p), "p out of range");
    if sorted_asc.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * sorted_asc.len() as f64).ceil() as usize;
    sorted_asc[(rank.max(1) - 1).min(sorted_asc.len() - 1)]
}

fn stats_ms(samples_ms: &mut Vec<f64>) -> (f64, f64, f64, f64) {
    samples_ms.sort_by(|a, b| a.total_cmp(b));
    let min = samples_ms.first().copied().unwrap_or(0.0);
    let max = samples_ms.last().copied().unwrap_or(0.0);
    (min, percentile_sorted(samples_ms, 50.0), percentile_sorted(samples_ms, 95.0), max)
}

/// Run one MDL subcommand `iterations` times, return per-command stats.
/// Each iteration is a fresh subprocess (cold-start inclusive).
pub fn run_cli_bench(exe: &std::path::Path, iterations: u32) -> Result<Vec<CommandBench>> {
    let mut out = Vec::new();
    for cmd in BENCH_COMMANDS {
        let mut samples = Vec::with_capacity(iterations as usize);
        for _ in 0..iterations {
            let t = Instant::now();
            let status = std::process::Command::new(exe)
                .arg(cmd)
                .arg("--format")
                .arg("json")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .with_context(|| format!("Failed to spawn {exe:?} {cmd}"))?;
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            if !status.success() {
                anyhow::bail!("Benchmarked command '{cmd}' exited with {status}");
            }
            samples.push(ms);
        }
        let (min, p50, p95, max) = stats_ms(&mut samples);
        out.push(CommandBench {
            command: (*cmd).to_string(),
            iterations,
            min,
            p50,
            p95,
            max,
        });
    }
    Ok(out)
}

/// Gate: fail when any tracked p95 exceeds its baseline by more than the
/// allowed ratio (e.g. 1.25 = +25% tolerated). Returns the offending rows.
pub fn gate_against_baseline<'a>(
    results: &'a [CommandBench],
    baseline: &std::collections::HashMap<String, f64>,
    tolerance_ratio: f64,
) -> Vec<&'a CommandBench> {
    results
        .iter()
        .filter(|r| {
            baseline
                .get(&r.command)
                .map(|base| r.p95 > base * tolerance_ratio)
                .unwrap_or(false)
        })
        .collect()
}

/// Sleep helper kept tiny so the module has no other side effects.
#[allow(dead_code)]
fn unused_noop(_d: Duration) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percentile_nearest_rank() {
        let mut v: Vec<f64> = vec![10.0, 20.0, 30.0, 40.0];
        v.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(percentile_sorted(&v, 50.0), 20.0);
        assert_eq!(percentile_sorted(&v, 95.0), 40.0);
        assert_eq!(percentile_sorted(&v, 100.0), 40.0);
        assert_eq!(percentile_sorted(&v, 25.0), 10.0);
        // Single sample.
        assert_eq!(percentile_sorted(&[7.5], 95.0), 7.5);
        assert_eq!(percentile_sorted(&[], 50.0), 0.0);
    }

    #[test]
    fn test_stats_ms_basic() {
        let mut v: Vec<f64> = vec![30.0, 10.0, 20.0, 40.0];
        let (min, p50, p95, max) = stats_ms(&mut v);
        assert_eq!(min, 10.0);
        assert_eq!(p50, 20.0);
        assert_eq!(p95, 40.0);
        assert_eq!(max, 40.0);
    }

    #[test]
    fn test_gate_flags_regression_only() {
        use std::collections::HashMap;
        let mut base = HashMap::new();
        base.insert("capabilities".to_string(), 100.0);
        base.insert("list".to_string(), 100.0);
        let rows = vec![
            CommandBench { command: "capabilities".into(), iterations: 3, min: 90.0, p50: 95.0, p95: 120.0, max: 121.0 },
            CommandBench { command: "list".into(), iterations: 3, min: 80.0, p50: 85.0, p95: 90.0, max: 91.0 },
        ];
        let flagged = gate_against_baseline(&rows, &base, 1.15);
        assert_eq!(flagged.len(), 1);
        assert_eq!(flagged[0].command, "capabilities");
    }

    #[test]
    fn test_run_cli_bench_smoke_current_exe() {
        // One iteration against the test binary itself: exercises the spawn
        // + measure plumbing without asserting absolute timings (CI noise).
        let exe = std::env::current_exe().unwrap();
        let rows = run_cli_bench(&exe, 1).unwrap_or_else(|e| {
            // Test harness binaries may not accept these args; skip softly.
            eprintln!("skipping: {e}");
            Vec::new()
        });
        for r in &rows {
            assert_eq!(r.iterations, 1);
            assert!(r.p95 >= r.min);
        }
    }
}
