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

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
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

// GitHub@NDBlockConnect | BlockConnect@StarsailsClover

/// Macro state (v26.5-alpha.6). Shape pinned from Despotes
/// MacroRecorder.statusJson(): macroCount, recording, playing (+playingName
/// /playingStep/playingTotalSteps while playing), macros[] with name +
/// stepCount.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MacroSnapshot {
    pub macros: HashMap<String, u64>,
    pub playing: Option<(String, u64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleEvent {
    Registered { name: String, period_ticks: u64 },
    Removed { name: String },
    Fired { name: String, execution_count: u64, next_run_in: u64 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum MacroEvent {
    Recorded { name: String, step_count: u64 },
    Removed { name: String },
    PlaybackStarted { name: String, total_steps: u64 },
    PlaybackFinished { name: String },
}

// GitHub@NDBlockConnect | BlockConnect@StarsailsClover

/// A registered circuit watch subscription (v26.5-alpha.8): a cube the
/// watcher repeatedly scans and diffs on behalf of an agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircuitWatch {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub radius: u8,
}

/// One circuit component, keyed by its position.
#[derive(Debug, Clone, PartialEq)]
pub struct CircuitComponent {
    pub block: String,
    pub powered: Option<bool>,
    pub delay: Option<i64>,
    pub note: Option<i64>,
    pub facing: Option<String>,
    pub locked: Option<bool>,
}

pub type CircuitSnapshot = HashMap<(i32, i32, i32), CircuitComponent>;

/// Parse a Despotes circuit-scan `result` (WorldProbes.circuit, v26.11).
/// Shape: inWorld, cx/cy/cz/radius, scanned, count, components[] with
/// block/x/y/z (+powered/delay/note/facing/locked when the blockstate has
/// them). Returns None while the game is not in a world (inWorld=false) so
/// the watcher keeps its previous snapshot instead of emitting mass
/// removals for a menu screen.
pub fn parse_circuit_snapshot(result: &serde_json::Value) -> Option<CircuitSnapshot> {
    if result.get("inWorld").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    let arr = result.get("components")?.as_array()?;
    let mut out = CircuitSnapshot::new();
    for c in arr {
        let (Some(x), Some(y), Some(z)) = (
            c.get("x").and_then(|v| v.as_i64()).map(|v| v as i32),
            c.get("y").and_then(|v| v.as_i64()).map(|v| v as i32),
            c.get("z").and_then(|v| v.as_i64()).map(|v| v as i32),
        ) else {
            continue;
        };
        let Some(block) = c.get("block").and_then(|v| v.as_str()) else { continue };
        let opt_bool = |k: &str| c.get(k).and_then(|v| v.as_bool());
        let opt_i64 = |k: &str| {
            c.get(k)
                .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        };
        out.insert(
            (x, y, z),
            CircuitComponent {
                block: block.to_string(),
                powered: opt_bool("powered"),
                delay: opt_i64("delay"),
                note: opt_i64("note"),
                facing: c.get("facing").and_then(|v| v.as_str()).map(String::from),
                locked: opt_bool("locked"),
            },
        );
    }
    Some(out)
}

// GitHub@NDBlockConnect | BlockConnect@StarsailsClover

/// Diff two circuit scans into compact per-component change entries:
/// appeared / removed / changed (any of powered, delay, note, facing,
/// locked, or the block id itself). Pure function; unit-tested.
pub fn diff_circuits(prev: &CircuitSnapshot, current: &CircuitSnapshot) -> Vec<serde_json::Value> {
    let mut changes = Vec::new();
    for (pos, cur) in current {
        let (x, y, z) = pos;
        match prev.get(pos) {
            None => {
                let mut e = json!({
                    "event": "appeared",
                    "block": cur.block,
                    "x": x, "y": y, "z": z,
                });
                if let Some(p) = cur.powered { e["powered"] = json!(p); }
                changes.push(e);
            }
            Some(old) if old != cur => {
                let mut e = json!({
                    "event": "changed",
                    "block": cur.block,
                    "x": x, "y": y, "z": z,
                });
                if let Some(p) = cur.powered { e["powered"] = json!(p); }
                if old.block != cur.block { e["wasBlock"] = json!(old.block); }
                if old.powered != cur.powered {
                    e["wasPowered"] = old.powered.map(|p| json!(p)).unwrap_or(json!(null));
                }
                changes.push(e);
            }
            _ => {}
        }
    }
    for (pos, old) in prev {
        if !current.contains_key(pos) {
            let (x, y, z) = pos;
            changes.push(json!({
                "event": "removed",
                "block": old.block,
                "x": x, "y": y, "z": z,
            }));
        }
    }
    changes
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

/// Parse a Despotes macro-status `result` envelope (MacroRecorder.statusJson).
/// Returns None when the payload lacks the expected fields (older Despotes).
pub fn parse_macro_snapshot(result: &serde_json::Value) -> Option<MacroSnapshot> {
    // GitHub@NDBlockConnect | BlockConnect@StarsailsClover
    // Shape gate: "recording"/"playing" booleans are the contract markers.
    result.get("playing")?.as_bool()?;
    result.get("recording").and_then(|v| v.as_bool())?;

    let mut macros = HashMap::new();
    if let Some(arr) = result.get("macros").and_then(|v| v.as_array()) {
        for m in arr {
            let Some(name) = m.get("name").and_then(|v| v.as_str()) else { continue };
            let steps = m
                .get("stepCount")
                .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)))
                .unwrap_or(0);
            macros.insert(name.to_string(), steps);
        }
    }
    let playing = if result.get("playing").and_then(|v| v.as_bool()) == Some(true) {
        let name = result.get("playingName").and_then(|v| v.as_str())?.to_string();
        let total = result
            .get("playingTotalSteps")
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)))
            .unwrap_or(0);
        Some((name, total))
    } else {
        None
    };
    Some(MacroSnapshot { macros, playing })
}

/// Diff macro snapshots into lifecycle events. Order: recordings, playback
/// start, playback finish, removals.
pub fn diff_macros(prev: &MacroSnapshot, current: &MacroSnapshot) -> Vec<MacroEvent> {
    let mut events = Vec::new();
    for (name, steps) in &current.macros {
        if !prev.macros.contains_key(name) {
            events.push(MacroEvent::Recorded { name: name.clone(), step_count: *steps });
        }
    }
    // GitHub@NDBlockConnect | BlockConnect@StarsailsClover
    match (&prev.playing, &current.playing) {
        (Some(old), Some((new, total))) if old.0 != *new => {
            events.push(MacroEvent::PlaybackFinished { name: old.0.clone() });
            events.push(MacroEvent::PlaybackStarted { name: new.clone(), total_steps: *total });
        }
        (None, Some((new, total))) => {
            events.push(MacroEvent::PlaybackStarted { name: new.clone(), total_steps: *total });
        }
        (Some(old), None) => {
            events.push(MacroEvent::PlaybackFinished { name: old.0.clone() });
        }
        _ => {}
    }
    for name in prev.macros.keys() {
        if !current.macros.contains_key(name) {
            events.push(MacroEvent::Removed { name: name.clone() });
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
    let mut macro_snapshots: HashMap<String, MacroSnapshot> = HashMap::new();
    let mut circuit_snapshots: HashMap<(String, String), CircuitSnapshot> = HashMap::new();

    loop {
        ticker.tick().await;
        // GitHub@NDBlockConnect | BlockConnect@StarsailsClover
        let instances: Vec<(String, Vec<CircuitWatch>)> = {
            let st = state.read().await;
            st.running_instances
                .keys()
                .map(|instance| (
                    instance.clone(),
                    st.watches.get(instance).cloned().unwrap_or_default(),
                ))
                .collect()
        };

        for (instance, watches) in instances {
            let dir = match super::server::resolve_instance_dir(&instance).await {
                Ok(d) => d,
                Err(_) => {
                    snapshots.remove(&instance);
                    macro_snapshots.remove(&instance);
                    circuit_snapshots.retain(|(i, _), _| i != &instance);
                    continue;
                }
            };
            // Unreachable game: forget its schedules so the next session
            // re-registers them.
            if !crate::game::client::is_available(&dir).await {
                snapshots.remove(&instance);
                macro_snapshots.remove(&instance);
                circuit_snapshots.retain(|(i, _), _| i != &instance);
                continue;
            }
            let timestamp = chrono::Utc::now().to_rfc3339();

            // ---- schedules (v26.9+) ----
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
                        timestamp: timestamp.clone(),
                    },
                };
                let _ = event_tx.send(event);
            }
            *prev = current;

            // ---- macros (v26.5-alpha.6) ----
            // GitHub@NDBlockConnect | BlockConnect@StarsailsClover
            let mstatus = match crate::game::client::macro_status(&dir).await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let mcurrent = match parse_macro_snapshot(&mstatus) {
                Some(c) => c,
                None => continue,
            };
            let mprev = macro_snapshots.entry(instance.clone()).or_default();
            for ev in diff_macros(mprev, &mcurrent) {
                let event = match ev {
                    MacroEvent::Recorded { name, step_count } => ServerEvent::MacroRecorded {
                        instance: instance.clone(),
                        name,
                        step_count,
                        timestamp: timestamp.clone(),
                    },
                    MacroEvent::PlaybackStarted { name, total_steps } => ServerEvent::MacroPlaybackStarted {
                        instance: instance.clone(),
                        name,
                        total_steps,
                        timestamp: timestamp.clone(),
                    },
                    MacroEvent::PlaybackFinished { name } => ServerEvent::MacroPlaybackFinished {
                        instance: instance.clone(),
                        name,
                        timestamp: timestamp.clone(),
                    },
                    MacroEvent::Removed { name } => ServerEvent::MacroRemoved {
                        instance: instance.clone(),
                        name,
                        timestamp: timestamp.clone(),
                    },
                };
                let _ = event_tx.send(event);
            }
            *mprev = mcurrent;

            // ---- circuit watches (v26.5-alpha.8) ----
            // Remove snapshots for watches deleted through the API. Without
            // this, deleting then re-adding the same name could inherit an
            // old scan and suppress its first appeared/change event.
            let active_watches: HashSet<&str> = watches.iter().map(|w| w.name.as_str()).collect();
            circuit_snapshots.retain(|(watch_instance, watch_name), _| {
                watch_instance != &instance || active_watches.contains(watch_name.as_str())
            });
            for watch in watches {
                let cstatus = match crate::game::client::circuit_query(
                    &dir,
                    Some(watch.x),
                    Some(watch.y),
                    Some(watch.z),
                    Some(watch.radius),
                )
                .await
                {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                // Menu / not-in-world responses keep the prior scan so the
                // next world entry does not emit a mass removal event.
                let Some(current) = parse_circuit_snapshot(&cstatus) else { continue };
                let key = (instance.clone(), watch.name.clone());
                let prev = circuit_snapshots.entry(key).or_default();
                let mut changes = diff_circuits(prev, &current);
                if !changes.is_empty() {
                    let truncated = changes.len() > 64;
                    changes.truncate(64);
                    if truncated {
                        changes.push(json!({"event": "truncated", "remaining": true}));
                    }
                    let _ = event_tx.send(ServerEvent::CircuitChanged {
                        instance: instance.clone(),
                        watch: watch.name,
                        changes,
                        timestamp: timestamp.clone(),
                    });
                }
                *prev = current;
            }
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

    // GitHub@NDBlockConnect | BlockConnect@StarsailsClover

    /// Shape pinned to Despotes MacroRecorder.statusJson() (v26.9+).
    #[test]
    fn test_parse_macro_snapshot_official_shape() {
        let idle = json!({
            "macroCount": 1,
            "recording": false,
            "playing": false,
            "macros": [{"name": "demo", "stepCount": 5}]
        });
        let s = parse_macro_snapshot(&idle).unwrap();
        assert_eq!(s.macros.get("demo"), Some(&5u64));
        assert_eq!(s.playing, None);

        let playing = json!({
            "macroCount": 1,
            "recording": false,
            "playing": true,
            "playingName": "demo",
            "playingStep": 2,
            "playingTotalSteps": 5,
            "macros": [{"name": "demo", "stepCount": 5}]
        });
        let s = parse_macro_snapshot(&playing).unwrap();
        assert_eq!(s.playing, Some(("demo".into(), 5u64)));

        // Older Despotes / junk payloads -> None.
        assert!(parse_macro_snapshot(&json!({})).is_none());
        assert!(parse_macro_snapshot(&json!({"playing": "yes"})).is_none());
        assert!(parse_macro_snapshot(&json!({"playing": true})).is_none(), "missing playingName");
    }

    #[test]
    fn test_diff_macros_lifecycle() {
        let empty = MacroSnapshot::default();

        let mut recorded = MacroSnapshot::default();
        recorded.macros.insert("demo".into(), 5);
        let evs = diff_macros(&empty, &recorded);
        assert_eq!(evs, vec![MacroEvent::Recorded { name: "demo".into(), step_count: 5 }]);

        // Playback start.
        let mut playing = recorded.clone();
        playing.playing = Some(("demo".into(), 5));
        let evs = diff_macros(&recorded, &playing);
        assert_eq!(evs, vec![MacroEvent::PlaybackStarted { name: "demo".into(), total_steps: 5 }]);

        // Playback finish.
        let evs = diff_macros(&playing, &recorded);
        assert_eq!(evs, vec![MacroEvent::PlaybackFinished { name: "demo".into() }]);

        // Macro swap while playing: finish + start, order preserved.
        let mut swapped = recorded.clone();
        swapped.playing = Some(("other".into(), 3));
        let evs = diff_macros(&playing, &swapped);
        assert_eq!(evs, vec![
            MacroEvent::PlaybackFinished { name: "demo".into() },
            MacroEvent::PlaybackStarted { name: "other".into(), total_steps: 3 },
        ]);

        // Removal.
        let evs = diff_macros(&recorded, &empty);
        assert_eq!(evs, vec![MacroEvent::Removed { name: "demo".into() }]);

        // No-op.
        assert!(diff_macros(&recorded, &recorded).is_empty());
    }

    // GitHub@NDBlockConnect | BlockConnect@StarsailsClover

    fn component(block: &str, powered: Option<bool>) -> CircuitComponent {
        CircuitComponent {
            block: block.into(),
            powered,
            delay: None,
            note: None,
            facing: None,
            locked: None,
        }
    }

    /// Shape pinned to Despotes WorldProbes.circuit() (v26.11).
    #[test]
    fn test_parse_circuit_snapshot_official_shape() {
        let result = json!({
            "inWorld": true,
            "cx": -516, "cy": 71, "cz": -87, "radius": 3,
            "scanned": 343, "count": 2,
            "components": [
                {"block":"minecraft:lever","x":-516,"y":71,"z":-87,
                 "powered":true,"facing":"north"},
                {"block":"minecraft:note_block","x":-515,"y":71,"z":-87,
                 "note":3,"powered":false}
            ]
        });
        let snap = parse_circuit_snapshot(&result).unwrap();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[&(-516, 71, -87)].powered, Some(true));
        assert_eq!(snap[&(-515, 71, -87)].note, Some(3));

        // Menu screen: keep prior snapshot, never emit mass removals.
        assert!(parse_circuit_snapshot(&json!({"inWorld": false})).is_none());
        // Missing components is unsupported/junk, skip safely.
        assert!(parse_circuit_snapshot(&json!({"inWorld": true})).is_none());
    }

    #[test]
    fn test_diff_circuits_lifecycle_and_state() {
        let empty = CircuitSnapshot::new();
        let mut lever_off = CircuitSnapshot::new();
        lever_off.insert((1, 2, 3), component("minecraft:lever", Some(false)));

        // Appearance.
        let evs = diff_circuits(&empty, &lever_off);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0]["event"], "appeared");
        assert_eq!(evs[0]["powered"], false);

        // Powered flip carries current + prior state.
        let mut lever_on = CircuitSnapshot::new();
        lever_on.insert((1, 2, 3), component("minecraft:lever", Some(true)));
        let evs = diff_circuits(&lever_off, &lever_on);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0]["event"], "changed");
        assert_eq!(evs[0]["powered"], true);
        assert_eq!(evs[0]["wasPowered"], false);

        // Removal and no-op.
        let evs = diff_circuits(&lever_on, &empty);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0]["event"], "removed");
        assert!(diff_circuits(&lever_on, &lever_on).is_empty());
    }
}
