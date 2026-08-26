// Minimal library surface for out-of-crate consumers (cargo-fuzz targets).
//
// mdl is primarily a binary crate; main.rs owns the full module tree.
// This lib re-exposes ONLY the self-contained parser modules - files with
// zero `crate::` dependencies - under their original paths so they compile
// unchanged. Fuzz targets link against this rlib instead of vendoring or
// refactoring the parsers.
//
// Scope discipline: do NOT add app modules here. Anything that pulls in the
// launcher runtime (tokio state, config globals) defeats the point of a
// minimal fuzz surface and slows every build.

pub mod diagnostic {
    pub mod log_parser;
}

pub mod loader {
    pub mod props;
}

pub mod util {
    pub mod jsonio;
}
