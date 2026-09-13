// Environment health check (`mdl doctor`, Alpha 12).
//
// A read-only self-check of everything MDL needs to work well: the Java
// runtime, the data/cache directory layout, the download cache, the mirror
// network (latency probe), the instances directory, and reachability of the
// external ecosystems MDL integrates with (Mojang, Modrinth, GitHub).
//
// The check never modifies anything; it only reads and reports. Each item is
// rendered as `[OK]` / `[WARN]` / `[FAIL]` with a short detail line so both
// humans and agents can consume the output.

use std::path::PathBuf;
use std::time::Instant;

/// One health-check finding.
pub struct CheckResult {
    pub name: &'static str,
    pub ok: bool,
    /// Informational detail (always present).
    pub detail: String,
    /// Non-fatal caveat attached to an otherwise-passing check.
    pub warn: bool,
}

/// Outcome of the full environment check.
pub struct DoctorReport {
    pub checks: Vec<CheckResult>,
}

impl DoctorReport {
    pub fn pass_count(&self) -> usize {
        self.checks.iter().filter(|c| c.ok).count()
    }

    pub fn fail_count(&self) -> usize {
        self.checks.iter().filter(|c| !c.ok).count()
    }

    /// Render the report as plain text lines.
    pub fn render(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for c in &self.checks {
            let mark = if !c.ok {
                "[FAIL]"
            } else if c.warn {
                "[WARN]"
            } else {
                "[OK]  "
            };
            lines.push(format!("{} {:<14} {}", mark, c.name, c.detail));
        }
        lines.push(String::new());
        lines.push(format!(
            "Result: {} passed, {} failed",
            self.pass_count(),
            self.fail_count()
        ));
        lines
    }
}

/// Run every environment check. Network checks use short timeouts so the
/// command stays snappy even when offline.
pub async fn run_all() -> DoctorReport {
    let mut checks = Vec::new();
    checks.push(check_java());
    checks.push(check_data_dirs());
    checks.push(check_cache());
    checks.push(check_mirrors().await);
    checks.push(check_instances().await);
    checks.push(check_jdk_bindings().await);
    checks.push(check_mojang().await);
    checks.push(check_modrinth().await);
    checks.push(check_github().await);
    checks.push(check_mdl_duplicates());
    DoctorReport { checks }
}

/// v26.3-alpha.9: detect multiple mdl.exe copies resolvable on PATH.
/// Field context: stale copies (e.g. an old zip extracted into a Downloads
/// folder) silently shadow newer installs and caused "update didn't work"
/// confusion. WARN lists every hit so the user can prune; a single hit is
/// an informational PASS.
fn check_mdl_duplicates() -> CheckResult {
    let out = std::process::Command::new("where.exe")
        .arg("mdl")
        .output();
    let paths: Vec<String> = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
        _ => Vec::new(),
    };

    match paths.len() {
        0 => CheckResult {
            name: "mdl-on-path",
            ok: true,
            warn: true,
            detail: "No mdl.exe found on PATH (running from a direct path?)".into(),
        },
        1 => CheckResult {
            name: "mdl-on-path",
            ok: true,
            warn: false,
            detail: format!("Single install on PATH: {}", paths[0]),
        },
        n => CheckResult {
            name: "mdl-on-path",
            ok: true,
            warn: true,
            detail: format!(
                "{n} mdl copies on PATH — stale copies may shadow updates:\n    {}",
                paths.join("\n    ")
            ),
        },
    }
}

fn check_java() -> CheckResult {
    match crate::version::java::JavaRuntime::detect() {
        Ok(rt) => CheckResult {
            name: "java",
            ok: true,
            warn: false,
            detail: format!(
                "Java {} (major {}) at {}",
                rt.version,
                rt.major_version,
                rt.path.display()
            ),
        },
        Err(e) => CheckResult {
            name: "java",
            ok: false,
            warn: false,
            detail: format!("No usable Java found: {}", e),
        },
    }
}

fn check_data_dirs() -> CheckResult {
    let dirs = [
        crate::util::paths::get_data_dir(),
        crate::util::paths::get_cache_dir(),
        crate::util::paths::get_instances_dir(),
        crate::util::paths::get_versions_cache_dir(),
        crate::util::paths::get_libraries_cache_dir(),
        crate::util::paths::get_assets_cache_dir(),
        crate::util::paths::get_java_cache_dir(),
    ];
    let mut missing: Vec<PathBuf> = Vec::new();
    let mut broken: Option<String> = None;
    for d in &dirs {
        match d {
            Ok(p) => {
                if !p.exists() {
                    missing.push(p.clone());
                }
            }
            Err(e) => {
                broken = Some(e.to_string());
                break;
            }
        }
    }
    if let Some(err) = broken {
        return CheckResult {
            name: "directories",
            ok: false,
            warn: false,
            detail: format!("Cannot resolve MDL directories: {}", err),
        };
    }
    if missing.is_empty() {
        CheckResult {
            name: "directories",
            ok: true,
            warn: false,
            detail: format!(
                "All MDL directories present (root: {})",
                crate::util::paths::get_data_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            ),
        }
    } else {
        // Missing dirs are created on demand, so this is a warning not a failure.
        CheckResult {
            name: "directories",
            ok: true,
            warn: true,
            detail: format!(
                "{} directory(ies) not yet created (auto-created on first use): {}",
                missing.len(),
                missing
                    .iter()
                    .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

fn check_cache() -> CheckResult {
    match crate::util::cache::DownloadCache::new() {
        Ok(c) => CheckResult {
            name: "cache",
            ok: true,
            warn: false,
            detail: format!(
                "{} entries, {:.2} MB",
                c.entry_count(),
                c.total_size() as f64 / 1024.0 / 1024.0
            ),
        },
        Err(e) => CheckResult {
            name: "cache",
            ok: false,
            warn: false,
            detail: format!("Cannot open download cache: {}", e),
        },
    }
}

async fn check_mirrors() -> CheckResult {
    let probes = crate::util::mirrors::probe_all().await;
    let ok: Vec<_> = probes.iter().filter(|p| p.ok).collect();
    if ok.is_empty() {
        CheckResult {
            name: "mirrors",
            ok: true,
            warn: true,
            detail: "No mirror responded; downloads fall back to the official Mojang CDN"
                .to_string(),
        }
    } else {
        let best = &ok[0];
        CheckResult {
            name: "mirrors",
            ok: true,
            warn: false,
            detail: format!(
                "{}/{} reachable, fastest: {} ({} ms)",
                ok.len(),
                probes.len(),
                best.name,
                best.latency_ms
            ),
        }
    }
}

async fn check_instances() -> CheckResult {
    match crate::instance::InstanceManager::new() {
        Ok(m) => match m.list().await {
            Ok(list) => CheckResult {
                name: "instances",
                ok: true,
                warn: false,
                detail: format!("{} instance(s) on disk", list.len()),
            },
            Err(e) => CheckResult {
                name: "instances",
                ok: false,
                warn: false,
                detail: format!("Failed to list instances: {}", e),
            },
        },
        Err(e) => CheckResult {
            name: "instances",
            ok: false,
            warn: false,
            detail: format!("Cannot open instances dir: {}", e),
        },
    }
}

// GitHub@NDBlockConnect | BlockConnect@StarsailsClover

/// v26.5-alpha.3: verify instance-level JDK bindings (`mdl jdk use`) resolve
/// to an installed AprismJDK runtime. Unresolvable bindings are WARN (not
/// FAIL): launch degrades to the standard Adoptium chain by design, but the
/// operator should know the preference is currently inert.
async fn check_jdk_bindings() -> CheckResult {
    let manager = match crate::instance::InstanceManager::new() {
        Ok(m) => m,
        Err(_) => {
            return CheckResult {
                name: "jdk-bindings",
                ok: true,
                warn: false,
                detail: "skipped (instances dir unavailable)".into(),
            };
        }
    };
    let list = match manager.list().await {
        Ok(l) => l,
        Err(_) => {
            return CheckResult {
                name: "jdk-bindings",
                ok: true,
                warn: false,
                detail: "skipped (instances not listable)".into(),
            };
        }
    };

    let mut bound: Vec<(String, String)> = Vec::new();
    for inst in &list {
        if let Some(b) = &inst.config.jdk {
            bound.push((inst.name.clone(), b.clone()));
        }
    }
    if bound.is_empty() {
        return CheckResult {
            name: "jdk-bindings",
            ok: true,
            warn: false,
            detail: "no instance-level JDK bindings".into(),
        };
    }

    let mut broken: Vec<String> = Vec::new();
    for (name, b) in &bound {
        let hint = b.strip_prefix("aprism").map(|r| r.trim_start_matches('@'));
        if crate::loader::aprism_jdk::resolve(hint).is_err() {
            broken.push(format!("{} ({})", name, b));
        }
    }
    if broken.is_empty() {
        CheckResult {
            name: "jdk-bindings",
            ok: true,
            warn: false,
            detail: format!(
                "{} binding(s), all resolve (e.g. {})",
                bound.len(),
                bound[0].1
            ),
        }
    } else {
        CheckResult {
            name: "jdk-bindings",
            ok: true,
            warn: true,
            detail: format!(
                "{} binding(s), {} unresolvable (falling back to Adoptium): {}",
                bound.len(),
                broken.len(),
                broken.join(", ")
            ),
        }
    }
}

/// Probe an HTTP endpoint and report latency; used for the external services.
async fn probe_http(url: &str) -> (bool, u64) {
    let client = match crate::util::http::create_http_client() {
        Ok(c) => c,
        Err(_) => return (false, 0),
    };
    let start = Instant::now();
    let ok = match tokio::time::timeout(
        std::time::Duration::from_secs(8),
        client.get(url).send(),
    )
    .await
    {
        Ok(Ok(resp)) => resp.status().is_success(),
        _ => false,
    };
    (ok, start.elapsed().as_millis() as u64)
}

async fn check_mojang() -> CheckResult {
    let (ok, ms) = probe_http("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json").await;
    CheckResult {
        name: "mojang",
        ok,
        warn: false,
        detail: if ok {
            format!("version manifest reachable ({} ms)", ms)
        } else {
            "version manifest unreachable (game/library downloads will fail)".to_string()
        },
    }
}

async fn check_modrinth() -> CheckResult {
    let (ok, ms) = probe_http("https://api.modrinth.com/v2/search?limit=1").await;
    CheckResult {
        name: "modrinth",
        ok,
        warn: !ok, // mod search is optional; warn rather than fail
        detail: if ok {
            format!("search API reachable ({} ms)", ms)
        } else {
            "search API unreachable (mod/resourcepack search unavailable)".to_string()
        },
    }
}

async fn check_github() -> CheckResult {
    let (ok, ms) = probe_http("https://api.github.com/repos/NDBlockConnect/Despotes").await;
    CheckResult {
        name: "github",
        ok,
        warn: !ok, // Despotes/Aprism installs need this, but the launcher core does not
        detail: if ok {
            format!("GitHub API reachable ({} ms)", ms)
        } else {
            "GitHub API unreachable (Despotes/Aprism artifact installs unavailable)".to_string()
        },
    }
}
