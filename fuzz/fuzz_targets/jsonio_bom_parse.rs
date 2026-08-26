#![no_main]

// Fuzz the BOM-tolerant JSON config path: strip_bom + UTF-8 conversion +
// serde_json, mirroring jsonio::parse_sync's core (the file I/O itself is
// excluded - fuzzing real paths would just test the OS). Instance configs,
// account stores and metrics files all flow through this logic.

use libfuzzer_sys::fuzz_target;
use mcdebug_launcher::util::jsonio::strip_bom;

fuzz_target!(|data: &[u8]| {
    let cleaned = strip_bom(data);

    // parse_sync uses from_utf8 + from_str; mirror that (from_slice is
    // equivalent for valid UTF-8 but skips the conversion path).
    if let Ok(text) = String::from_utf8(cleaned.to_vec()) {
        let value: Result<serde_json::Value, _> = serde_json::from_str(&text);
        // Double-strip must be a no-op (idempotence of BOM handling).
        assert_eq!(
            strip_bom(cleaned),
            cleaned,
            "strip_bom is not idempotent"
        );
        let _ = value;
    }
});
