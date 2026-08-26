#![no_main]

// Fuzz the server.properties editor: parse arbitrary line soup into a
// PropertiesFile, then drive the mutation API (set/remove/get/get_bool) with
// keys derived from the input itself. The editor preserves comments and
// order and must never panic - it edits live server configs.

use libfuzzer_sys::fuzz_target;
use mcdebug_launcher::loader::props::PropertiesFile;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let lines: Vec<String> = text.lines().map(str::to_string).collect();

    let mut props = PropertiesFile::from_lines(lines);

    let _ = props.pairs();

    // Derive probe keys from input content so they collide with real keys.
    let probe_key: String = text.chars().take(24).collect();
    props.set(&probe_key, "true");
    let _ = props.get_bool(&probe_key);
    let _ = props.get(&probe_key);

    // Idempotence: setting twice must not duplicate lines beyond one entry.
    let before = props.pairs().len();
    props.set(&probe_key, "false");
    let after = props.pairs().len();
    assert!(after <= before + 1, "duplicate key injection: {before} -> {after}");

    props.remove(&probe_key);
    assert!(props.get(&probe_key).is_none(), "remove failed to drop key");
});
