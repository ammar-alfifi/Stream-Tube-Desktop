//! Web watch-page stream fallback (plan2.md Track A2): the watch page's own
//! embedded player response, with every `signatureCipher`/`n` challenge
//! deciphered in-process by the bundled EJS scripts (`super::nsig`).
//!
//! Android reaches this after ANDROID_VR fails and before the Apple HLS
//! clients; desktop keeps its yt-dlp/visionOS resolve path untouched. The
//! anonymous WEB `/youtubei/v1/player` call is bot-gated ("The page needs to
//! be reloaded"), but the watch page HTML still embeds a player response
//! with playable formats, so the HTML is the source.
use super::http::{self, urldecode, urlencode};
use super::nsig;
use super::streams::{parse_player, playability_error};
use crate::api::types::StreamInfo;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Resolves playable streams from the watch page, deciphered with the EJS
/// solver. See `resolve_reported` for a version that reports every step.
pub fn resolve(video_id: &str) -> Result<StreamInfo> {
    resolve_reported(video_id).map(|(info, _)| info)
}

/// The same, plus a step-by-step report of what was fetched and deciphered —
/// the live harness prints it and stores it as evidence.
pub fn resolve_reported(video_id: &str) -> Result<(StreamInfo, String)> {
    let mut log = String::new();
    let t = Instant::now();
    let page = http::get_bytes(
        &format!(
            "https://www.youtube.com/watch?v={}&hl=en",
            urlencode(video_id)
        ),
        &[("Accept-Language", "en-US,en;q=0.9")],
    )?;
    log.push_str(&format!(
        "watch page: {} bytes in {:?}\n",
        page.len(),
        t.elapsed()
    ));
    let page = String::from_utf8_lossy(&page).into_owned();
    let mut player = extract_player_response(&page).ok_or_else(|| {
        anyhow!("watch page has no ytInitialPlayerResponse (bot check or layout change)")
    })?;
    if let Some(err) = playability_error(&player, Some("web")) {
        return Err(err);
    }
    log.push_str(&format!(
        "playability: {} ({:?} so far)\n",
        player
            .pointer("/playabilityStatus/status")
            .and_then(Value::as_str)
            .unwrap_or("?"),
        t.elapsed()
    ));
    let player_js = player_js(&page, &mut log)?;
    decipher_formats(&mut player, &player_js, &mut log)?;
    let mut info = parse_player(&player);
    info.source = "web";
    log.push_str(&format!(
        "deciphered formats: {} video, {} audio, {} muxed, hls={} in {:?} total\n",
        info.video.len(),
        info.audio.len(),
        info.muxed.len(),
        info.hls_url.is_some(),
        t.elapsed()
    ));
    if info.video.is_empty()
        && info.audio.is_empty()
        && info.muxed.is_empty()
        && info.hls_url.is_none()
    {
        return Err(anyhow!(
            "web: the watch page carries no decipherable formats"
        ));
    }
    if crate::perf::on() {
        eprintln!("[web_stream]\n{log}");
    }
    Ok((info, log))
}

/// `ytInitialPlayerResponse = {…};` — the player response embedded in the
/// watch page HTML. Brace- and string-aware scan from each marker; the first
/// candidate that parses as JSON with a playabilityStatus or streamingData wins.
fn extract_player_response(page: &str) -> Option<Value> {
    let bytes = page.as_bytes();
    let mut from = 0;
    while let Some(found) = page[from..].find("ytInitialPlayerResponse") {
        let marker = from + found;
        from = marker + "ytInitialPlayerResponse".len();
        let open = marker + bytes[marker..].iter().position(|&b| b == b'{')?;
        let Some(end) = json_object_end(bytes, open) else {
            continue;
        };
        if let Ok(v) = serde_json::from_str::<Value>(&page[open..=end]) {
            if v.get("playabilityStatus").is_some() || v.get("streamingData").is_some() {
                return Some(v);
            }
        }
    }
    None
}

/// Index of the `}` that closes the JSON object opened at `open`, honoring
/// string escapes; None when the page ends first.
fn json_object_end(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The watch page's bundled player JS (the challenge code lives there).
/// Cached: one script serves every video and changes at most every few days.
fn player_js(page: &str, log: &mut String) -> Result<String> {
    static CACHE: parking_lot::Mutex<Option<(String, String, Instant)>> =
        parking_lot::Mutex::new(None);
    const TTL: Duration = Duration::from_secs(12 * 3600);
    let url = player_js_url(page);
    if url.is_empty() {
        return Err(anyhow!("watch page names no player JS"));
    }
    if let Some((u, js, at)) = CACHE.lock().as_ref() {
        if *u == url && at.elapsed() < TTL {
            log.push_str(&format!("player JS (cached): {url}\n"));
            return Ok(js.clone());
        }
    }
    let t = Instant::now();
    let js = http::get_text(&url)?;
    if js.len() < 100_000 {
        return Err(anyhow!("player JS suspiciously small: {} bytes", js.len()));
    }
    log.push_str(&format!(
        "player JS: {url} ({} bytes, {:?})\n",
        js.len(),
        t.elapsed()
    ));
    *CACHE.lock() = Some((url, js.clone(), Instant::now()));
    Ok(js)
}

/// ytcfg's `PLAYER_JS_URL`, or the script the iframe API names.
fn player_js_url(page: &str) -> String {
    if let Some(u) = quoted_value(page, "PLAYER_JS_URL") {
        if u.starts_with("//") {
            return format!("https:{u}");
        }
        if u.starts_with('/') {
            return format!("https://www.youtube.com{u}");
        }
        if !u.is_empty() {
            return u;
        }
    }
    let api = http::get_text("https://www.youtube.com/iframe_api").unwrap_or_default();
    let id: String = api
        .split("/s/player/")
        .nth(1)
        .map(|r| r.chars().take_while(|c| c.is_ascii_hexdigit()).collect())
        .unwrap_or_default();
    if !id.is_empty() {
        return format!("https://www.youtube.com/s/player/{id}/player_ias.vflset/en_US/base.js");
    }
    String::new()
}

/// `"KEY":"VALUE"` straight out of the HTML (ytcfg style), one escape undone.
fn quoted_value(haystack: &str, key: &str) -> Option<String> {
    let rest = haystack.split(&format!("\"{key}\":\"")).nth(1)?;
    let v: String = rest.chars().take_while(|c| *c != '"').collect();
    Some(v.replace("\\/", "/"))
}

/// One format's URL before deciphering: either a plain `url` (its `n` still
/// needs solving) or a `signatureCipher` (`sig`, `sp` and the `url` it signs).
struct Pending {
    url: String,
    sig: Option<String>,
    sp: String,
}

/// "s=…&sp=…&url=…" with one level of percent-encoding on each value.
fn parse_cipher(cipher: &str) -> Option<(String, String, String)> {
    let mut s = None;
    let mut sp = "signature".to_string();
    let mut url = None;
    for pair in cipher.split('&') {
        let (k, v) = pair.split_once('=')?;
        let v = urldecode(v);
        match k {
            "s" => s = Some(v),
            "sp" => sp = v,
            "url" => url = Some(v),
            _ => {}
        }
    }
    Some((s?, sp, url?))
}

/// The raw `n` challenge in a URL — undecoded, exactly as yt-dlp reads it,
/// bounded by `?`/`&` on the left and `&` or the end on the right.
fn n_challenge(url: &str) -> Option<&str> {
    n_span(url).map(|(v, _)| v)
}

/// Where the `n` parameter's raw value sits: (value, offset of the `n=` start).
fn n_span(url: &str) -> Option<(&str, usize)> {
    let bytes = url.as_bytes();
    let mut from = 0;
    while let Some(found) = url[from..].find("n=") {
        let at = from + found;
        from = at + 2;
        let boundary = at == 0 || bytes[at - 1] == b'?' || bytes[at - 1] == b'&';
        if !boundary {
            continue;
        }
        let end = url[at + 2..]
            .find('&')
            .map(|e| at + 2 + e)
            .unwrap_or(url.len());
        if end > at + 2 {
            return Some((&url[at + 2..end], at));
        }
    }
    None
}

/// Replaces the `n` parameter's raw value, keeping every other byte as-is.
fn replace_n(url: &str, solved: &str) -> String {
    match n_span(url) {
        Some((_, at)) => {
            let value_end = url[at + 2..]
                .find('&')
                .map(|e| at + 2 + e)
                .unwrap_or(url.len());
            format!("{}{}{}", &url[..at + 2], solved, &url[value_end..])
        }
        None => url.to_string(),
    }
}

/// One batched answer from the EJS solver: challenge → solved.
fn solved_map(out: &Value, request: usize) -> Result<HashMap<String, String>> {
    let data = out
        .pointer(&format!("/responses/{request}/data"))
        .ok_or_else(|| anyhow!("EJS response {request} has no data: {out}"))?;
    let mut map = HashMap::new();
    for (k, v) in data.as_object().into_iter().flatten() {
        if let Some(s) = v.as_str() {
            map.insert(k.clone(), s.to_string());
        }
    }
    Ok(map)
}

/// Deciphers every format URL in the player response in place: collects all
/// challenges, solves them in one EJS call, then rewrites the URLs. Formats
/// with neither a URL nor a cipher (the watch page's PO-token-gated adaptive
/// formats) are left alone and skipped downstream; formats with unanswered
/// challenges are dropped — an unsolved URL 403s after the first megabyte.
fn decipher_formats(player: &mut Value, player_js: &str, log: &mut String) -> Result<()> {
    let pending = collect_formats(player);
    log.push_str(&format!(
        "formats to decipher: {} ({} with signatureCipher)\n",
        pending.len(),
        pending.iter().filter(|p| p.2.sig.is_some()).count()
    ));
    if pending.is_empty() {
        return Ok(());
    }
    let mut sigs: Vec<String> = Vec::new();
    let mut ns: Vec<String> = Vec::new();
    for (_, _, p) in &pending {
        if let Some(s) = &p.sig {
            if !sigs.contains(s) {
                sigs.push(s.clone());
            }
        }
        if let Some(n) = n_challenge(&p.url) {
            let n = n.to_string();
            if !ns.contains(&n) {
                ns.push(n);
            }
        }
    }
    log.push_str(&format!("challenges: {} sig, {} n\n", sigs.len(), ns.len()));
    let mut requests = Vec::new();
    if !sigs.is_empty() {
        requests.push(json!({ "type": "sig", "challenges": sigs }));
    }
    if !ns.is_empty() {
        requests.push(json!({ "type": "n", "challenges": ns }));
    }
    let t = Instant::now();
    let out = nsig::solve(json!({
        "type": "player",
        "player": player_js,
        "requests": requests,
    }))?;
    log.push_str(&format!("EJS solve: {:?}\n", t.elapsed()));
    let mut sig_map = HashMap::new();
    if !sigs.is_empty() {
        sig_map = solved_map(&out, 0)?;
        for (challenge, solved) in &sig_map {
            log.push_str(&format!("sig {challenge} -> {solved}\n"));
        }
    }
    let mut n_map = HashMap::new();
    if !ns.is_empty() {
        n_map = solved_map(&out, if sigs.is_empty() { 0 } else { 1 })?;
        for (challenge, solved) in &n_map {
            log.push_str(&format!("n {challenge} -> {solved}\n"));
        }
    }
    rewrite_urls(player, &pending, &sig_map, &n_map);
    Ok(())
}

/// Gathers every format that carries a URL to decipher, stripping the raw
/// cipher from the response as it goes.
fn collect_formats(player: &mut Value) -> Vec<(&'static str, usize, Pending)> {
    let mut pending = Vec::new();
    let Some(sd) = player.get_mut("streamingData") else {
        return pending;
    };
    for key in ["formats", "adaptiveFormats"] {
        let Some(list) = sd.get_mut(key).and_then(Value::as_array_mut) else {
            continue;
        };
        for (idx, f) in list.iter_mut().enumerate() {
            let Some(obj) = f.as_object_mut() else {
                continue;
            };
            let cipher = obj
                .get("signatureCipher")
                .and_then(Value::as_str)
                .or_else(|| obj.get("cipher").and_then(Value::as_str));
            let p = match cipher {
                Some(c) => match parse_cipher(c) {
                    Some((sig, sp, url)) => Pending {
                        url,
                        sig: Some(sig),
                        sp,
                    },
                    None => {
                        obj.remove("signatureCipher");
                        obj.remove("cipher");
                        continue;
                    }
                },
                None => match obj.get("url").and_then(Value::as_str) {
                    Some(u) => Pending {
                        url: u.to_string(),
                        sig: None,
                        sp: String::new(),
                    },
                    None => continue, // metadata-only: no URL to fix
                },
            };
            pending.push((key, idx, p));
        }
    }
    pending
}

/// Writes the deciphered URLs back; formats with unanswered challenges end up
/// URL-less, which `parse_player` skips.
fn rewrite_urls(
    player: &mut Value,
    pending: &[(&'static str, usize, Pending)],
    sig_map: &HashMap<String, String>,
    n_map: &HashMap<String, String>,
) {
    let Some(sd) = player.get_mut("streamingData") else {
        return;
    };
    for (key, idx, p) in pending {
        let Some(f) = sd
            .get_mut(*key)
            .and_then(Value::as_array_mut)
            .and_then(|l| l.get_mut(*idx))
        else {
            continue;
        };
        let Some(obj) = f.as_object_mut() else {
            continue;
        };
        obj.remove("signatureCipher");
        obj.remove("cipher");
        let mut url = match &p.sig {
            Some(s) => match sig_map.get(s) {
                Some(solved) => format!("{}&{}={}", p.url, p.sp, urlencode(solved)),
                None => continue,
            },
            None => p.url.clone(),
        };
        if let Some(n) = n_challenge(&url) {
            match n_map.get(n) {
                Some(solved) => url = replace_n(&url, solved),
                None => continue,
            }
        }
        obj.insert("url".into(), json!(url));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_embedded_player_response() {
        let page = "<html><script>var ytInitialPlayerResponse = {\"playabilityStatus\":{\"status\":\"OK\"},\"streamingData\":{\"adaptiveFormats\":[{\"itag\":18}]}};</script></html>";
        let pr = extract_player_response(page).unwrap();
        assert_eq!(
            pr.pointer("/playabilityStatus/status")
                .and_then(Value::as_str),
            Some("OK")
        );
        assert!(pr.pointer("/streamingData/adaptiveFormats").is_some());
    }

    #[test]
    fn skips_a_marker_without_json_and_keeps_scanning() {
        let page = "if (\"ytInitialPlayerResponse\" in window) {} var ytInitialPlayerResponse = {\"streamingData\":{}};";
        let pr = extract_player_response(page).unwrap();
        assert!(pr.get("streamingData").is_some());
    }

    #[test]
    fn json_object_end_honors_braces_inside_strings() {
        // braces and an escaped quote inside a string must not move the depth
        let page = r#"{"a":"}{\"}","b":"}"}"#;
        let end = json_object_end(page.as_bytes(), 0).unwrap();
        assert_eq!(&page[end..=end], "}");
        assert!(serde_json::from_str::<Value>(&page[..=end]).is_ok());
    }

    #[test]
    fn parses_a_cipher_and_keeps_the_inner_encoding() {
        let (s, sp, url) = parse_cipher(
            "s=AB%3D%3D&sp=sig&url=https%3A%2F%2Frr.googlevideo.com%2Fvideoplayback%3Fip%3D2001%253A1%26n%3Dabc-123",
        )
        .unwrap();
        assert_eq!(s, "AB==");
        assert_eq!(sp, "sig");
        assert_eq!(
            url,
            "https://rr.googlevideo.com/videoplayback?ip=2001%3A1&n=abc-123"
        );
        assert_eq!(n_challenge(&url), Some("abc-123"));
    }

    #[test]
    fn parse_cipher_defaults_the_sig_param_and_survives_extra_pairs() {
        let (s, sp, url) = parse_cipher("spm=xyz&s=SIG&url=https%3A%2F%2Fx%2Fv%3Fa%3D1").unwrap();
        assert_eq!(
            (s.as_str(), sp.as_str(), url.as_str()),
            ("SIG", "signature", "https://x/v?a=1")
        );
    }

    #[test]
    fn finds_the_n_challenge_only_at_a_query_boundary() {
        // not "len=…" (n not at a boundary) and not a value containing "n="
        let url = "https://x/v?len=3&n=chal&spc=n%3Dfake";
        assert_eq!(n_challenge(url), Some("chal"));
    }

    #[test]
    fn replaces_the_n_value_and_keeps_every_other_byte() {
        let url = "https://x/v?a=1&n=abc-123&b=2";
        assert_eq!(replace_n(url, "SOLVED_n"), "https://x/v?a=1&n=SOLVED_n&b=2");
    }

    #[test]
    fn replace_n_without_a_challenge_is_a_noop() {
        let url = "https://x/v?a=1";
        assert_eq!(replace_n(url, "z"), url);
    }

    #[test]
    fn solved_maps_read_the_right_response_slot() {
        let out: Value = serde_json::from_str(
            r#"{"responses":[{"data":{"s1":"S1"},"type":"result"},{"data":{"n1":"N1"},"type":"result"}],"type":"result"}"#,
        )
        .unwrap();
        assert_eq!(solved_map(&out, 0).unwrap()["s1"], "S1");
        assert_eq!(solved_map(&out, 1).unwrap()["n1"], "N1");
        assert!(solved_map(&out, 2).is_err());
    }

    #[test]
    fn collect_and_rewrite_decipher_a_canned_player_response() {
        let mut player: Value = serde_json::from_str(
            r#"{"streamingData":{
                "formats":[{"itag":18,"signatureCipher":"s=SS&sp=sig&url=https%3A%2F%2Fx%2Fv%3Fn%3DNN"}],
                "adaptiveFormats":[{"itag":140,"url":"https://x/a?n=NN"},{"itag":244,"url":"https://x/na"},{"itag":251,"signatureCipher":"s=BROKEN&sp=sig&url=https%3A%2F%2Fx%2Fu"}]}}"#,
        )
        .unwrap();
        let pending = collect_formats(&mut player);
        assert_eq!(pending.len(), 4);
        let sig_map = HashMap::from([("SS".to_string(), "SOLVED_SIG".to_string())]);
        let n_map = HashMap::from([("NN".to_string(), "SOLVED_N".to_string())]);
        rewrite_urls(&mut player, &pending, &sig_map, &n_map);
        // the raw cipher is gone from every rewritten format
        for f in player
            .pointer("/streamingData/formats")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .chain(
                player
                    .pointer("/streamingData/adaptiveFormats")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten(),
            )
        {
            assert!(f.get("signatureCipher").is_none() && f.get("cipher").is_none());
        }

        // ciphered format: solved sig appended, n replaced
        assert_eq!(
            player
                .pointer("/streamingData/formats/0/url")
                .and_then(Value::as_str)
                .unwrap(),
            "https://x/v?n=SOLVED_N&sig=SOLVED_SIG"
        );
        // plain url: only n replaced
        assert_eq!(
            player
                .pointer("/streamingData/adaptiveFormats/0/url")
                .and_then(Value::as_str)
                .unwrap(),
            "https://x/a?n=SOLVED_N"
        );
        // url without n stays untouched
        assert_eq!(
            player
                .pointer("/streamingData/adaptiveFormats/1/url")
                .and_then(Value::as_str)
                .unwrap(),
            "https://x/na"
        );
        // unanswered sig challenge: format dropped (no url, no cipher)
        let dropped = &player["streamingData"]["adaptiveFormats"][2];
        assert!(dropped.get("url").is_none() && dropped.get("signatureCipher").is_none());
    }

    /// Live, network-dependent: watch page → EJS deciphering → signed URL →
    /// HTTP 206. Run explicitly: cargo test -p streamtube-core --lib -- --ignored
    #[test]
    #[ignore]
    fn live_web_fallback_solves_and_streams() {
        let id = std::env::var("VID").unwrap_or_else(|_| "dQw4w9WgXcQ".into());
        let (info, log) = resolve_reported(&id).expect("web fallback resolves");
        eprintln!("{log}");
        let pick = info
            .muxed
            .first()
            .or_else(|| info.video.first())
            .or_else(|| info.audio.first())
            .expect("a deciphered format");
        eprintln!(
            "range-testing itag {} {}",
            pick.itag,
            &pick.url[..pick.url.len().min(90)]
        );
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .new_agent();
        let resp = agent
            .get(&pick.url)
            .header("User-Agent", crate::api::http::BROWSER_UA)
            .header("Range", "bytes=0-1023")
            .call()
            .unwrap();
        let status = resp.status().as_u16();
        let range = resp
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok().map(str::to_string));
        let bytes = resp.into_body().read_to_vec().unwrap();
        eprintln!("HTTP {status} range={range:?} bytes={}", bytes.len());
        assert_eq!(status, 206, "deciphered URL must honor range requests");
        assert_eq!(bytes.len(), 1024);
    }
}
