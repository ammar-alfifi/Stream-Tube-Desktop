//! Live proof of the web watch-page fallback (plan2.md Track A2):
//! watch page → player JS → EJS sig/n deciphering → signed URL → HTTP 206.
//!
//! Exits non-zero when any step fails, so it can gate CI-style verification.
//!
//! ```bash
//! cargo run -p streamtube-core --example web_stream_probe --locked -- dQw4w9WgXcQ
//! ```
use streamtube_core::api::web_stream::resolve_reported;

fn main() -> anyhow::Result<()> {
    let id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "dQw4w9WgXcQ".into());
    println!("video: {id}");

    let (info, log) = match resolve_reported(&id) {
        Ok(ok) => ok,
        Err(e) => {
            // a refusal (bot check, network block) is a documented outcome,
            // never a fabricated success
            println!("RESULT: FAIL\nresolve error: {e:#}");
            std::process::exit(1);
        }
    };
    println!("--- web fallback steps ---\n{log}");

    println!(
        "title: {} | live: {} | hls: {} | video {} audio {} muxed {}",
        info.title,
        info.is_live,
        info.hls_url.is_some(),
        info.video.len(),
        info.audio.len(),
        info.muxed.len()
    );
    for f in info.muxed.iter().chain(info.video.iter()).take(6) {
        println!(
            "  itag {} {} {}x{} {} (url {} bytes)",
            f.itag,
            f.mime,
            f.width,
            f.height,
            f.quality_label,
            f.url.len()
        );
    }

    // the proof: a deciphered URL must serve a byte range
    let pick = info
        .muxed
        .first()
        .or_else(|| info.video.first())
        .or_else(|| info.audio.first())
        .expect("a deciphered format");
    println!("--- range check (bytes=0-1023) ---");
    println!(
        "itag {} url head: {}",
        pick.itag,
        &pick.url[..pick.url.len().min(120)]
    );
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent();
    let resp = agent
        .get(&pick.url)
        .header("User-Agent", streamtube_core::api::http::BROWSER_UA)
        .header("Range", "bytes=0-1023")
        .call()?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok().map(str::to_string));
    let content_range = resp
        .headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok().map(str::to_string));
    let bytes = resp.into_body().read_to_vec()?;
    println!("HTTP {status}");
    println!("Content-Type: {}", content_type.as_deref().unwrap_or("?"));
    println!("Content-Range: {}", content_range.as_deref().unwrap_or("?"));
    println!("Bytes read: {}", bytes.len());

    if status == 206 && bytes.len() == 1024 {
        println!("RESULT: PASS (deciphered web URL serves a real byte range)");
        Ok(())
    } else {
        println!("RESULT: FAIL (expected HTTP 206 with 1024 bytes)");
        std::process::exit(1);
    }
}
