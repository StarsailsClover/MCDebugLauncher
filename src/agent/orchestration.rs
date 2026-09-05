// Orchestration event watcher (v26.5-alpha.5).
//
// Bridges Despotes schedule state into the MDL WebSocket event stream:
// a background loop polls every running game's schedule status (Despotes
// v26.9+ `{"type":"schedule","op":"status"}`), diffs it against the last
// snapshot, and broadcasts schedule_registered / schedule_fired /
// schedule_removed events so agents can REACT to orchestration instead of
// polling it.
//
// Response shape (authoritative, Despotes ScheduleManager.statusJson()):
//   result.count: int
//   result.schedules: [{ name, id, periodTicks, commandCount,
//                        executionCount, nextRunIn }]
//
// Design notes:
//   - Only instances tracked by THIS agent server (running_instances) are
//     probed, so the cost is bounded by live games, not workspace size.
//   - Transient status failures keep the previous snapshot (no spurious
//     removed-events); the snapshot is dropped only when the game is no
//     longer reachable, so a relaunch re-registers schedules naturally.
//   - Diffing is a pure function (unit-tested); the loop only does IO.

// GitHub@NDBlockConnect | BlockConnect@StarsailsClover

use std::collections::HashMap;
use std::time::Duration;

use super::server::ServerEvent;

/// Poll cadence. 5s keeps `schedule_fired` latency well under one period of
/// any realistic schedule (minimum sensible periodTicks is ~20 = 1s) while
/// staying negligible next to game traffic.
const POLL_INTERVAL_SECS: u64 = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleSnapshot {
    pub execution_count: u64,
    pub period_ticks: u64,
    pub next_run_in: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleEvent {
    Registered { name: String, period_ticks: u64 },
    Removed { name: String },
    Fired { name: String, execution_count: u64, next_run_in: u64 },
}

/// Parse a Despotes schedule-status `result` envelope into a snapshot map.
/// Returns None when the payload does not carry a `schedules` array (older
/// Despotes lines) so the watcher can skip the instance cleanly.
pub fn parse_schedule_snapshot(result: &serde_json::Value) -> Option<HashMap<String, ScheduleSnapshot>> {
    let arr = result.get("schedules")?.as_array()?;
    let mut out = HashMap::new();
    for s in arr {
        let Some(name) = s.get("name").and_then(|v| v.as_str()) else { continue };
        let num = |field: &str| -> u64 {
            s.get(field)
                .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)))
                .unwrap_or(0)
        };
        out.insert(
            name.to_string(),
            ScheduleSnapshot {
                execution_count: num("executionCount"),
                period_ticks: num("periodTicks"),
                next_run_in: num("nextRunIn"),
            },
        );
    }
    Some(out)
}

/// Diff two snapshots into lifecycle/firing events. Order: registrations,
/// firings, removals - agents see setup, then activity, then teardown.
pub fn diff_schedules(
    prev: &HashMap<String, ScheduleSnapshot>,
    current: &HashMap<String, ScheduleSnapshot>,
) -> Vec<ScheduleEvent> {
    let mut events = Vec::new();
    // GitHub@NDBlockConnect | BlockConnect@StarsailsClover
    for (name, cur) in current {
        match prev.get(name) {
            None => events.push(ScheduleEvent::Registered {
                name: name.clone(),
                period_ticks: cur.period_ticks,
            }),
            Some(old) if cur.execution_count > old.execution_count => {
                events.push(ScheduleEvent::Fired {
                    name: name.clone(),
                    execution_count: cur.execution_count,
                    next_run_in: cur.next_run_in,
                });
            }
            _ => {}
        }
    }
    for name in prev.keys() {
        if !current.contains_key(name) {
            events.push(ScheduleEvent::Removed { name: name.clone() });
        }
    }
    events
}

/// Background loop: poll tracked instances, diff, broadcast. Never returns
/// (spawned once per agent server); all failures are swallowed - the watcher
/// must never take the server down.
pub(super) async fn watch_loop(
    state: std::sync::Arc<tokio::sync::RwLock<super::server::ServerState>>,
    event_tx: tokio::sync::broadcast::Sender<ServerEvent>,
) {
    let mut ticker = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Snapshots keyed by instance name.
    let mut snapshots: HashMap<String, HashMap<String, ScheduleSnapshot>> = HashMap::new();

    loop {
        ticker.tick().await;
        // GitHub@NDBlockConnect | BlockConnect@StarsailsClover
        let instances: Vec<String> = {
            let st = state.read().await;
            st.running_instances.keys().cloned().collect()
        };

        for instance in instances {
            let dir = match super::server::resolve_instance_dir(&instance).await {
                Ok(d) => d,
                Err(_) => {
                    snapshots.remove(&instance);
                    continue;
                }
            };
            // Unreachable game: forget its schedules so the next session
            // re-registers them.
            if !crate::game::client::is_available(&dir).await {
                snapshots.remove(&instance);
                continue;
            }
            let status = match crate::game::client::schedule_status(&dir).await {
                Ok(v) => v,
                // Transient failure (menu screen, tick hitch): keep the old
                // snapshot so we do not emit phantom removals.
                Err(_) => continue,
            };
            let current = match parse_schedule_snapshot(&status) {
                Some(c) => c,
                // Old Despotes without schedule support: skip silently.
                None => continue,
            };

            let prev = snapshots.entry(instance.clone()).or_default();
            for ev in diff_schedules(prev, &current) {
                let timestamp = chrono::Utc::now().to_rfc3339();
                let event = match ev {
                    ScheduleEvent::Registered { name, period_ticks } => ServerEvent::ScheduleRegistered {
                        instance: instance.clone(),
                        name,
                        period_ticks,
                        timestamp: timestamp.clone(),
                    },
                    ScheduleEvent::Fired { name, execution_count, next_run_in } => ServerEvent::ScheduleFired {
                        instance: instance.clone(),
                        name,
                        execution_count,
                        next_run_in,
                        timestamp: timestamp.clone(),
                    },
                    ScheduleEvent::Removed { name } => ServerEvent::ScheduleRemoved {
                        instance: instance.clone(),
                        name,
                        timestamp,
                    },
                };
                let _ = event_tx.send(event);
            }
            *prev = current;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // GitHub@NDBlockConnect | BlockConnect@StarsailsClover

    fn snap(count: u64, period: u64, next: u64) -> ScheduleSnapshot {
        ScheduleSnapshot { execution_count: count, period_ticks: period, next_run_in: next }
    }

    /// Shape pinned to Despotes ScheduleManager.statusJson() (v26.9+).
    #[test]
    fn test_parse_schedule_snapshot_official_shape() {
        let result = json!({
            "count": 2,
            "schedules": [
                {"name": "heartbeat", "id": 1, "periodTicks": 100,
                 "commandCount": 1, "executionCount": 4, "nextRunIn": 37},
                {"name": "lights", "id": 2, "periodTicks": 40,
                 "commandCount": 3, "executionCount": 0, "nextRunIn": 40}
            ]
        });
        let m = parse_schedule_snapshot(&result).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m["heartbeat"], snap(4, 100, 37));
        assert_eq!(m["lights"], snap(0, 40, 40));

        // Older Despotes (no schedules field) -> None, watcher skips.
        assert!(parse_schedule_snapshot(&json!({})).is_none());
        assert!(parse_schedule_snapshot(&json!({"schedules": "junk"})).is_none());
        // Entries without a name are skipped, not fatal.
        let partial = parse_schedule_snapshot(&json!({"schedules": [{"id": 9}, {"name": "x", "executionCount": 1}]})).unwrap();
        assert_eq!(partial.len(), 1);
    }

    #[test]
    fn test_diff_schedules_lifecycle() {
        let empty = HashMap::new();
        let mut one = HashMap::new();
        one.insert("hb".to_string(), snap(0, 100, 100));

        // Registration on first sight.
        let evs = diff_schedules(&empty, &one);
        assert_eq!(evs, vec![ScheduleEvent::Registered { name: "hb".into(), period_ticks: 100 }]);

        // Firing: count increases.
        let mut fired = HashMap::new();
        fired.insert("hb".to_string(), snap(3, 100, 42));
        let evs = diff_schedules(&one, &fired);
        assert_eq!(evs, vec![ScheduleEvent::Fired { name: "hb".into(), execution_count: 3, next_run_in: 42 }]);

        // Same count -> no event.
        assert!(diff_schedules(&fired, &fired).is_empty());

        // Removal.
        let evs = diff_schedules(&fired, &empty);
        assert_eq!(evs, vec![ScheduleEvent::Removed { name: "hb".into() }]);
    }
}
