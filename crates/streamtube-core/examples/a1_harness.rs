//! Track A1/A5 harness (plan2.md): proves YouTube extraction works without
//! yt-dlp through the exact chain `resolve_anonymous` runs on Android
//! (ANDROID_VR → visionOS → iOS), and that a known-good public HLS/MP4 stream
//! still downloads when YouTube refuses every client (DR-2). Every printed line
//! is a real HTTP result; nothing is simulated.
//!
//! host:
//!   cargo run -p streamtube-core --example a1_harness -- [video-id]
//! emulator (x86_64, API 26+):
//!   cargo ndk -t x86_64-linux-android -P 26 build -p streamtube-core --release --example a1_harness
//!   adb push target/x86_64-linux-android/release/examples/a1_harness /data/local/tmp/
//!   adb shell 'TMPDIR=/data/local/tmp /data/local/tmp/a1_harness [video-id]'
//!
//! exit 0: a playable YouTube stream proven without yt-dlp.
//! exit 3: YouTube refused (DR-2 recorded) and the public fallback streamed.
//! exit 1: neither could be proven.

use std::io::Read;
use streamtube_core::api::http;
use streamtube_core::api::streams::{self, FALLBACK_HLS, FALLBACK_MP4};
use streamtube_core::api::types::StreamInfo;
use streamtube_core::store;

/// One real HTTP GET (optional Range), printing status, bytes and URL.
/// Same 30s budget as `http::agent`. Retries transport errors twice because
/// this network's ureq path to some CDNs stalls intermittently (curl to the
/// same URLs succeeds — see docs/evidence/A1/network-troubleshooting.md).
fn get(url: &str, range: Option<&str>) -> Option<(u16, Vec<u8>)> {
    // googlevideo/youtube don't care, but media.w3.org's edge answers 403 to
    // browser UAs: everything that is not YouTube gets a plain client UA.
    let ua = if url.contains("googlevideo.com") || url.contains("youtube.com") {
        http::BROWSER_UA
    } else {
        "streamtube-a1-harness/0.1"
    };
    for attempt in 1..=3u8 {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build()
            .new_agent();
        let mut req = agent.get(url).header("User-Agent", ua);
        if let Some(r) = range {
            req = req.header("Range", r);
        }
        match req.call() {
            Ok(r) => {
                let status = r.status().as_u16();
                let mut body = Vec::new();
                let _ = r
                    .into_body()
                    .with_config()
                    .limit(64 * 1024 * 1024)
                    .reader()
                    .read_to_end(&mut body);
                if attempt > 1 {
                    println!("[http] attempt {attempt} succeeded: {url}");
                }
                return Some((status, body));
            }
            Err(e) => {
                println!("[http] {url} -> transport error (attempt {attempt}/3): {e:#}");
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        }
    }
    None
}

/// Resolves a playlist-relative media URL.
fn joined(base: &str, rel: &str) -> Option<String> {
    url::Url::parse(base)
        .ok()?
        .join(rel)
        .ok()
        .map(|u| u.to_string())
}

/// The exact client identity behind a resolved `source` tag.
fn client_label(source: &str) -> &'static str {
    match source {
        "innertube" => "ANDROID_VR 1.65.10 (Oculus Quest 3)",
        "visionos" => "VISIONOS 1.02 (Apple RealityDevice17,1)",
        "ios" => "IOS 20.10.4 (Apple iPhone16,2)",
        _ => "unknown",
    }
}

/// master playlist → first variant playlist → first segment: the same three
/// requests an HLS player makes. Returns bytes of the first segment fetched.
fn hls_probe(master: &str) -> Option<usize> {
    let (st, body) = get(master, None)?;
    let text = String::from_utf8_lossy(&body).into_owned();
    let variants = text
        .lines()
        .filter(|l| l.starts_with("#EXT-X-STREAM-INF"))
        .count();
    let bytes = text.len();
    println!("[hls] master -> HTTP {st}, {bytes} bytes, {variants} variants\n[hls]   {master}");
    if !(st == 200 || st == 206) || variants == 0 {
        return None;
    }
    let mut variant = None;
    let mut lines = text.lines();
    while let Some(l) = lines.next() {
        if l.starts_with("#EXT-X-STREAM-INF") {
            if let Some(u) = lines.next() {
                variant = Some(u.trim().to_string());
            }
            break;
        }
    }
    let v = variant?;
    let vurl = joined(master, &v).unwrap_or(v.clone());
    let (st2, body2) = get(&vurl, None)?;
    println!(
        "[hls] variant -> HTTP {st2}, {} bytes\n[hls]   {vurl}",
        body2.len()
    );
    if !(st2 == 200 || st2 == 206) {
        return None;
    }
    let text2 = String::from_utf8_lossy(&body2).into_owned();
    let seg = text2
        .lines()
        .find(|l| !l.trim().is_empty() && !l.starts_with('#'))?;
    let surl = joined(&vurl, seg.trim()).unwrap_or_else(|| seg.trim().to_string());
    let (st3, body3) = get(&surl, None)?;
    println!("[hls] segment -> HTTP {st3}, {} bytes", body3.len());
    if st3 == 200 || st3 == 206 {
        Some(body3.len())
    } else {
        None
    }
}

/// Proves the resolved formats actually serve bytes (Range GETs), not just
/// that the player call listed them.
fn prove_streams(info: &StreamInfo) -> bool {
    println!(
        "[a1] resolved: client={} source={} title={:?} live={} formats: video={} audio={} muxed={} hls_url={}",
        client_label(info.source),
        info.source,
        info.title,
        info.is_live,
        info.video.len(),
        info.audio.len(),
        info.muxed.len(),
        info.hls_url.is_some()
    );
    if info.hls_only {
        return match info.hls_url.as_deref().and_then(hls_probe) {
            Some(bytes) => {
                println!("[a1] HLS path serves real bytes ({bytes})");
                true
            }
            None => {
                println!("[a1] HLS path refused or empty");
                false
            }
        };
    }
    let (video, audio) = streams::pick_formats(info, 1080, "vp9", None);
    let mut proven = false;
    if let Some(v) = video {
        println!(
            "[a1] best video: {}x{} {}fps codecs={} itag={} label={}",
            v.width, v.height, v.fps, v.codecs, v.itag, v.quality_label
        );
        if let Some((st, b)) = get(&v.url, Some("bytes=0-1048575")) {
            println!(
                "[a1] video Range GET bytes=0-1048575 -> HTTP {st}, {} bytes\n[a1]   {}",
                b.len(),
                v.url
            );
            proven = (st == 206 || st == 200) && b.len() >= 32 * 1024;
        }
    }
    if let Some(a) = audio {
        println!(
            "[a1] best audio: codecs={} itag={} label={:?}",
            a.codecs, a.itag, a.quality_label
        );
        if let Some((st, b)) = get(&a.url, Some("bytes=0-262143")) {
            println!(
                "[a1] audio Range GET bytes=0-262143 -> HTTP {st}, {} bytes",
                b.len()
            );
        }
    }
    proven
}

/// Track A5: a general HLS/MP4 line with streams known to be valid, so the
/// playback path is proven even under a YouTube DR-2 block.
fn a5_fallback() -> bool {
    println!("[a5] general fallback (known-valid public streams)");
    let hls = hls_probe(FALLBACK_HLS).is_some_and(|b| b > 100_000);
    let mp4 = match get(FALLBACK_MP4, Some("bytes=0-1048575")) {
        Some((st, b)) => {
            println!(
                "[a5] mp4 Range GET bytes=0-1048575 -> HTTP {st}, {} bytes\n[a5]   {FALLBACK_MP4}",
                b.len()
            );
            (st == 206 || st == 200) && b.len() >= 32 * 1024
        }
        None => false,
    };
    hls && mp4
}

fn main() {
    let vid = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "dQw4w9WgXcQ".into());
    let dir = std::env::temp_dir().join(format!("a1-harness-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    store::set_data_dir(dir.clone());
    println!("[a1] harness start: video={vid} data_dir={}", dir.display());
    println!("[a1] chain: ANDROID_VR -> visionOS -> iOS (no yt-dlp involved)");

    let extraction = match streams::android_vr_first(&vid) {
        Ok(info) => {
            let ok = prove_streams(&info);
            if !ok {
                println!(
                    "[a1] verdict=FORMATS_UNUSABLE: player call answered but no real bytes served — DR-2"
                );
            }
            Some(ok)
        }
        Err(e) => {
            let text = format!("{e:#}");
            println!("[a1] chain refused:");
            for line in text.lines() {
                println!("[a1]   {line}");
            }
            println!(
                "[a1] verdict=EXTRACTION_BLOCKED bot_or_login={}",
                streams::verification_requested(&text)
            );
            Some(false)
        }
    };

    let fallback = a5_fallback();
    let (verdict, code) = match extraction {
        Some(true) => ("A1_OK: playable YouTube stream proven without yt-dlp", 0),
        Some(false) if fallback => (
            "DR2_BLOCKED: YouTube refused; general fallback HLS/MP4 proven",
            3,
        ),
        Some(false) => (
            "DR2_BLOCKED_AND_NO_FALLBACK: neither YouTube nor the public streams served",
            1,
        ),
        None => ("NO_PROOF", 1),
    };
    println!("[a1] FINAL VERDICT: {verdict} (fallback_ok={fallback})");
    std::process::exit(code);
}
