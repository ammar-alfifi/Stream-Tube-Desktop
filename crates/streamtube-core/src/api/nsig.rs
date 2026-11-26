//! YouTube nsig/signature solver using the yt-dlp EJS scripts.
//!
//! The scripts run in a fresh QuickJS runtime with no host functions, a memory
//! limit, and a deadline. Only JSON crosses the boundary. Release 0.8.0 is
//! bundled; a verified newer release can be installed into the app data dir.
//! Installed hashes detect corruption, not a malicious process with the same
//! OS identity; the data dir is app-private and downloaded code has no host API.

use anyhow::{Result, anyhow};
use rquickjs::{Context, Runtime};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, Instant};

const EJS_LIB: &str = include_str!("../../../../third_party/ejs/yt.solver.lib.min.js");
const EJS_CORE: &str = include_str!("../../../../third_party/ejs/yt.solver.core.min.js");
const MEMORY_LIMIT: usize = 256 * 1024 * 1024;
const STACK_LIMIT: usize = 8 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(30);

fn scripts() -> Result<(String, String)> {
    if let Some(pair) = installed_scripts(&crate::store::data_dir().join("ejs")) {
        return Ok(pair);
    }
    Ok((EJS_LIB.to_string(), EJS_CORE.to_string()))
}

fn installed_scripts(base: &Path) -> Option<(String, String)> {
    let current = std::fs::read_to_string(base.join("CURRENT")).ok()?;
    let generation = current.trim();
    if !valid_generation(generation) {
        return None;
    }
    let dir = base.join(generation);
    Some((
        verified_script(&dir, "yt.solver.lib.min.js")?,
        verified_script(&dir, "yt.solver.core.min.js")?,
    ))
}

fn valid_generation(generation: &str) -> bool {
    !generation.is_empty()
        && generation.len() <= 128
        && generation != "."
        && generation != ".."
        && generation
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

fn valid_sha256(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn verified_script(dir: &Path, name: &str) -> Option<String> {
    let sums = std::fs::read_to_string(dir.join("SHA256SUMS")).ok();
    let expected = sums.as_deref().and_then(|text| {
        text.lines().find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let file = parts.next()?;
            (file.trim_start_matches('*') == name).then(|| hash.to_string())
        })
    });
    let Some(expected) = expected else {
        return None;
    };
    if !valid_sha256(&expected) {
        return None;
    }
    let Ok(bytes) = std::fs::read(dir.join(name)) else {
        return None;
    };
    if sha256(&bytes) != expected {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Downloads the latest yt-dlp/ejs release, verifies GitHub's SHA-256 digests,
/// and commits a new generation through the `CURRENT` pointer. A failed or
/// concurrent install never exposes a mixed script pair; bundled scripts remain
/// the fallback until a complete generation is selected.
pub fn update_from_github() -> Result<String> {
    let release =
        crate::api::http::get_json("https://api.github.com/repos/yt-dlp/ejs/releases/latest")?;
    let version = release
        .get("tag_name")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("EJS release has no tag_name"))?
        .to_string();
    let assets = release
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("EJS release has no assets"))?;
    let mut files = Vec::new();
    for name in ["yt.solver.lib.min.js", "yt.solver.core.min.js"] {
        let asset = assets
            .iter()
            .find(|a| a.get("name").and_then(Value::as_str) == Some(name))
            .ok_or_else(|| anyhow!("EJS release is missing {name}"))?;
        let url = asset
            .get("browser_download_url")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("EJS asset {name} has no URL"))?;
        validate_asset_url(url)?;
        let digest = asset
            .get("digest")
            .and_then(Value::as_str)
            .and_then(|d| d.strip_prefix("sha256:"))
            .ok_or_else(|| anyhow!("EJS asset {name} has no SHA-256 digest"))?;
        if !valid_sha256(digest) {
            return Err(anyhow!("EJS asset {name} has a malformed SHA-256 digest"));
        }
        let bytes = crate::api::http::get_bytes(url, &[])?;
        if sha256(&bytes) != digest {
            return Err(anyhow!("EJS asset {name} failed SHA-256 verification"));
        }
        files.push((name, bytes, digest.to_string()));
    }
    install_generation(&crate::store::data_dir().join("ejs"), &version, &files)?;
    Ok(version)
}

fn validate_asset_url(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url)?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("github.com")
        || parsed.port_or_known_default() != Some(443)
        || !parsed.username().is_empty()
        || !parsed.password().unwrap_or("").is_empty()
        || !parsed.path().starts_with("/yt-dlp/ejs/releases/download/")
    {
        return Err(anyhow!(
            "EJS asset URL is outside the expected GitHub release path"
        ));
    }
    Ok(())
}

fn install_generation(base: &Path, version: &str, files: &[(&str, Vec<u8>, String)]) -> Result<()> {
    std::fs::create_dir_all(base)?;
    let safe_version: String = version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let generation = format!("{safe_version}-{}-{nonce}", std::process::id());
    anyhow::ensure!(
        valid_generation(&generation),
        "EJS release tag produces an invalid generation name"
    );
    let dir = base.join(&generation);
    std::fs::create_dir(&dir)?;
    for (name, bytes, _digest) in files {
        std::fs::write(dir.join(name), bytes)?;
    }
    let sums = files
        .iter()
        .map(|(name, _, digest)| format!("{digest}  {name}\n"))
        .collect::<String>();
    std::fs::write(dir.join("SHA256SUMS"), sums)?;
    let current_tmp = base.join(format!("CURRENT.{generation}.tmp"));
    std::fs::write(&current_tmp, format!("{generation}\n"))?;
    std::fs::rename(current_tmp, base.join("CURRENT"))?;
    Ok(())
}

/// Solves one batch of `n`/`sig` challenges with the verified EJS scripts.
pub fn solve(input: Value) -> Result<Value> {
    let started = Instant::now();
    let runtime = Runtime::new()?;
    runtime.set_memory_limit(MEMORY_LIMIT);
    runtime.set_max_stack_size(STACK_LIMIT);
    runtime.set_interrupt_handler(Some(Box::new(move || started.elapsed() > DEADLINE)));
    let context = Context::full(&runtime)?;
    let (ejs_lib, ejs_core) = scripts()?;
    context.with(|ctx| -> Result<Value> {
        let _: () = ctx.eval(ejs_lib.as_str()).map_err(|e| anyhow!("load EJS library: {e}"))?;
        let _: () = ctx
            .eval("var meriyah = lib.meriyah; var astring = lib.astring;")
            .map_err(|e| anyhow!("bind EJS globals: {e}"))?;
        let _: () = ctx.eval(ejs_core.as_str()).map_err(|e| anyhow!("load EJS core: {e}"))?;
        let input = input.to_string();
        let solve: rquickjs::Function = ctx.eval(
            "(s) => { try { return JSON.stringify(jsc(JSON.parse(s))); } catch (e) { return JSON.stringify({error: String(e), stack: e.stack}); } }",
        )?;
        let output: String = solve.call((input,))?;
        let value: Value = serde_json::from_str(&output)?;
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Err(anyhow::anyhow!("{error}"));
        }
        Ok(value)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        install_generation, installed_scripts, sha256, solve, valid_generation, validate_asset_url,
        verified_script,
    };
    use serde_json::json;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "streamtube-ejs-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn solves_preprocessed_challenges_inside_quickjs() {
        let out = solve(json!({
            "type": "preprocessed",
            "preprocessed_player": "_result.n = x => x + '_n'; _result.sig = x => x + '_s';",
            "requests": [
                {"type": "n", "challenges": ["abc"]},
                {"type": "sig", "challenges": ["def"]},
            ],
        }))
        .unwrap();
        assert_eq!(out["responses"][0]["data"]["abc"], "abc_n");
        assert_eq!(out["responses"][1]["data"]["def"], "def_s");
    }

    #[test]
    fn malformed_player_is_an_error_not_a_panic() {
        assert!(
            solve(json!({
                "type": "player",
                "player": "var x = 1;",
                "requests": [{"type": "n", "challenges": ["abc"]}],
            }))
            .is_err()
        );
    }

    #[test]
    fn installed_scripts_require_the_recorded_hash() {
        let dir = temp_dir("hash");
        std::fs::create_dir_all(&dir).unwrap();
        let name = "yt.solver.core.min.js";
        std::fs::write(dir.join(name), b"trusted").unwrap();
        assert!(verified_script(&dir, name).is_none());
        std::fs::write(
            dir.join("SHA256SUMS"),
            format!("{}  {name}\n", sha256(b"trusted")),
        )
        .unwrap();
        assert_eq!(verified_script(&dir, name).as_deref(), Some("trusted"));
        std::fs::write(dir.join(name), b"changed").unwrap();
        assert!(verified_script(&dir, name).is_none());
        std::fs::write(dir.join(name), b"trusted").unwrap();
        std::fs::write(dir.join("SHA256SUMS"), format!("not-a-hash  {name}\n")).unwrap();
        assert!(verified_script(&dir, name).is_none());
        std::fs::write(
            dir.join("SHA256SUMS"),
            format!("{}  {name}\n", sha256(b"trusted")),
        )
        .unwrap();
        std::fs::write(
            dir.join("SHA256SUMS"),
            format!("{}  {name}\n", sha256(&[0xff, 0xfe])),
        )
        .unwrap();
        std::fs::write(dir.join(name), [0xff, 0xfe]).unwrap();
        assert!(verified_script(&dir, name).is_none());
        std::fs::remove_file(dir.join(name)).unwrap();
        assert!(verified_script(&dir, name).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn generation_commit_selects_a_complete_pair_and_rejects_traversal() {
        let base = temp_dir("generation");
        let v1 = [
            (
                "yt.solver.lib.min.js",
                b"lib-v1".to_vec(),
                sha256(b"lib-v1"),
            ),
            (
                "yt.solver.core.min.js",
                b"core-v1".to_vec(),
                sha256(b"core-v1"),
            ),
        ];
        install_generation(&base, "0.8.0", &v1).unwrap();
        let first_current = std::fs::read_to_string(base.join("CURRENT")).unwrap();
        let first_dir = base.join(first_current.trim());
        assert_eq!(installed_scripts(&base).unwrap().0, "lib-v1");

        let v2 = [
            (
                "yt.solver.lib.min.js",
                b"lib-v2".to_vec(),
                sha256(b"lib-v2"),
            ),
            (
                "yt.solver.core.min.js",
                b"core-v2".to_vec(),
                sha256(b"core-v2"),
            ),
        ];
        install_generation(&base, "0.8.1", &v2).unwrap();
        let second_current = std::fs::read_to_string(base.join("CURRENT")).unwrap();
        assert_eq!(installed_scripts(&base).unwrap().0, "lib-v2");
        assert!(
            first_dir.exists(),
            "the previous generation must remain intact"
        );
        std::fs::write(first_dir.join("yt.solver.lib.min.js"), b"corrupt").unwrap();
        assert_eq!(installed_scripts(&base).unwrap().0, "lib-v2");

        let long_version = "v".repeat(200);
        assert!(install_generation(&base, &long_version, &v2).is_err());
        assert_eq!(
            std::fs::read_to_string(base.join("CURRENT")).unwrap(),
            second_current
        );
        std::fs::write(base.join("CURRENT"), "../escape\n").unwrap();
        assert!(installed_scripts(&base).is_none());
        assert!(!valid_generation(".."));
        assert!(!valid_generation("a/b"));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn release_assets_must_use_the_expected_https_github_path() {
        assert!(
            validate_asset_url(
                "https://github.com/yt-dlp/ejs/releases/download/0.8.0/yt.solver.core.min.js"
            )
            .is_ok()
        );
        for bad in [
            "http://github.com/yt-dlp/ejs/releases/download/0.8.0/yt.solver.core.min.js",
            "https://example.com/yt-dlp/ejs/releases/download/0.8.0/yt.solver.core.min.js",
            "https://github.com/attacker/ejs/releases/download/0.8.0/yt.solver.core.min.js",
            "https://github.com/yt-dlp/ejs/releases/download/../../evil.js",
            "https://github.com:444/yt-dlp/ejs/releases/download/0.8.0/yt.solver.core.min.js",
            "https://user@github.com/yt-dlp/ejs/releases/download/0.8.0/yt.solver.core.min.js",
        ] {
            assert!(validate_asset_url(bad).is_err(), "accepted {bad}");
        }
    }
}
