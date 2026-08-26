#![no_main]

// Fuzz the diagnostic log parser end-to-end. Arbitrary UTF-8 (lossy) text
// goes through every public entry point: whole-log parsing + analysis,
// per-line parsing, stack-trace extraction, crash-type detection and mod
// extraction. The parser must never panic on hostile input - it runs on
// crash dumps collected in the field.

use libfuzzer_sys::fuzz_target;
use mcdebug_launcher::diagnostic::log_parser::{self, LogParser};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);

    let parser = LogParser::new();
    let entries = parser.parse_content(&text);
    let _ = parser.analyze(&entries);

    for (i, line) in text.lines().enumerate() {
        let _ = log_parser::parse_line(line, i as u64);
        let _ = log_parser::is_continuation(line);
    }

    let _ = log_parser::extract_stack_trace(&text, 64);
    let _ = log_parser::detect_crash_type(&text);
    let _ = log_parser::extract_mods(&text, 32);
});
