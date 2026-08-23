# perf-bench.ps1 - MDL performance baseline runner / regression gate
# (v26.3-alpha.6)
#
# Modes:
#   1. Measure only:
#        .\scripts\perf-bench.ps1 -Iterations 20
#   2. Save a new baseline:
#        .\scripts\perf-bench.ps1 -Iterations 30 -SaveBaseline perf-baseline.json
#   3. Gate against an existing baseline (CI-friendly; exits 2 on regression):
#        .\scripts\perf-bench.ps1 -Baseline perf-baseline.json -ToleranceRatio 1.25 -Gate
#
# Optional launch-pipeline stats: pass -Instance <name> to also summarize
# spawn/ready timings from that instance's runtime/metrics.jsonl history
# (no launches are performed by this script).
#
# Baseline JSON shape (written by -SaveBaseline):
# {
#   "generated": "2026-08-23T...",
#   "iterations": 30,
#   "p95_ms": { "capabilities": 41.2, "status": 38.7, "list": 39.9 }
# }
#
# NOTE: baselines are HOST-SPECIFIC (absolute ms depend on the machine).
# Never commit one for CI gating on other machines; generate per-runner.
# Known finding (alpha.6): 'mdl status' over many instances costs ~0.5-2s
# (per-instance sysinfo probing) - tracked for a later optimization pass.

param(
    [int]$Iterations = 20,
    [string]$MdlPath = "",
    [string]$SaveBaseline = "",
    [string]$Baseline = "",
    [double]$ToleranceRatio = 1.25,
    [switch]$Gate,
    [string]$Instance = ""
)

$ErrorActionPreference = "Stop"

if (-not $MdlPath) {
    $MdlPath = (Get-Command mdl -ErrorAction SilentlyContinue).Source
    if (-not $MdlPath) { Write-Error "mdl not found on PATH; pass -MdlPath."; exit 1 }
}

Write-Host "=== MDL Performance Benchmark ==="
Write-Host "Binary : $MdlPath"
Write-Host "Iter   : $Iterations per command"
Write-Host ""

# --- Run the built-in bench and parse its JSON output ---
$raw = & $MdlPath bench --iterations $Iterations --format json 2>$null | Out-String
$bench = $raw | ConvertFrom-Json
if (-not $bench -or -not $bench.data) { Write-Error "Failed to run 'mdl bench'."; exit 1 }

$rows = @()
foreach ($c in $bench.data.commands) {
    $rows += [pscustomobject]@{
        Command = $c.command
        Min     = [math]::Round($c.min, 1)
        P50     = [math]::Round($c.p50, 1)
        P95     = [math]::Round($c.p95, 1)
        Max     = [math]::Round($c.max, 1)
    }
}
$rows | Format-Table -AutoSize | Out-Host | Out-Null
$rows | Format-Table -AutoSize

$p95map = @{}
foreach ($c in $bench.data.commands) { $p95map[$c.command] = $c.p95 }

# --- Optional: launch-pipeline metrics from history ---
if ($Instance) {
    $instancesDir = Join-Path $env:APPDATA "mdl\instances"
    $histPath = Join-Path $instancesDir "$Instance\runtime\metrics.jsonl"
    if (Test-Path $histPath) {
        $recs = Get-Content $histPath | ForEach-Object { try { $_ | ConvertFrom-Json } catch {} }
        if ($recs) {
            $spawn = $recs | ForEach-Object { $_.spawn_secs * 1000 } | Sort-Object
            $ready = $recs | Where-Object { $_.ready_secs } | ForEach-Object { $_.ready_secs * 1000 } | Sort-Object
            Write-Host "--- Launch pipeline history ($Instance, $($recs.Count) record(s)) ---"
            Write-Host ("spawn ms : min={0:N0} p50={1:N0} max={2:N0}" -f $spawn[0], $spawn[[int][math]::Floor($spawn.Count*0.5)], $spawn[-1])
            if ($ready) {
                Write-Host ("ready ms : min={0:N0} p50={1:N0} max={2:N0}" -f $ready[0], $ready[[int][math]::Floor($ready.Count*0.5)], $ready[-1])
            } else {
                Write-Host "ready ms : no --wait-ready records"
            }
        }
    } else {
        Write-Host "(no metrics.jsonl for '$Instance' - launch it with metrics recording first)"
    }
    Write-Host ""
}

# --- Save baseline ---
if ($SaveBaseline) {
    $obj = [ordered]@{
        generated  = (Get-Date).ToUniversalTime().ToString("o")
        iterations = $Iterations
        p95_ms     = $p95map
    }
    $obj | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 $SaveBaseline
    Write-Host "Baseline saved: $SaveBaseline"
    exit 0
}

# --- Gate against baseline ---
if ($Gate) {
    if (-not $Baseline -or -not (Test-Path $Baseline)) {
        Write-Error "Gate mode requires -Baseline pointing at an existing baseline file."; exit 1
    }
    $base = Get-Content $Baseline -Raw | ConvertFrom-Json
    $failed = @()
    foreach ($cmd in $base.p95_ms.PSObject.Properties.Name) {
        if (-not $p95map.ContainsKey($cmd)) { continue }
        $limit = $base.p95_ms.$cmd * $ToleranceRatio
        $actual = $p95map[$cmd]
        $ok = $actual -le $limit
        $state = if ($ok) { "OK " } else { "FAIL" }
        Write-Host ("[{0}] {1,-14} p95 {2,8:N1}ms  limit {3,8:N1}ms  (baseline {4,8:N1} x {5})" -f `
            $state, $cmd, $actual, $limit, $base.p95_ms.$cmd, $ToleranceRatio)
        if (-not $ok) { $failed += $cmd }
    }
    if ($failed.Count -gt 0) {
        Write-Error ("PERFORMANCE REGRESSION: " + ($failed -join ", "))
        exit 2
    }
    Write-Host "Gate passed: no p95 regression beyond ${ToleranceRatio}x baseline."
    exit 0
}

Write-Host "Tip: -SaveBaseline <file> to record; -Baseline <file> -Gate to enforce."
