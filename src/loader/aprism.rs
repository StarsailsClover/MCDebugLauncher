// Aprism JE Native loader support (Alpha 7).
//
// Aprism (https://github.com/NDBlockConnect/Aprism) is a JE/BE native mod
// loader. On JE it attaches as a javaagent:
//   -javaagent:<jar>=aprismVersion=<v>;mcEdit=JE;mcVersion=<mc>;gameRoot=<dir>
//
// MDL detects the applicable Aprism artifact from GitHub Releases (same
// policy as Despotes: stable preferred, pre-release opt-in fallback),
// downloads + caches it, and appends the `-javaagent` flag at launch.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const APRISM_OWNER: &str = "NDBlockConnect";
pub const APRISM_REPO: &str = "Aprism";
pub const APRISM_JAR_PREFIX: &str = "Aprism-";

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    prerelease: bool,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
    digest: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AprismAsset {
    pub name: String,
    pub url: String,
    pub digest: Option<String>,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct AprismRelease {
    pub tag: String,
    pub prerelease: bool,
    pub assets: Vec<AprismAsset>,
}

/// Fetch Aprism releases (newest first).
pub async fn fetch_releases() -> Result<Vec<AprismRelease>> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases?per_page=30",
        APRISM_OWNER, APRISM_REPO
    );
    let response = crate::util::http::create_http_client()?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Failed to query Aprism releases")?;
    if !response.status().is_success() {
        anyhow::bail!("GitHub returned HTTP {} for Aprism releases", response.status());
    }
    let raw: Vec<GhRelease> = response.json().await.context("Failed to parse Aprism releases")?;
    Ok(raw
        .into_iter()
        .map(|r| AprismRelease {
            tag: r.tag_name,
            prerelease: r.prerelease,
            assets: r
                .assets
                .into_iter()
                .map(|a| AprismAsset {
                    name: a.name,
                    url: a.browser_download_url,
                    digest: a.digest,
                    size: a.size.unwrap_or(0),
                })
                .collect(),
        })
        .collect())
}

/// Asset names look like `Aprism-v26.0-Alpha.8-JE-26.2.jar`.
pub fn parse_asset_name(name: &str) -> Option<(String, String, String)> {
    let stem = name.strip_suffix(".jar")?;
    let rest = stem.strip_prefix(APRISM_JAR_PREFIX)?;
    // <tag>-JE-<mcver>  (tag may contain dashes)
    let (tag_and_edit, mc) = rest.rsplit_once('-')?;
    let (tag, edit) = tag_and_edit.rsplit_once('-')?;
    Some((tag.to_string(), edit.to_string(), mc.to_string()))
}

/// Pick the applicable JE artifact for a Minecraft version, honouring the
/// stable-first / pre-release-fallback policy.
pub fn select_release(
    releases: &[AprismRelease],
    mc_version: &str,
    allow_prerelease: bool,
) -> Option<(AprismRelease, AprismAsset)> {
    let applicable = |a: &AprismAsset| {
        parse_asset_name(&a.name)
            .map(|(tag, edit, mc)| edit == "JE" && mc == mc_version && !tag.is_empty())
            .unwrap_or(false)
    };
    for r in releases.iter().filter(|r| !r.prerelease) {
        if let Some(a) = r.assets.iter().find(|a| applicable(a)) {
            return Some((r.clone(), a.clone()));
        }
    }
    if allow_prerelease {
        for r in releases.iter().filter(|r| r.prerelease) {
            if let Some(a) = r.assets.iter().find(|a| applicable(a)) {
                return Some((r.clone(), a.clone()));
            }
        }
    }
    None
}

fn cache_dir() -> Result<PathBuf> {
    let dir = crate::util::paths::get_data_dir()?.join("aprism");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Download and cache an Aprism artifact.
///
/// v26.6-alpha.3 (downstream report, AprismRefract FACT.md: "the filename
/// lies"): the cache used to be a bare name-exists check, so a same-named
/// stale/truncated/overwritten jar was reused silently forever - and the
/// agent cache selector picks by filename, so launches attached the wrong
/// bytes. Cached files are now validated against the release metadata
/// (size, plus GitHub's sha256 digest when present) and re-downloaded on
/// mismatch.
pub async fn download_asset(asset: &AprismAsset) -> Result<PathBuf> {
    let dir = cache_dir()?;
    let dest = dir.join(&asset.name);
    if dest.exists() && cached_asset_is_valid(&dest, asset) {
        return Ok(dest);
    }
    if dest.exists() {
        tracing::warn!(
            "Cached Aprism artifact {} failed validation (size/digest); re-downloading",
            asset.name
        );
    }
    crate::version::downloader::download_file(&asset.url, &dest, None).await?;
    Ok(dest)
}

// GitHub@NDBlockConnect | BlockConnect@StarsailsClover

/// Validate a cached artifact against release metadata. A size of 0 in the
/// metadata means "unknown" (older API payloads), in which case only the
/// digest is checked when present, else the file is accepted.
fn cached_asset_is_valid(path: &Path, asset: &AprismAsset) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if asset.size > 0 && meta.len() != asset.size {
        return false;
    }
    if let Some(expected) = &asset.digest {
        let Some(expected) = expected.strip_prefix("sha256:") else {
            // Unknown digest algorithm: size check (above) is all we have.
            return true;
        };
        let Ok(bytes) = std::fs::read(path) else {
            return false;
        };
        use sha2::Digest as _;
        let actual = format!("{:x}", sha2::Sha256::digest(&bytes));
        return actual.eq_ignore_ascii_case(expected);
    }
    true
}

/// Build the `-javaagent` argument for launching with Aprism.
pub fn javaagent_arg(jar: &Path, aprism_version: &str, mc_version: &str, game_root: &Path) -> String {
    format!(
        "-javaagent:{}=aprismVersion={};mcEdit=JE;mcVersion={};gameRoot={}",
        jar.display(),
        aprism_version,
        mc_version,
        game_root.display()
    )
}

// GitHub@NDBlockConnect | BlockConnect@StarsailsClover

/// True when the JAR carries unrelocated ObjectWeb ASM classes
/// (v26.6-alpha.3).
///
/// Field evidence (2026-09-26 crash log, reproduced 2026-09-14 by MDL):
/// the Aprism JE agent jar embeds ASM 9.9. Fabric Loader >= 0.19.3 scans the
/// classpath with `ClassLoader.getResources(ClassReader.class)` and
/// **hard-fails when more than one ASM source is visible** - agent jars
/// attached via `-javaagent` are visible to that lookup, so
/// `--aprism` on a modern Fabric host dies in `Knot.<clinit>` with
/// "duplicate ASM classes found on classpath". MDL cannot dedupe an agent
/// jar's embedded classes without rewriting a signed artifact
/// (Aprism ships `.jar.sig`), so the launch path detects the conflict
/// up-front and fails with actionable guidance instead of an obscure
/// Knot stack trace.
pub fn jar_embeds_asm(jar: &Path) -> bool {
    let Ok(file) = std::fs::File::open(jar) else {
        return false;
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return false;
    };
    // Scope the borrow so the ZipFile temporary is dropped before `zip`.
    let found = match zip.by_name("org/objectweb/asm/ClassReader.class") {
        Ok(_entry) => true,
        Err(_) => false,
    };
    found
}

/// True when the Fabric Loader version rejects duplicate ASM sources
/// (>= 0.19.3, per LoaderUtil.verifyClasspath). Unparseable versions
/// (e.g. the literal "latest") are treated as strict: 0.19.3+ is what
/// current 26.x hosts resolve to, and the failure mode is fatal.
pub fn fabric_rejects_duplicate_asm(loader_version: &str) -> bool {
    let mut parts = loader_version
        .trim()
        .split(['.', '-', '+'])
        .filter_map(|p| p.parse::<u64>().ok());
    let (major, minor, patch) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    if major == 0 && minor == 0 && patch == 0 {
        // Unparseable ("latest", a hash, ...): assume strict.
        return true;
    }
    (major, minor, patch) >= (0, 19, 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    // GitHub@NDBlockConnect | BlockConnect@StarsailsClover

    /// v26.6-alpha.3 (downstream §1.1): the Fabric duplicate-ASM gate.
    #[test]
    fn test_fabric_rejects_duplicate_asm_versions() {
        // Strict lines reject (the reproduced crash was 0.19.3).
        assert!(fabric_rejects_duplicate_asm("0.19.3"));
        assert!(fabric_rejects_duplicate_asm("0.19.4"));
        assert!(fabric_rejects_duplicate_asm("0.20.0"));
        assert!(fabric_rejects_duplicate_asm("1.0.0"));
        // Unparseable values are treated as strict (current hosts).
        assert!(fabric_rejects_duplicate_asm("latest"));
        assert!(fabric_rejects_duplicate_asm(""));
        // Older loaders tolerate duplicates.
        assert!(!fabric_rejects_duplicate_asm("0.19.2"));
        assert!(!fabric_rejects_duplicate_asm("0.18.4"));
        assert!(!fabric_rejects_duplicate_asm("0.16.14"));
    }

    /// The agent jar ASM probe: a JAR carrying ClassReader.class is flagged,
    /// one without it is not.
    #[test]
    fn test_jar_embeds_asm_detection() {
        use std::io::Write as _;
        let dir = tempfile::TempDir::new().unwrap();

        let with_asm = dir.path().join("with-asm.jar");
        {
            let f = std::fs::File::create(&with_asm).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("org/objectweb/asm/ClassReader.class", opts).unwrap();
            z.write_all(b"fake").unwrap();
            z.finish().unwrap();
        }
        assert!(jar_embeds_asm(&with_asm));

        let without = dir.path().join("plain.jar");
        {
            let f = std::fs::File::create(&without).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("net/example/Mod.class", opts).unwrap();
            z.write_all(b"fake").unwrap();
            z.finish().unwrap();
        }
        assert!(!jar_embeds_asm(&without));

        // Missing/corrupt files never panic.
        assert!(!jar_embeds_asm(&dir.path().join("nope.jar")));
        let junk = dir.path().join("junk.jar");
        std::fs::write(&junk, b"not a zip").unwrap();
        assert!(!jar_embeds_asm(&junk));
    }
    // Network-dependent integration test: the --aprism launch path must
    // resolve and download the JE javaagent artifact from GitHub Releases.
    // Disabled by default; run with --ignored.
    #[tokio::test]
    #[ignore]
    async fn test_real_aprism_javaagent_download() {
        let releases = fetch_releases().await.expect("fetch aprism releases");
        assert!(!releases.is_empty());
        // The probe target MC version is discovered from the real asset list
        // so the test never hardcodes a brittle version.
        let mc = releases
            .iter()
            .flat_map(|r| r.assets.iter())
            .filter_map(|a| parse_asset_name(&a.name))
            .map(|(_, _, mc)| mc)
            .next()
            .expect("an aprism JE asset exists");
        let (rel, asset) = select_release(&releases, &mc, true)
            .expect("aprism JE artifact selectable");
        let (tag, edit, amc) = parse_asset_name(&asset.name).unwrap();
        assert_eq!(tag, rel.tag);
        assert_eq!(edit, "JE");
        assert_eq!(amc, mc);
        let cached = download_asset(&asset).await.expect("download aprism jar");
        assert!(cached.exists());
        // javaagent arg must embed the jar path and all four key=value fields.
        let arg = javaagent_arg(&cached, &rel.tag, &mc, std::path::Path::new("/g"));
        assert!(arg.starts_with("-javaagent:"));
        assert!(arg.contains("aprismVersion="));
        assert!(arg.contains("mcEdit=JE"));
        assert!(arg.contains("mcVersion="));
        assert!(arg.contains("gameRoot="));
    }


    use super::*;

    #[test]
    fn test_parse_asset_name() {
        let (tag, edit, mc) = parse_asset_name("Aprism-v26.0-Alpha.8-JE-26.2.jar").unwrap();
        assert_eq!(tag, "v26.0-Alpha.8");
        assert_eq!(edit, "JE");
        assert_eq!(mc, "26.2");
        assert!(parse_asset_name("Despotes-v26.0-Alpha.2-fabric-1.21.1.jar").is_none());
    }

    #[test]
    fn test_javaagent_arg() {
        let arg = javaagent_arg(
            Path::new("/x/aprism.jar"),
            "v26.0-Alpha.8",
            "26.2",
            Path::new("/game"),
        );
        assert!(arg.starts_with("-javaagent:/x/aprism.jar=aprismVersion=v26.0-Alpha.8;mcEdit=JE;mcVersion=26.2;gameRoot=/game"));
    }
}
