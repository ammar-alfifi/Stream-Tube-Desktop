//! Browser sign-in: the YouTube website's own session cookies.
//!
//! The TV sign-in can read the account (feeds, history, likes), but YouTube no
//! longer lets it play videos, so nothing watched through it reaches the account.
//! With the website's cookies the app reports playback the way youtube.com does:
//! the video lands in the account's watch history and the position you stopped at
//! is what other devices resume from.
//!
//! Signing in happens in a real browser (Edge, Chrome, Brave or Chromium) started
//! with its own empty profile. The app reads that profile's youtube.com cookies over
//! the browser's local debugging port once sign-in is done, then closes the browser
//! and deletes the profile. Nothing else from the user's browsers is read.
use super::http;
use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::{Duration, Instant};

const WEB_VERSION: &str = "2.20251120.01.00";
const ORIGIN: &str = "https://www.youtube.com";

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
struct Session {
    cookies: BTreeMap<String, String>,
    account: Option<String>,
}

fn session() -> &'static Mutex<Option<Session>> {
    static S: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
    S.get_or_init(|| {
        let s: Option<Session> = crate::store::kv_get("web_session").and_then(|s| serde_json::from_str(&s).ok());
        Mutex::new(s.filter(|s| has_login(&s.cookies)))
    })
}

fn save(s: &Option<Session>) {
    crate::store::kv_set("web_session", &s.as_ref().map(|s| serde_json::to_string(s).unwrap_or_default()).unwrap_or_default());
}

fn has_login(c: &BTreeMap<String, String>) -> bool {
    (c.contains_key("SAPISID") || c.contains_key("__Secure-3PAPISID")) && (c.contains_key("SID") || c.contains_key("__Secure-3PSID"))
}

/// Whether the browser sign-in is connected.
pub fn connected() -> bool {
    session().lock().is_some()
}

/// The account name the browser session belongs to.
pub fn account_name() -> Option<String> {
    session().lock().as_ref().and_then(|s| s.account.clone())
}

pub fn disconnect() {
    let mut s = session().lock();
    *s = None;
    save(&s);
}

/// Stores a set of youtube.com cookies and checks they belong to a signed-in account.
fn connect(cookies: BTreeMap<String, String>) -> Result<String> {
    if !has_login(&cookies) {
        return Err(anyhow!("These cookies aren't from a signed-in YouTube session."));
    }
    {
        let mut s = session().lock();
        *s = Some(Session { cookies, account: None });
    }
    let name = match fetch_account_name() {
        Ok(n) => n,
        Err(e) => {
            *session().lock() = None;
            return Err(e.context("YouTube didn't accept the session"));
        }
    };
    let mut s = session().lock();
    if let Some(st) = s.as_mut() {
        st.account = Some(name.clone());
    }
    save(&s);
    Ok(name)
}

/// Writes the browser sign-in's youtube.com cookies as a Netscape cookies.txt for
/// yt-dlp, in the private profile folder under a name of its own. The caller
/// deletes it when yt-dlp is done. None when the browser sign-in isn't connected.
pub fn write_cookies_file() -> Option<PathBuf> {
    use std::sync::atomic::AtomicU64;
    static N: AtomicU64 = AtomicU64::new(0);
    let text = {
        let s = session().lock();
        let s = s.as_ref()?;
        let mut t = String::from("# Netscape HTTP Cookie File\n");
        for (k, v) in &s.cookies {
            // domain, include subdomains, path, secure, expiry, name, value
            t.push_str(&format!(".youtube.com\tTRUE\t/\tTRUE\t2147483647\t{k}\t{v}\n"));
        }
        t
    };
    let path = crate::store::data_dir().join(format!("yt-dlp-cookies-{}-{}.txt", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    use std::io::Write;
    opts.open(&path).ok()?.write_all(text.as_bytes()).ok()?;
    Some(path)
}

fn cookie_header() -> Option<String> {
    let s = session().lock();
    let s = s.as_ref()?;
    Some(s.cookies.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; "))
}

fn sha1_hex(s: &str) -> String {
    use sha1::{Digest, Sha1};
    Sha1::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The `Authorization` header youtube.com sends with its API calls.
fn sapisid_auth() -> Option<String> {
    let s = session().lock();
    let c = &s.as_ref()?.cookies;
    let ts = chrono::Utc::now().timestamp();
    let parts: Vec<String> = [("SAPISIDHASH", "SAPISID"), ("SAPISID1PHASH", "__Secure-1PAPISID"), ("SAPISID3PHASH", "__Secure-3PAPISID")]
        .iter()
        .filter_map(|(scheme, name)| {
            let v = c.get(*name).or_else(|| if *name == "SAPISID" { c.get("__Secure-3PAPISID") } else { None })?;
            Some(format!("{scheme} {ts}_{}", sha1_hex(&format!("{ts} {v} {ORIGIN}"))))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// A call to the YouTube website's API as the signed-in user.
pub fn web_call(endpoint: &str, mut body: Value) -> Result<Value> {
    let cookie = cookie_header().ok_or_else(|| anyhow!("browser sign-in not connected"))?;
    let auth = sapisid_auth().ok_or_else(|| anyhow!("browser sign-in not connected"))?;
    let st = crate::store::settings_snapshot();
    body["context"] = json!({ "client": { "clientName": "WEB", "clientVersion": WEB_VERSION, "hl": st.language, "gl": st.region } });
    let (status, v) = http::post_json_status(
        &format!("https://www.youtube.com/youtubei/v1/{endpoint}?prettyPrint=false"),
        &body,
        &[
            ("Cookie", &cookie),
            ("Authorization", &auth),
            ("Origin", ORIGIN),
            ("X-Origin", ORIGIN),
            ("Referer", "https://www.youtube.com/"),
            ("X-Goog-AuthUser", "0"),
            ("X-Youtube-Bootstrap-Logged-In", "true"),
            ("X-Youtube-Client-Name", "1"),
            ("X-Youtube-Client-Version", WEB_VERSION),
        ],
    )?;
    if status == 401 || status == 403 {
        return Err(anyhow!("YouTube refused the browser session (HTTP {status}). Connect it again."));
    }
    if status >= 400 {
        return Err(anyhow!("HTTP {status}"));
    }
    Ok(v)
}

fn fetch_account_name() -> Result<String> {
    let r = web_call("account/account_menu", json!({}))?;
    let header = crate::api::json::find(&r, "activeAccountHeaderRenderer").ok_or_else(|| anyhow!("not signed in"))?;
    let name = header.get("accountName").map(crate::api::json::text).filter(|s| !s.is_empty());
    let handle = header.get("channelHandle").map(crate::api::json::text).filter(|s| !s.is_empty());
    name.or(handle).ok_or_else(|| anyhow!("not signed in"))
}

/// Regular videos of a page, without the Shorts shelf (its items carry no channel or length).
fn long_videos(p: crate::api::types::Page) -> crate::api::types::Page {
    use crate::api::types::Item;
    let mut seen = std::collections::HashSet::new();
    crate::api::types::Page {
        items: p.items.into_iter().filter(|i| matches!(i, Item::Video(v) if !v.is_short && !v.channel_name.is_empty() && seen.insert(v.id.clone()))).collect(),
        continuation: p.continuation,
    }
}

/// The account's own home feed, the same one youtube.com shows.
pub fn home() -> Result<crate::api::types::Page> {
    let r = web_call("browse", json!({ "browseId": "FEwhat_to_watch" }))?;
    Ok(long_videos(crate::api::innertube::collect_items(&r)))
}

/// Next page of a feed loaded with the browser session.
pub fn browse_continue(token: &str) -> Result<crate::api::types::Page> {
    let r = web_call("browse", json!({ "continuation": token }))?;
    Ok(long_videos(crate::api::innertube::collect_items(&r)))
}

// ───────────── sign-in through a browser ─────────────

#[cfg(not(target_os = "android"))]
mod browser;
#[cfg(not(target_os = "android"))]
pub use browser::{browser_name, browser_sign_in};

#[cfg(target_os = "android")]
pub fn browser_name() -> Option<String> {
    None
}

#[cfg(target_os = "android")]
pub fn browser_sign_in(_cancel: Arc<AtomicBool>) -> Result<String> {
    Err(anyhow!("Browser sign-in is not available on Android yet"))
}

/// Reads a Netscape-format cookies.txt (as saved by "Get cookies.txt" style
/// browser extensions) and connects with its youtube.com cookies.
pub fn import_cookies_txt(path: &std::path::Path) -> Result<String> {
    let text = std::fs::read_to_string(path)?;
    let mut cookies = BTreeMap::new();
    for line in text.lines() {
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 7 {
            continue;
        }
        let domain = f[0].trim_start_matches('.');
        if domain == "youtube.com" || domain.ends_with(".youtube.com") {
            cookies.insert(f[5].to_string(), f[6].trim_end_matches('\r').to_string());
        }
    }
    if cookies.is_empty() {
        return Err(anyhow!("No youtube.com cookies in that file."));
    }
    connect(cookies)
}

// ───────────── reporting what is watched ─────────────

fn sts() -> Option<u64> {
    static STS: Mutex<Option<(u64, Instant)>> = Mutex::new(None);
    if let Some((v, at)) = *STS.lock() {
        if at.elapsed() < Duration::from_secs(6 * 3600) {
            return Some(v);
        }
    }
    let api = String::from_utf8(http::get_bytes("https://www.youtube.com/iframe_api", &[]).ok()?).ok()?;
    let id: String = api
        .split("/s/player/")
        .nth(1)
        .or_else(|| api.split(r"player\/").nth(1))?
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    let js = String::from_utf8(http::get_bytes(&format!("https://www.youtube.com/s/player/{id}/player_ias.vflset/en_US/base.js"), &[]).ok()?).ok()?;
    let v: u64 = ["signatureTimestamp:", "sts:"]
        .iter()
        .find_map(|k| js.split(k).nth(1).and_then(|t| t.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()))?;
    *STS.lock() = Some((v, Instant::now()));
    Some(v)
}

struct Watch {
    id: String,
    playback: String,
    watchtime: String,
    cpn: String,
    len: f64,
    started: Instant,
    last_ping: Instant,
    last_cmt: f64,
    pings: u32,
    playing: bool,
}

impl Watch {
    fn begin(id: &str, len: f64) -> Result<Self> {
        let mut body = json!({ "videoId": id, "racyCheckOk": true, "contentCheckOk": true });
        if let Some(sts) = sts() {
            body["playbackContext"] = json!({ "contentPlaybackContext": { "signatureTimestamp": sts, "html5Preference": "HTML5_PREF_WANTS" } });
        }
        let r = web_call("player", body)?;
        let url = |k: &str| r.pointer(&format!("/playbackTracking/{k}/baseUrl")).and_then(Value::as_str).map(str::to_string);
        let playback = url("videostatsPlaybackUrl").ok_or_else(|| anyhow!("no playback tracking"))?;
        let watchtime = url("videostatsWatchtimeUrl").ok_or_else(|| anyhow!("no watchtime tracking"))?;
        let len = r
            .pointer("/videoDetails/lengthSeconds")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .filter(|l: &f64| *l > 0.0)
            .unwrap_or(len);
        let cpn: String = {
            use rand::Rng;
            const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
            let mut rng = rand::rng();
            (0..16).map(|_| A[rng.random_range(0..A.len())] as char).collect()
        };
        let w = Watch { id: id.to_string(), playback, watchtime, cpn, len, started: Instant::now(), last_ping: Instant::now(), last_cmt: 0.0, pings: 0, playing: true };
        w.get(&w.playback, &[("cmt", "0".into()), ("rtn", "0".into()), ("state", "playing".into())])?;
        Ok(w)
    }

    /// Sends a stats ping: the base URL from the player response with the fields
    /// youtube.com's own player adds. Fields already in the base URL are replaced,
    /// never repeated.
    fn get(&self, base: &str, fields: &[(&str, String)]) -> Result<()> {
        // the ping carries the sign-in cookies: only ever to YouTube itself
        if !is_youtube_https(base) {
            return Err(anyhow!("unexpected stats host"));
        }
        let rt = self.started.elapsed().as_secs_f64();
        let mut add: Vec<(&str, String)> = vec![
            ("ver", "2".into()),
            ("cpn", self.cpn.clone()),
            ("c", "WEB".into()),
            ("cver", WEB_VERSION.into()),
            ("cplayer", "UNIPLAYER".into()),
            ("cbr", "Chrome".into()),
            ("cbrver", "140.0.0.0".into()),
            ("cos", "Windows".into()),
            ("cosver", "10.0".into()),
            ("cplatform", "DESKTOP".into()),
            ("fs", "0".into()),
            ("volume", "100".into()),
            ("muted", "0".into()),
            ("lact", "1200".into()),
            ("len", format!("{:.3}", self.len)),
            ("rt", format!("{rt:.3}")),
        ];
        add.extend(fields.iter().cloned());
        let (path, query) = base.split_once('?').unwrap_or((base, ""));
        let mut pairs: Vec<(String, String)> = query
            .split('&')
            .filter(|kv| !kv.is_empty())
            .map(|kv| {
                let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                (k.to_string(), v.to_string())
            })
            .collect();
        for (k, v) in add {
            match pairs.iter_mut().find(|(pk, _)| pk == k) {
                Some(p) => p.1 = v,
                None => pairs.push((k.to_string(), v)),
            }
        }
        let url = format!("{path}?{}", pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&"));
        let cookie = cookie_header().unwrap_or_default();
        let referer = format!("https://www.youtube.com/watch?v={}", self.id);
        http::get_bytes(&url, &[("Cookie", &cookie), ("Referer", &referer), ("Origin", ORIGIN)])?;
        Ok(())
    }

    /// Reports the stretch watched since the last report, ending at `cmt`.
    fn report(&mut self, cmt: f64, playing: bool, last: bool) {
        let st = if cmt >= self.last_cmt && cmt - self.last_cmt < 120.0 { self.last_cmt } else { cmt };
        let rtn = self.started.elapsed().as_secs_f64();
        let mut fields = vec![
            ("cmt", format!("{cmt:.3}")),
            ("st", format!("{st:.3}")),
            ("et", format!("{cmt:.3}")),
            ("state", if playing { "playing" } else { "paused" }.to_string()),
            ("rtn", format!("{:.0}", rtn.max(1.0))),
            ("rti", format!("{:.0}", rtn.max(1.0))),
            ("idpj", "-1".into()),
            ("ldpj", "-1".into()),
        ];
        if last {
            fields.push(("final", "1".into()));
        }
        if let Err(e) = self.get(&self.watchtime, &fields) {
            if cfg!(test) {
                eprintln!("watchtime ping failed: {e}");
            }
        }
        self.last_cmt = cmt;
        self.last_ping = Instant::now();
        self.pings += 1;
        self.playing = playing;
    }

    /// youtube.com reports at 10, 20 and 30 seconds, then every 40 seconds.
    fn due(&self) -> bool {
        let every = if self.pings < 3 { 10 } else { 40 };
        self.last_ping.elapsed() >= Duration::from_secs(every)
    }
}

enum Event {
    Start { id: String, len: f64 },
    Progress { id: String, t: f64, playing: bool },
    Stop { id: String, t: f64 },
}

fn reporter() -> &'static Mutex<mpsc::Sender<Event>> {
    static TX: OnceLock<Mutex<mpsc::Sender<Event>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Event>();
        std::thread::Builder::new()
            .name("watch-report".into())
            .spawn(move || {
                let mut cur: Option<Watch> = None;
                while let Ok(ev) = rx.recv() {
                    match ev {
                        Event::Start { id, len } => {
                            if let Some(mut w) = cur.take() {
                                let t = w.last_cmt;
                                w.report(t, false, true);
                            }
                            cur = Watch::begin(&id, len).ok();
                        }
                        Event::Progress { id, t, playing } => {
                            if let Some(w) = cur.as_mut().filter(|w| w.id == id) {
                                if w.due() || playing != w.playing {
                                    w.report(t, playing, false);
                                }
                            }
                        }
                        Event::Stop { id, t } => {
                            if let Some(mut w) = cur.take() {
                                if w.id == id {
                                    w.report(t, false, true);
                                } else {
                                    cur = Some(w);
                                }
                            }
                        }
                    }
                }
            })
            .ok();
        Mutex::new(tx)
    })
}

/// A video started playing.
pub fn report_start(id: &str, len: f64) {
    if connected() && crate::store::settings_snapshot().history_enabled {
        let _ = reporter().lock().send(Event::Start { id: id.to_string(), len });
    }
}

/// Playback position update; cheap to call often.
pub fn report_progress(id: &str, t: f64, playing: bool) {
    if connected() {
        let _ = reporter().lock().send(Event::Progress { id: id.to_string(), t, playing });
    }
}

/// The video was closed or replaced at position `t`.
pub fn report_stop(id: &str, t: f64) {
    if connected() {
        let _ = reporter().lock().send(Event::Stop { id: id.to_string(), t });
    }
}

/// `https://youtube.com/...` or `https://<anything>.youtube.com/...`, nothing else.
fn is_youtube_https(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    // no user info or port tricks ("youtube.com@evil.example")
    !host.contains(['@', ':']) && (host == "youtube.com" || host.ends_with(".youtube.com"))
}

#[cfg(test)]
mod host_tests {
    use super::is_youtube_https;

    #[::core::prelude::v1::test]
    fn only_youtube() {
        assert!(is_youtube_https("https://s.youtube.com/api/stats/playback?x=1"));
        assert!(is_youtube_https("https://www.youtube.com/api/stats/watchtime"));
        assert!(!is_youtube_https("http://s.youtube.com/api"));
        assert!(!is_youtube_https("https://youtube.com.evil.example/api"));
        assert!(!is_youtube_https("https://www.youtube.com@evil.example/api"));
        assert!(!is_youtube_https("https://evilyoutube.com/api"));
    }
}
