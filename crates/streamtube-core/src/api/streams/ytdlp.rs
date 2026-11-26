use super::*;
use std::process::Command;

static YTDLP_CACHED: parking_lot::Mutex<Option<(String, Option<String>)>> =
    parking_lot::Mutex::new(None);

/// Forget where yt-dlp was (or wasn't) found, e.g. after fetching it.
pub fn reset_ytdlp_cache() {
    *YTDLP_CACHED.lock() = None;
}

pub fn ytdlp_path() -> Option<String> {
    static CACHED: &parking_lot::Mutex<Option<(String, Option<String>)>> = &YTDLP_CACHED;
    let custom = crate::store::settings_snapshot().ytdlp_path.clone();
    if let Some((key, found)) = CACHED.lock().clone() {
        if key == custom {
            return found;
        }
    }
    let found = find_ytdlp(&custom);
    *CACHED.lock() = Some((custom, found.clone()));
    found
}

fn find_ytdlp(custom: &str) -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();
    if !custom.is_empty() {
        candidates.push(custom.to_string());
    }
    // bundled next to the app (installer) or kept in the data folder
    let exe = if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    };
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
    {
        let p = d.join(exe);
        if p.is_file() {
            candidates.push(p.to_string_lossy().into_owned());
        }
    }
    let p = crate::store::data_dir().join(exe);
    if p.is_file() {
        candidates.push(p.to_string_lossy().into_owned());
    }
    candidates.push("yt-dlp".into());
    for c in candidates {
        let mut cmd = Command::new(&c);
        cmd.arg("--version");
        hide_window(&mut cmd);
        if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
            return Some(c);
        }
    }
    None
}

pub fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = cmd;
}

pub fn ytdlp_streams(video_id: &str) -> Result<StreamInfo> {
    ytdlp_streams_with(video_id, None)
}

pub(super) fn ytdlp_streams_with(
    video_id: &str,
    cookies: Option<&std::path::Path>,
) -> Result<StreamInfo> {
    let bin = ytdlp_path()
        .ok_or_else(|| anyhow!("yt-dlp not found (install it or set its path in Settings)"))?;
    let mut cmd = Command::new(bin);
    // The app chooses its own tracks. Request the inventory even when
    // yt-dlp cannot choose a default download format.
    cmd.args(["-J", "--ignore-no-formats-error", "--no-playlist"]);
    if let Some(c) = cookies {
        cmd.arg("--cookies").arg(c);
    }
    if let Some(p) = crate::store::proxy_setting() {
        cmd.arg("--proxy").arg(p);
    }
    if crate::store::settings_snapshot().ipv4_only {
        cmd.arg("--force-ipv4");
    }
    cmd.arg(format!("https://www.youtube.com/watch?v={video_id}"));
    hide_window(&mut cmd);
    let out = cmd.output()?;
    let result = ytdlp_inventory(video_id, &out);
    if result.is_err() && retry_with_ios(&String::from_utf8_lossy(&out.stderr)) {
        let mut retry = Command::new(cmd.get_program());
        retry
            .args(cmd.get_args())
            .arg("--extractor-args")
            .arg(YTDLP_IOS);
        hide_window(&mut retry);
        if let Ok(o) = retry.output() {
            if let Ok(info) = ytdlp_inventory(video_id, &o) {
                return Ok(info);
            }
        }
    }
    result
}

fn ytdlp_inventory(video_id: &str, out: &std::process::Output) -> Result<StreamInfo> {
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(anyhow!(
            "{}",
            stderr.lines().last().unwrap_or("yt-dlp failed")
        ));
    }
    let v: Value = serde_json::from_slice(&out.stdout)?;
    let mut info = StreamInfo {
        id: video_id.to_string(),
        title: s(&v, "/title").unwrap_or_default(),
        author: s(&v, "/channel")
            .or_else(|| s(&v, "/uploader"))
            .unwrap_or_default(),
        channel_id: s(&v, "/channel_id").unwrap_or_default(),
        length_secs: v.get("duration").and_then(Value::as_f64).unwrap_or(0.0) as u64,
        view_count: v.get("view_count").and_then(Value::as_u64),
        description: s(&v, "/description").unwrap_or_default(),
        is_live: v.get("is_live").and_then(Value::as_bool).unwrap_or(false),
        source: "yt-dlp",
        ..Default::default()
    };
    for f in v
        .get("formats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(url) = s(f, "/url") else { continue };
        let proto = s(f, "/protocol").unwrap_or_default();
        let vcodec = s(f, "/vcodec").unwrap_or_else(|| "none".into());
        let acodec = s(f, "/acodec").unwrap_or_else(|| "none".into());
        if (vcodec == "none" && acodec == "none")
            || f.get("has_drm").and_then(Value::as_bool) == Some(true)
        {
            continue;
        }
        if proto.contains("m3u8") {
            if info.hls_url.is_none() {
                info.hls_url = s(f, "/manifest_url").or(Some(url.clone()));
            }
            continue;
        }
        if proto != "https" && proto != "http" {
            continue;
        }
        let height = f.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
        let sf = StreamFormat {
            itag: s(f, "/format_id")
                .and_then(|x| x.split('-').next().and_then(|n| n.parse().ok()))
                .unwrap_or(0),
            url,
            mime: format!(
                "{}/{}",
                if vcodec == "none" { "audio" } else { "video" },
                s(f, "/ext").unwrap_or_default()
            ),
            codecs: if vcodec == "none" {
                acodec.clone()
            } else {
                vcodec.clone()
            },
            bitrate: (f.get("tbr").and_then(Value::as_f64).unwrap_or(0.0) * 1000.0) as u64,
            width: f.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
            height,
            fps: f.get("fps").and_then(Value::as_f64).unwrap_or(0.0) as u32,
            quality_label: if height > 0 {
                format!("{height}p")
            } else {
                String::new()
            },
            audio_language: s(f, "/language"),
            audio_default: f
                .get("language_preference")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                >= 0,
            content_length: f.get("filesize").and_then(Value::as_u64),
            is_audio: vcodec == "none",
            is_muxed: vcodec != "none" && acodec != "none",
        };
        if sf.is_muxed {
            info.muxed.push(sf);
        } else if sf.is_audio {
            info.audio.push(sf);
        } else {
            info.video.push(sf);
        }
    }
    if info.video.is_empty()
        && info.muxed.is_empty()
        && info.audio.is_empty()
        && info.hls_url.is_none()
    {
        if let Some(reason) = stderr.lines().rev().find(|line| super::needs_sign_in(line)) {
            return Err(anyhow!("{reason}"));
        }
        return Err(anyhow!("No playable stream found"));
    }
    for (tracks, auto) in [(&v["subtitles"], false), (&v["automatic_captions"], true)] {
        if let Some(m) = tracks.as_object() {
            for (code, list) in m {
                if let Some(vtt) = list
                    .as_array()
                    .and_then(|a| a.iter().find(|x| s(x, "/ext").as_deref() == Some("vtt")))
                {
                    // Keep actual tracks, including multiple ASR languages. A timedtext
                    // URL with tlang requests an on-demand translation of another track.
                    let url = s(vtt, "/url").unwrap_or_default();
                    if auto && url.split(['?', '&']).any(|p| p.starts_with("tlang=")) {
                        continue;
                    }
                    if auto && !code.ends_with("-orig") && m.contains_key(&format!("{code}-orig")) {
                        continue;
                    }
                    info.captions.push(CaptionTrack {
                        lang: code.clone(),
                        name: s(vtt, "/name").unwrap_or(code.clone()),
                        url,
                        json3_url: list
                            .as_array()
                            .and_then(|items| items.iter().find(|item| item["ext"] == "json3"))
                            .and_then(|item| s(item, "/url")),
                        auto,
                    });
                }
            }
        }
    }
    info.original_language = s(&v, "/language")
        .and_then(|l| clean_lang(&l))
        .or_else(|| {
            v.get("formats").and_then(Value::as_array).and_then(|fmts| {
                fmts.iter()
                    .filter(|f| {
                        s(f, "/vcodec").as_deref() == Some("none")
                            && s(f, "/language").and_then(|l| clean_lang(&l)).is_some()
                    })
                    .max_by_key(|f| {
                        f.get("language_preference")
                            .and_then(Value::as_i64)
                            .unwrap_or(i64::MIN)
                    })
                    .and_then(|f| s(f, "/language"))
                    .and_then(|l| clean_lang(&l))
            })
        })
        .or_else(|| {
            info.captions
                .iter()
                .find(|c| c.auto && c.lang.ends_with("-orig"))
                .and_then(|c| clean_lang(&c.lang))
        });
    sort_formats(&mut info);
    Ok(info)
}
