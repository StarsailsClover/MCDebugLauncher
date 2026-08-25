// Native library loading via JavaAgent (v26.4-alpha.2).
//
// BUG FIX (field report): `mdl inject --dll` used CreateRemoteThread to
// load DLLs into JVM processes. On JDK 25 with CFG/CET mitigations the
// remote thread crashes the target before DllMain executes — MaxHook's
// init code never ran.
//
// Fix strategy (vendor-expected channel): package a micro JavaAgent whose
// premain/agentmain calls System.load(dllPath), then attach it through the
// standard JVM Attach API. The class is embedded (compiled with
// javac -source 11 -target 11 from the adjacent .java) and packaged into
// a minimal JAR at runtime — the JVM requires a JAR with Premain-Class /
// Agent-Class manifest attributes for loadAgent.
//
// Non-JVM targets (e.g. bedrock_server.exe) keep the legacy remote-thread
// path in util::injector; callers decide via process-name detection.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Embedded NativeLoaderAgent.class bytes (CAFEBABE-prefixed).
const AGENT_CLASS: &[u8] = include_bytes!("native_loader/NativeLoaderAgent.class");

/// Manifest required for the JVM to recognize premain/agentmain.
const AGENT_MANIFEST: &str = "Manifest-Version: 1.0\r\n\
                              Premain-Class: NativeLoaderAgent\r\n\
                              Agent-Class: NativeLoaderAgent\r\n\
                              \r\n";

/// Ensure `<data>/native-loader/NativeLoaderAgent.jar` exists and is
/// up-to-date, returning its path. Idempotent: rewritten every call so
/// mdl upgrades always carry their current agent.
pub fn ensure_agent_jar(base_dir: &Path) -> Result<PathBuf> {
    let dir = base_dir.join("native-loader");
    std::fs::create_dir_all(&dir)?;
    let jar_path = dir.join("NativeLoaderAgent.jar");

    let file = std::fs::File::create(&jar_path)
        .with_context(|| format!("Failed to create {}", jar_path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored); // tiny file; no gain from deflate

    zip.start_file("META-INF/MANIFEST.MF", opts)
        .context("Failed to write manifest entry")?;
    zip.write_all(AGENT_MANIFEST.as_bytes())?;

    zip.start_file("NativeLoaderAgent.class", opts)
        .context("Failed to write class entry")?;
    zip.write_all(AGENT_CLASS)?;

    zip.finish().context("Failed to finalize agent JAR")?;
    Ok(jar_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_class_embedded() {
        assert!(AGENT_CLASS.len() > 100);
        assert_eq!(&AGENT_CLASS[0..4], &[0xCA, 0xFE, 0xBA, 0xBE]);
    }

    #[test]
    fn test_ensure_agent_jar_produces_valid_zip() {
        let dir = std::env::temp_dir().join(format!("mdl_nl_test_{}", std::process::id()));
        let jar = ensure_agent_jar(&dir).unwrap();
        assert!(jar.exists());
        // Must be a readable zip with both entries.
        let f = std::fs::File::open(&jar).unwrap();
        let mut archive = zip::ZipArchive::new(f).unwrap();
        assert!(archive.by_name("META-INF/MANIFEST.MF").is_ok());
        assert!(archive.by_name("NativeLoaderAgent.class").is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_manifest_has_both_attributes() {
        assert!(AGENT_MANIFEST.contains("Premain-Class: NativeLoaderAgent"));
        assert!(AGENT_MANIFEST.contains("Agent-Class: NativeLoaderAgent"));
        assert!(AGENT_MANIFEST.ends_with("\r\n\r\n"));
    }
}
