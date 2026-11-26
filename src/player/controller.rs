//! Player controller: owns the mpv instance, stream selection, SponsorBlock, queue and progress.
//! Platform-independent playback decisions live in `streamtube-core`'s `playback` module.
use super::mpv::{Mpv, MpvEvent, PropValue};
use crate::api::services::{self, Segment};
use crate::api::streams;
use crate::api::types::*;
use crate::i18n::{t as translate, tf};
use crate::playback;
use crate::store;
use crate::ui::{blocking, toast};
use gpui_kit::*;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct FrameProgress {
    presented: u64,
    position: f64,
    since: Instant,
    min_advance: f64,
}

impl FrameProgress {
    fn new(presented: u64, position: f64) -> Self {
        Self { presented, position, since: Instant::now(), min_advance: 3.0 }
    }
}

pub enum PlayerEvent {
    Ended,
    Loaded,
    PlaybackReadyChanged,
    /// average colour of the picture changed (ambient mode)
    Ambient,
}

#[derive(Clone, Copy)]
pub enum KeyboardFeedback {
    Volume(f64),
    Seek(f64),
}

const BACK_CACHE_SIZE: &str = "12MiB";

pub struct Player {
    /// speed to go back to while a press-and-hold plays at 2x
    pub boost: Option<f64>,
    mpv: Option<Arc<Mpv>>,
    pub init_error: Option<String>,
    pub video: Option<VideoItem>,
    pub streams: Option<Arc<StreamInfo>>,
    pub loading: bool,
    pub playback_ready: bool,
    pub error: Option<String>,
    pub quality: u32,
    pub audio_only: bool,
    pub time: f64,
    pub duration: f64,
    pub paused: bool,
    pub buffering: bool,
    pub seeking: bool,
    cache_time: f64,
    cache_ranges: Option<Vec<(f64, f64)>>,
    // End of contiguous data from the last seek, and the farthest buffered end to reconnect.
    seek_cache_target: Option<(f64, f64)>,
    seek_disk_bytes: u64,
    preloaded_until: f64,
    pub volume: f64,
    pub muted: bool,
    pub speed: f64,
    keyboard_feedback: Option<(KeyboardFeedback, Task<()>)>,
    pub eof: bool,
    pub frame: Option<Arc<RenderImage>>,
    old_frames: Vec<Arc<RenderImage>>,
    pub video_size: (f64, f64),
    pub segments: Vec<Segment>,
    skipped: HashSet<String>,
    pub sb_prompt: Option<Segment>,
    muted_by_sb: bool,
    pub caption: Option<usize>,
    pub queue: Vec<VideoItem>,
    pub queue_pos: Option<usize>,
    pub chapters: Vec<Chapter>,
    pub heatmap: Vec<(f64, f64, f32)>,
    /// archived annotations of the current video
    pub annotations: Arc<Vec<crate::api::annotations::Annotation>>,
    /// average colour of recent frames, for the ambient glow
    pub ambient: Option<Rgba>,
    ambient_tick: u32,
    /// black bars found in recent frames, for the "Auto" aspect mode
    pub auto_crop: super::aspect::AutoCrop,
    pub loop_video: bool,
    load_serial: u64,
    last_progress_save: f64,
    pub storyboard: Option<Storyboard>,
    last_prop_notify: std::time::Instant,
    fallback_tried: bool,
    frame_progress: Option<FrameProgress>,
    hold_picture: Option<u64>,
    playback_timing: Option<(&'static str, Instant)>,
    recovering_picture: bool,
    pub channel_id: Option<String>,
    pub last_activity: std::time::Instant,
    pub audio_track: Option<String>,
    /// "Stats for nerds" overlay
    pub show_stats: bool,
}

impl EventEmitter<PlayerEvent> for Player {}

pub struct PlayerHandle(pub Entity<Player>);
impl Global for PlayerHandle {}

pub fn player(cx: &App) -> Entity<Player> {
    cx.global::<PlayerHandle>().0.clone()
}

pub fn init(cx: &mut App) {
    let p = cx.new(Player::new);
    cx.set_global(PlayerHandle(p));
}

pub fn enqueue(v: VideoItem, cx: &mut App) {
    player(cx).update(cx, |p, cx| {
        if playback::enqueue(v.clone(), p.video.as_ref(), &mut p.queue, &mut p.queue_pos) {
            p.play(v, None, cx);
        }
        cx.notify();
    });
}

pub const EQ_PRESETS: &[(&str, &str, &str)] = &[
    ("flat", "Flat", ""),
    ("bass", "Bass boost", "equalizer=f=60:width_type=o:w=1.5:g=6,equalizer=f=150:width_type=o:w=1.5:g=4"),
    ("voice", "Voice", "equalizer=f=300:width_type=o:w=1.5:g=-3,equalizer=f=1500:width_type=o:w=2:g=4,equalizer=f=3500:width_type=o:w=1.5:g=3"),
    ("treble", "Treble boost", "equalizer=f=4000:width_type=o:w=1.5:g=4,equalizer=f=10000:width_type=o:w=1.5:g=6"),
    ("warm", "Warm", "equalizer=f=100:width_type=o:w=1.5:g=3,equalizer=f=8000:width_type=o:w=1.5:g=-3"),
];

pub fn eq_label(id: &str) -> &'static str {
    EQ_PRESETS.iter().find(|&&(k, _, _)| k == id).map(|&(_, l, _)| l).unwrap_or("Flat")
}

fn eq_filter(id: &str) -> &'static str {
    EQ_PRESETS.iter().find(|&&(k, _, _)| k == id).map(|&(_, _, f)| f).unwrap_or("")
}

impl Player {
    fn new(cx: &mut Context<Self>) -> Self {
        let s = store::settings_snapshot();
        Self {
            mpv: None,
            init_error: None,
            video: None,
            streams: None,
            loading: false,
            playback_ready: false,
            error: None,
            quality: s.default_quality,
            audio_only: s.audio_only,
            time: 0.0,
            duration: 0.0,
            paused: true,
            buffering: false,
            seeking: false,
            cache_time: 0.0,
            cache_ranges: None,
            seek_cache_target: None,
            seek_disk_bytes: 0,
            preloaded_until: 0.0,
            volume: s.volume,
            muted: false,
            speed: s.default_speed,
            boost: None,
            keyboard_feedback: None,
            eof: false,
            frame: None,
            old_frames: Vec::new(),
            video_size: (16.0, 9.0),
            segments: Vec::new(),
            skipped: HashSet::new(),
            sb_prompt: None,
            muted_by_sb: false,
            caption: None,
            queue: Vec::new(),
            queue_pos: None,
            chapters: Vec::new(),
            heatmap: Vec::new(),
            annotations: Arc::new(Vec::new()),
            ambient: None,
            ambient_tick: 0,
            auto_crop: Default::default(),
            loop_video: s.loop_video,
            load_serial: 0,
            last_progress_save: 0.0,
            storyboard: None,
            last_prop_notify: std::time::Instant::now(),
            fallback_tried: false,
            frame_progress: None,
            hold_picture: None,
            playback_timing: None,
            recovering_picture: false,
            channel_id: None,
            last_activity: std::time::Instant::now(),
            audio_track: None,
            show_stats: false,
        }
        .with_cx(cx)
    }

    fn with_cx(self, _cx: &mut Context<Self>) -> Self {
        self
    }

    /// True while mpv is actually progressing through a video.
    fn should_inhibit_idle(&self) -> bool {
        self.mpv.is_some()
            && self.video.is_some()
            && !self.loading
            && !self.paused
            && !self.eof
            && self.error.is_none()
    }

    fn sync_idle(&self, cx: &mut App) {
        crate::idle_inhibit::set_playing(cx, self.should_inhibit_idle());
    }

    fn ensure_mpv(&mut self, cx: &mut Context<Self>) -> Option<Arc<Mpv>> {
        if let Some(m) = &self.mpv {
            return Some(m.clone());
        }
        let s = store::settings_snapshot();
        let (tx, rx) = futures::channel::mpsc::unbounded::<MpvEvent>();
        let vol = format!("{}", s.volume.round());
        let ua = crate::api::http::BROWSER_UA;
        let hw = if s.hwdec { "auto-copy" } else { "no" };
        let opts: Vec<(&str, &str)> = vec![
            ("vo", "libmpv"),
            ("hwdec", hw),
            ("keep-open", "yes"),
            ("idle", "yes"),
            ("ytdl", "no"),
            ("terminal", "no"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            ("osc", "no"),
            ("cache", "yes"),
            ("demuxer-max-bytes", "48MiB"),
            ("demuxer-max-back-bytes", BACK_CACHE_SIZE),
            ("demuxer-readahead-secs", "40"),
            ("network-timeout", "20"),
            ("user-agent", ua),
            ("volume", &vol),
            ("volume-max", "150"),
            ("hr-seek", "yes"),
            ("force-seekable", "yes"),
            ("audio-client-name", "Stream Tube"),
            ("af", eq_filter(&s.eq_preset)),
            ("sub-font-size", "42"),
            ("sub-border-size", "2.5"),
            ("sub-auto", "no"),
            ("osd-level", "0"),
            // software render path: convert colors only, never scale on the CPU
            ("zimg-fast", "yes"),
            ("zimg-scaler", "bilinear"),
            ("zimg-dither", "no"),
            ("zimg-threads", "auto"),
            ("sws-scaler", "fast-bilinear"),
            // libswscale's integer SIMD path converts yuv420p to bgr0 in about a quarter
            // of the CPU time zimg's float pipeline needs (10.7 -> 2.9 ms a frame at
            // 1080p60 on an M-series Mac), with the same colours
            ("sws-allow-zimg", "no"),
            ("sws-fast", "yes"),
            ("video-sync", "audio"),
            ("framedrop", "vo"),
        ];
        let opts: Vec<(&str, &str)> = opts.into_iter().collect();
        let proxy = s.proxy.clone();
        let mut opts_owned: Vec<(String, String)> = opts.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        // Embedded libmpv can have no default cache directory.
        let cache_dir = store::data_dir().join("playback-cache");
        let cache_dir = match std::fs::create_dir_all(&cache_dir) {
            Ok(()) => cache_dir,
            Err(e) => {
                eprintln!("[player] Could not create playback cache directory: {e}");
                std::env::temp_dir()
            }
        };
        opts_owned.push(("demuxer-cache-dir".into(), cache_dir.to_string_lossy().into_owned()));
        // normal videos reach mpv through the app's local stream proxy, which already
        // uses the user's proxy; mpv itself only fetches live streams directly, and can
        // only use HTTP proxies (127.0.0.1 is excluded through no_proxy, see main)
        if crate::store::mpv_http_proxy(&proxy) {
            opts_owned.push(("http-proxy".into(), proxy));
        }
        if let Ok(f) = std::env::var("STREAMTUBE_MPV_LOG") {
            opts_owned.push(("log-file".into(), f));
        }
        let refs: Vec<(&str, &str)> = opts_owned.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        match Mpv::new(&refs, move |ev| {
            let _ = tx.unbounded_send(ev);
        }) {
            Ok(m) => {
                self.mpv = Some(m.clone());
                cx.spawn(async move |this, cx| {
                    use futures::StreamExt;
                    let mut rx = rx;
                    while let Some(ev) = rx.next().await {
                        // coalesce queued events
                        let mut batch = vec![ev];
                        while let Ok(e) = rx.try_recv() {
                            batch.push(e);
                        }
                        let ok = this
                            .update(cx, |p, cx| {
                                for e in batch {
                                    p.handle_event(e, cx);
                                }
                            })
                            .is_ok();
                        if !ok {
                            break;
                        }
                    }
                })
                .detach();
                Some(m)
            }
            Err(e) => {
                let error = e.to_string();
                self.init_error = Some(match error.as_str() {
                    #[cfg(windows)]
                    super::mpv::MISSING_WINDOWS_MPV => translate(super::mpv::MISSING_WINDOWS_MPV).to_owned(),
                    _ => tf!("Could not start the video engine (libmpv): {0}", error),
                });
                None
            }
        }
    }

    pub fn mpv(&self) -> Option<Arc<Mpv>> {
        self.mpv.clone()
    }

    fn handle_event(&mut self, ev: MpvEvent, cx: &mut Context<Self>) {
        let tracked = matches!(
            &ev,
            MpvEvent::FileLoaded
                | MpvEvent::PlaybackRestart
                | MpvEvent::EndFile { .. }
                | MpvEvent::Prop(..)
                | MpvEvent::Shutdown
        );
        match ev {
            MpvEvent::Frame => {
                let t0 = std::time::Instant::now();
                crate::perf::add(&crate::perf::UI_FRAMES, 1);
                let _g = scopeguard(move || crate::perf::add(&crate::perf::UI_HANDLE_US, t0.elapsed().as_micros() as u64));
                if let Some(m) = &self.mpv {
                    if let Some(f) = m.take_frame() {
                        // A seek or reload can redraw an empty surface before
                        // the first decoded picture at the new position arrives.
                        if self.hold_picture == Some(f.picture_serial) {
                            return;
                        }
                        self.hold_picture = None;
                        self.ambient_tick += 1;
                        if self.ambient_tick % 12 == 1 && store::settings_snapshot().ambient_mode {
                            if let Some(c) = average_color(&f.data, f.width, f.height) {
                                let changed = self.ambient.map(|o| (o.r - c.r).abs() + (o.g - c.g).abs() + (o.b - c.b).abs() > 0.04).unwrap_or(true);
                                if changed {
                                    self.ambient = Some(c);
                                    cx.emit(PlayerEvent::Ambient);
                                }
                            }
                        }
                        // a few looks a second are enough to follow the bars
                        if self.ambient_tick % 10 == 0 && store::settings_snapshot().aspect_mode == "auto" {
                            self.auto_crop.feed(super::aspect::detect_bars(&f.data, f.width, f.height));
                        }
                        if let Some(buf) = image::RgbaImage::from_raw(f.width, f.height, f.data) {
                            let img = Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(buf)]));
                            if let Some(old) = self.frame.replace(img) {
                                self.old_frames.push(old);
                            }
                            // drop textures of frames that are no longer displayed
                            while self.old_frames.len() > 1 {
                                let o = self.old_frames.remove(0);
                                cx.drop_image(o, None);
                            }
                            cx.notify();
                        }
                    }
                }
            }
            MpvEvent::FileLoaded => {
                if self.video.as_ref().is_some_and(|v| v.is_short) {
                    self.cache_time = 0.0;
                }
                self.loading = false;
                self.eof = false;
                self.apply_caption();
                cx.emit(PlayerEvent::Loaded);
                cx.notify();
            }
            MpvEvent::PlaybackRestart => {
                self.frame_progress = None;
                if !self.loading && !self.buffering && self.error.is_none() && self.streams.is_some() {
                    if let Some((action, started)) = self.playback_timing.take() {
                        eprintln!("[player] {action} to playback restart: {} ms", started.elapsed().as_millis());
                    }
                    self.set_playback_ready(true, cx);
                }
                self.seeking = false;
                cx.notify();
            }
            MpvEvent::EndFile { eof, error } => {
                if let Some(e) = error {
                    if self.video.is_some() {
                        if self.loading && !self.fallback_tried {
                            self.fallback_to_ytdlp(cx);
                        } else {
                            self.loading = false;
                            self.error = Some(tf!("Playback error: {0}", e));
                        }
                    }
                }
                if eof {
                    self.on_ended(cx);
                }
                cx.notify();
            }
            MpvEvent::Prop(name, val) => {
                // While frames are flowing, the frame event already redraws the player;
                // only redraw for clock/cache updates when no video frames arrive.
                let quiet = matches!(name, "time-pos" | "demuxer-cache-time" | "demuxer-cache-state")
                    && !self.paused
                    && !self.audio_only
                    && self.frame.is_some()
                    && self.last_prop_notify.elapsed() < std::time::Duration::from_millis(500);
                match (name, val) {
                    ("time-pos", PropValue::Double(t)) => {
                        self.time = t;
                        if t > 0. && self.error.is_none() && !self.loading && !self.buffering && !self.paused && !self.playback_ready {
                            self.set_playback_ready(true, cx);
                        }
                        if !self.recover_stalled_picture(cx) {
                            self.on_time(cx);
                        }
                    }
                    ("duration", PropValue::Double(d)) => self.duration = d,
                    ("pause", PropValue::Flag(b)) => {
                        self.paused = b;
                        self.frame_progress = None;
                    }
                    ("eof-reached", PropValue::Flag(b)) => {
                        if b {
                            self.on_ended(cx);
                        } else {
                            self.eof = false;
                        }
                    }
                    ("paused-for-cache", PropValue::Flag(b)) => {
                        self.buffering = b;
                        if b {
                            self.frame_progress = None;
                            self.set_playback_ready(false, cx);
                        }
                    }
                    ("seeking", PropValue::Flag(b)) => {
                        self.seeking = b;
                        self.frame_progress = None;
                    }
                    ("demuxer-cache-time", PropValue::Double(c)) => self.cache_time = c,
                    ("demuxer-cache-state", PropValue::Cache { ranges, disk_bytes }) if !self.loading && self.streams.is_some() => {
                        self.seek_disk_bytes = self.seek_disk_bytes.max(disk_bytes);
                        if let Some((from, end)) = self.seek_cache_target {
                            let target = playback::refill_progress(from, end, &ranges, self.seek_disk_bytes);
                            self.set_seek_cache_target(target);
                        }
                        self.cache_ranges = Some(ranges);
                    }
                    ("speed", PropValue::Double(s)) => self.speed = s,
                    ("volume", PropValue::Double(v)) => self.volume = v,
                    ("mute", PropValue::Flag(m)) => self.muted = m,
                    ("dwidth", PropValue::Double(w)) => self.video_size.0 = w,
                    ("dheight", PropValue::Double(h)) => self.video_size.1 = h,
                    _ => {}
                }
                if !quiet {
                    self.last_prop_notify = std::time::Instant::now();
                    cx.notify();
                }
            }
            MpvEvent::Shutdown => {}
        }
        if tracked {
            self.sync_idle(cx);
        }
    }

    fn on_ended(&mut self, cx: &mut Context<Self>) {
        let Some(video) = &self.video else { return };
        if self.loading || self.error.is_some() || self.eof {
            return;
        }
        let live = video.is_live || self.streams.as_ref().is_some_and(|info| info.is_live);
        let position = self.mpv.as_ref().and_then(|mpv| mpv.get_f64("time-pos")).unwrap_or(self.time);
        if !live && playback::premature_eof(position, self.duration) {
            self.time = position;
            self.error = Some("Playback stopped before the end of the video.".into());
            self.save_progress(true);
            return;
        }
        self.eof = true;
        self.save_progress(true);
        if self.loop_video {
            self.eof = false;
            self.seek_to(0.0);
            self.set_paused(false, cx);
            return;
        }
        if let Some(next) = playback::next_index(&self.queue, self.queue_pos) {
            self.play_queue_index(next, cx);
            return;
        }
        cx.emit(PlayerEvent::Ended);
        self.sync_idle(cx);
    }

    pub fn play_queue_index(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some(v) = self.queue.get(idx).cloned() {
            self.queue_pos = Some(idx);
            let id = v.id.clone();
            self.play(v, None, cx);
            crate::ui::app::sync_watch_route(id, cx);
        }
    }

    /// A failed video decoder can leave mpv playing the audio indefinitely
    /// without an EndFile event. Compare rendered pictures with the media clock.
    fn recover_stalled_picture(&mut self, cx: &mut Context<Self>) -> bool {
        let (Some(mpv), Some(info)) = (&self.mpv, &self.streams) else { return false };
        if self.audio_only || self.loading || self.paused || self.buffering || self.seeking || self.eof || self.error.is_some()
            || info.is_live || (!info.hls_only && info.video.is_empty() && info.muxed.is_empty())
            || (self.duration > 0.0 && self.duration - self.time < 2.0)
        {
            self.frame_progress = None;
            return false;
        }
        let presented = mpv.presented_frames();
        let progress = self.frame_progress.get_or_insert_with(|| FrameProgress::new(presented, self.time));
        if progress.presented != presented || self.time < progress.position {
            *progress = FrameProgress::new(presented, self.time);
            return false;
        }
        let advanced = self.time - progress.position;
        if progress.since.elapsed() < Duration::from_secs(5) || advanced < progress.min_advance {
            return false;
        }
        // Still-image tracks and very low frame rates have legitimate gaps.
        let fps = mpv.get_f64("container-fps").unwrap_or(0.0);
        if fps > 0.0 && advanced < 2.0 / fps {
            progress.min_advance = 2.0 / fps;
            return false;
        }
        if ["current-tracks/video/image", "current-tracks/video/albumart"].iter().any(|p| mpv.get_string(p).as_deref() == Some("yes")) {
            self.frame_progress = None;
            return false;
        }
        self.frame_progress = None;
        if self.recovering_picture {
            self.set_paused(true, cx);
            self.set_playback_ready(false, cx);
            self.error = Some(translate("The video decoder stopped producing frames.").into());
            cx.notify();
        } else {
            self.recovering_picture = true;
            eprintln!("[player] Video frames stalled while playback advanced. Reopening at {:.1}s with software decoding.", self.time);
            self.load_current(Some(self.time), true, cx);
        }
        true
    }

    fn on_time(&mut self, cx: &mut Context<Self>) {
        let t = self.time;
        // progress save
        if (t - self.last_progress_save).abs() >= 5.0 {
            self.save_progress(false);
            let id = self.video.as_ref().map(|v| v.id.clone());
            if let Some(id) = id {
                cx.default_global::<crate::ui::cards::Watched>().0.insert(id, (t, self.duration));
            }
        }
        // SponsorBlock
        let s = store::settings_snapshot();
        if !s.sponsorblock || self.segments.is_empty() {
            return;
        }
        let decision = playback::sb_decision(&s, &self.segments, t, &self.skipped);
        for seg in decision.skips {
            self.skipped.insert(seg.uuid.clone());
            let back_to = seg.start;
            self.seek_to(seg.end);
            let server = s.sponsorblock_server.clone();
            let uuid = seg.uuid.clone();
            std::thread::spawn(move || services::sponsor_viewed(&server, &uuid));
            if s.sb_notify {
                let weak = cx.entity().downgrade();
                let mut actions: Vec<(SharedString, Arc<dyn Fn(&mut Window, &mut App)>)> = vec![(
                    translate("Unskip").into(),
                    Arc::new(move |_, cx: &mut App| {
                        if let Some(p) = weak.upgrade() {
                            p.update(cx, |p, _| p.seek_to(back_to));
                        }
                    }),
                )];
                if !s.sb_user_id.is_empty() {
                    for (label, up) in [("Upvote", true), ("Downvote", false)] {
                        let (server, uuid, user) = (s.sponsorblock_server.clone(), seg.uuid.clone(), s.sb_user_id.clone());
                        actions.push((
                            translate(label).into(),
                            Arc::new(move |_, cx: &mut App| {
                                let (server, uuid, user) = (server.clone(), uuid.clone(), user.clone());
                                cx.spawn(async move |cx| {
                                    let res = crate::ui::blocking(move || services::sponsor_vote(&server, &uuid, &user, up)).await;
                                    let _ = cx.update(|cx| match res {
                                        Ok(()) => toast(cx, translate("Thanks for voting")),
                                        Err(e) => toast(cx, tf!("Vote failed: {0}", e)),
                                    });
                                })
                                .detach();
                            }),
                        ));
                    }
                }
                crate::ui::toast_with_actions(cx, tf!("Skipped {0}", translate(services::sponsor_category_name(&seg.category))), actions);
            }
        }
        if decision.mute != self.muted_by_sb {
            self.muted_by_sb = decision.mute;
            if let Some(m) = &self.mpv {
                m.set_flag("mute", decision.mute || self.muted);
            }
        }
        self.sb_prompt = decision.prompt;
    }

    pub fn save_progress(&mut self, force: bool) {
        if let Some(v) = &self.video {
            if force || self.time > 0.0 {
                // the periodic save goes to a background thread; a forced one (switching
                // videos, the end, quitting) is written before going on
                store::history_progress_queued(&v.id, self.time, self.duration, force);
                self.last_progress_save = self.time;
                crate::api::web::report_progress(&v.id, self.time, !self.paused);
            }
        }
    }

    /// Starts playing a video. Fetches streams, SponsorBlock segments and resumes progress.
    fn set_playback_ready(&mut self, ready: bool, cx: &mut Context<Self>) {
        if self.playback_ready != ready {
            self.playback_ready = ready;
            if !ready {
                super::shorts_preload::suspend();
            }
            cx.emit(PlayerEvent::PlaybackReadyChanged);
        }
    }

    pub fn play(&mut self, video: VideoItem, start: Option<f64>, cx: &mut Context<Self>) {
        self.playback_timing = std::env::var_os("STREAMTUBE_PLAYBACK_TIMING").map(|_| ("open", Instant::now()));
        self.keyboard_feedback = None;
        self.preloaded_until = 0.0;
        self.set_playback_ready(false, cx);
        self.save_progress(true);
        if let Some(prev) = &self.video {
            crate::api::web::report_stop(&prev.id, self.time);
        }
        let Some(mpv) = self.ensure_mpv(cx) else {
            self.video = Some(video);
            self.sync_idle(cx);
            cx.notify();
            return;
        };
        self.load_serial += 1;
        self.fallback_tried = false;
        self.recovering_picture = false;
        self.frame_progress = None;
        let serial = self.load_serial;
        self.video = Some(video.clone());
        self.loading = true;
        self.error = None;
        self.time = 0.0;
        self.duration = video.duration_secs.unwrap_or(0) as f64;
        self.eof = false;
        self.segments.clear();
        self.skipped.clear();
        self.sb_prompt = None;
        self.streams = None;
        self.storyboard = None;
        self.chapters.clear();
        self.heatmap.clear();
        self.annotations = Arc::new(Vec::new());
        self.ambient = None;
        self.auto_crop = Default::default();
        self.channel_id = video.channel_id.clone();
        self.audio_track = None;
        if let Some(old) = self.frame.take() {
            cx.drop_image(old, None);
        }
        let _ = mpv.command(&["stop"]);
        store::history_add(&video);
        // with the browser sign-in, the video goes into the account's YouTube history
        // and the position reported as it plays is where other devices resume
        crate::api::web::report_start(&video.id, self.duration);
        self.sync_idle(cx);
        let s = store::settings_snapshot();
        let id = video.id.clone();
        let resume = if start.is_some() {
            start
        } else if s.resume_playback {
            playback::resume_position(store::progress_map().get(&id))
        } else {
            None
        };
        cx.notify();
        if s.annotations {
            self.load_annotations(cx);
        }
        if s.sponsorblock {
            let segment_id = id.clone();
            cx.spawn(async move |this, cx| {
                let segments = blocking(move || {
                    let categories: Vec<String> = s
                        .sb_actions
                        .iter()
                        .filter(|(_, action)| action.as_str() != "off")
                        .map(|(category, _)| category.clone())
                        .collect();
                    services::sponsor_segments(&segment_id, &s.sponsorblock_server, &categories)
                        .unwrap_or_default()
                })
                .await;
                let _ = this.update(cx, |p, cx| {
                    if p.load_serial == serial {
                        p.segments = segments;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.spawn(async move |this, cx| {
            let res = blocking(move || streams::get_streams(&id)).await;
            let _ = this.update(cx, |p, cx| {
                if p.load_serial != serial {
                    return;
                }
                match res {
                    Ok(info) => {
                        let info = Arc::new(info);
                        if p.duration <= 0.0 {
                            p.duration = info.length_secs as f64;
                        }
                        if let Some(v) = p.video.as_mut() {
                            if v.channel_id.is_none() && !info.channel_id.is_empty() {
                                v.channel_id = Some(info.channel_id.clone());
                            }
                            if v.title.is_empty() {
                                v.title = info.title.clone();
                            }
                            if v.channel_name.is_empty() {
                                v.channel_name = info.author.clone();
                            }
                        }
                        if p.channel_id.is_none() && !info.channel_id.is_empty() {
                            p.channel_id = Some(info.channel_id.clone());
                        }
                        p.storyboard = info.storyboard.clone();
                        p.streams = Some(info);
                        // default caption
                        let s = store::settings_snapshot();
                        p.caption = if s.captions_enabled {
                            p.streams.as_ref().and_then(|i| pick_caption(i, &s.captions_lang))
                        } else {
                            None
                        };
                        // speed per channel
                        let speed = p
                            .channel_id
                            .as_deref()
                            .filter(|_| s.remember_speed_per_channel)
                            .and_then(store::channel_speed)
                            .unwrap_or(s.default_speed);
                        p.speed = speed;
                        p.load_current(resume, false, cx);
                    }
                    Err(e) => {
                        p.loading = false;
                        p.error = Some(e.to_string());
                    }
                }
                p.sync_idle(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Fetches the current video's archived annotations (most videos have none).
    pub fn load_annotations(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.video.as_ref().map(|v| v.id.clone()) else { return };
        let serial = self.load_serial;
        cx.spawn(async move |this, cx| {
            let list = blocking(move || crate::api::annotations::fetch(&id).unwrap_or_default()).await;
            let _ = this.update(cx, |p, cx| {
                if p.load_serial == serial && !list.is_empty() {
                    p.annotations = Arc::new(list);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The built-in streams failed to open: fetch them again through yt-dlp.
    fn fallback_to_ytdlp(&mut self, cx: &mut Context<Self>) {
        self.preloaded_until = 0.0;
        self.fallback_tried = true;
        let Some(id) = self.video.as_ref().map(|v| v.id.clone()) else { return };
        streams::invalidate(&id);
        let serial = self.load_serial;
        let resume = (self.time > 1.0).then_some(self.time);
        cx.spawn(async move |this, cx| {
            let res = blocking(move || streams::ytdlp_streams(&id)).await;
            let _ = this.update(cx, |p, cx| {
                if p.load_serial != serial {
                    return;
                }
                match res {
                    Ok(info) => {
                        if p.storyboard.is_none() {
                            p.storyboard = info.storyboard.clone();
                        }
                        p.streams = Some(Arc::new(info));
                        p.load_current(resume, false, cx);
                    }
                    Err(e) => {
                        p.loading = false;
                        p.error = Some(tf!("Could not play this video: {0}", e));
                    }
                }
                p.sync_idle(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// (Re)loads current streams into mpv with current quality/audio settings.
    fn load_current(&mut self, start: Option<f64>, keep_pause: bool, cx: &mut Context<Self>) {
        // mpv restores file-local options when the previous file closes.
        self.seek_cache_target = None;
        self.seek_disk_bytes = 0;
        self.cache_ranges = None;
        self.cache_time = 0.0;
        self.preloaded_until = 0.0;
        self.frame_progress = None;
        self.set_playback_ready(false, cx);
        let (Some(mpv), Some(info)) = (self.mpv.clone(), self.streams.clone()) else { return };
        if let Some(("open", started)) = self.playback_timing {
            eprintln!("[player] open to stream selection: {} ms ({})", started.elapsed().as_millis(), info.source);
        }
        self.hold_picture = Some(mpv.presented_frames());
        let s = store::settings_snapshot();
        let is_short = self.video.as_ref().is_some_and(|v| v.is_short) && !info.is_live;
        let selection = streams::playback_stream(
            &info,
            self.quality,
            &s.codec_pref,
            self.audio_track.as_deref(),
            self.audio_only,
            self.recovering_picture,
        );
        let mut opening = 0.0;
        let src = match selection {
            Some(streams::PlaybackStream::Hls(url)) if info.hls_only || is_short => {
                let audio = streams::hls_audio_choice(&info, self.audio_track.as_deref());
                let source = super::proxy::playback_hls(url, self.quality, audio.as_deref(), self.recovering_picture);
                if is_short && !self.recovering_picture && source != url && start.unwrap_or(0.0) == 0.0 {
                    opening = super::shorts_preload::hls_opening_seconds(
                        url,
                        self.quality,
                        audio.as_deref(),
                    )
                    .unwrap_or(0.0);
                }
                Some(source)
            }
            Some(streams::PlaybackStream::Hls(url)) => {
                // libmpv only speaks HTTP proxies: with a SOCKS proxy, live HLS goes through the local proxy
                if crate::store::proxy_setting().is_some_and(|p| !crate::store::mpv_http_proxy(&p)) {
                    let audio = streams::hls_audio_choice(&info, self.audio_track.as_deref());
                    Some(super::proxy::local_hls(url, self.quality, audio.as_deref()))
                } else {
                    Some(url.to_owned())
                }
            }
            Some(streams::PlaybackStream::Muxed(m)) if is_short => {
                Some(super::proxy::local_url(&m.url, m.content_length, &m.mime))
            }
            Some(streams::PlaybackStream::Muxed(m)) => Some(m.url.clone()),
            Some(streams::PlaybackStream::Adaptive(v, a)) => {
                streams::mpv_source(v, a, info.length_secs)
            }
            None => None,
        };
        let Some(src) = src else {
            self.error = Some(translate("No playable stream found").into());
            self.loading = false;
            self.sync_idle(cx);
            return;
        };
        let was_paused = self.paused;
        let mut opts = vec![format!("speed={}", self.speed)];
        if self.recovering_picture {
            opts.push("hwdec=no".into());
        } else if info.hls_only && !info.is_live {
            // The proxy already selects one HLS rendition. Limit how much of its
            // opening segments libavformat reads before it can start or resume.
            opts.push("demuxer-lavf-analyzeduration=0.8".into());
        }
        // Bound network readahead so previously buffered ranges survive a seek.
        if !info.is_live {
            opts.push("cache-secs=40".into());
        }
        if let Some(t) = start.filter(|t| *t > 0.0) {
            opts.push(format!("start={t}"));
        }
        if keep_pause && was_paused {
            opts.push("pause=yes".into());
        } else {
            opts.push("pause=no".into());
        }
        if let Some(v) = &self.video {
            opts.push(mpv_media_title_option(&v.title));
        }
        mpv.set("vid", if self.audio_only { "no" } else { "auto" });
        self.loading = true;
        // mpv 0.38 added an index argument; older libmpv (Ubuntu 22.04's 0.34) takes options third
        let joined = opts.join(",");
        let res = mpv.command(&["loadfile", &src, "replace", "-1", &joined]).or_else(|_| mpv.command(&["loadfile", &src, "replace", &joined]));
        if let Err(e) = res {
            self.loading = false;
            self.error = Some(e.to_string());
        } else {
            self.preloaded_until = opening;
        }
        cx.notify();
    }

    pub fn buffered_ranges(&self) -> Vec<(f64, f64)> {
        let mut ranges = Vec::new();
        if !self.loading {
            if let Some(cached) = &self.cache_ranges {
                ranges.extend_from_slice(cached);
            } else if self.cache_time > self.time {
                ranges.push((self.time, self.cache_time));
            }
        }
        if self.preloaded_until > 0.0 {
            ranges.push((0.0, self.preloaded_until));
        }
        ranges
    }

    /// Applies caption styling settings to mpv (see [`caption_font`] and [`mpv_color`]).
    fn apply_caption_style(&self) {
        let Some(mpv) = &self.mpv else { return };
        let s = store::settings_snapshot();
        mpv.set("sub-font-size", &s.caption_size.to_string());
        mpv.set("sub-font", caption_font(&s.caption_font));
        mpv.set("sub-bold", "no");
        mpv.set("sub-margin-y", "40");
        mpv.set("sub-color", &mpv_color(s.caption_color, s.caption_opacity));
        if s.caption_background {
            mpv.set("sub-border-style", "background-box");
            mpv.set("sub-back-color", &mpv_color(s.caption_bg_color, s.caption_bg_opacity));
            mpv.set("sub-border-size", "2");
            mpv.set("sub-shadow-offset", "0");
        } else {
            mpv.set("sub-border-style", "outline-and-shadow");
            mpv.set("sub-border-color", "#FF000000");
            mpv.set("sub-shadow-color", "#80000000");
            let (border, shadow) = match s.caption_edge.as_str() {
                "outline" => ("2.5", "0"),
                "shadow" => ("0", "2"),
                "none" => ("0", "0"),
                _ => ("2.5", "1"),
            };
            mpv.set("sub-border-size", border);
            mpv.set("sub-shadow-offset", shadow);
        }
    }

    /// Re-applies settings that affect the current playback.
    pub fn apply_settings(&mut self, cx: &mut Context<Self>) {
        let s = store::settings_snapshot();
        self.loop_video = s.loop_video;
        if let Some(m) = &self.mpv {
            m.set("hwdec", if s.hwdec { "auto-copy" } else { "no" });
        }
        self.apply_caption_style();
        if self.video.is_some() {
            if s.audio_only != self.audio_only {
                self.set_audio_only(s.audio_only, cx);
            }
            let want_caps = s.captions_enabled;
            if want_caps {
                let desired = self.streams.as_ref().and_then(|i| pick_caption(i, &s.captions_lang));
                if desired != self.caption {
                    self.caption = desired;
                    self.apply_caption();
                }
            } else if self.caption.is_some() {
                self.caption = None;
                self.apply_caption();
            }
        }
        cx.notify();
    }

    fn apply_caption(&mut self) {
        self.apply_caption_style();
        let (Some(mpv), Some(info)) = (&self.mpv, &self.streams) else { return };
        let _ = mpv.command(&["sub-remove"]);
        if let Some(c) = self.caption.and_then(|i| info.captions.get(i)) {
            let url = if crate::store::proxy_setting().is_some() {
                crate::player::proxy::local_pass(&c.url)
            } else {
                c.url.clone()
            };
            let _ = mpv.command(&["sub-add", &url, "select", &c.name, &c.lang]);
        } else {
            mpv.set("sid", "no");
        }
    }

    pub fn set_caption(&mut self, idx: Option<usize>, cx: &mut Context<Self>) {
        self.caption = idx;
        self.apply_caption();
        if let Some(i) = idx {
            if let Some(c) = self.streams.as_ref().and_then(|s| s.captions.get(i)) {
                let lang = strip_orig_suffix(&c.lang).split('-').next().unwrap_or("en").to_string();
                store::update_settings(|s| {
                    s.captions_enabled = true;
                    s.captions_lang = lang;
                });
            }
        } else {
            store::update_settings(|s| s.captions_enabled = false);
        }
        cx.notify();
    }

    /// Toggles captions on/off (for the `C` shortcut) without overwriting `captions_lang`
    /// so `"original"` stays intact.
    pub fn toggle_captions(&mut self, cx: &mut Context<Self>) {
        if self.caption.is_some() {
            self.caption = None;
            self.apply_caption();
            store::update_settings(|s| s.captions_enabled = false);
        } else {
            let lang = store::settings_snapshot().captions_lang.clone();
            let next = self
                .streams
                .as_ref()
                .and_then(|i| pick_caption(i, &lang).or_else(|| (!i.captions.is_empty()).then_some(0)));
            self.caption = next;
            self.apply_caption();
            if next.is_some() {
                store::update_settings(|s| s.captions_enabled = true);
            }
        }
        cx.notify();
    }

    /// Where "Jump ahead" goes: the next part of the video most viewers skip to.
    pub fn jump_ahead_target(&self) -> Option<f64> {
        playback::jump_ahead_target(&self.heatmap, self.time)
    }

    pub fn set_quality(&mut self, q: u32, cx: &mut Context<Self>) {
        if q > 0 && store::settings_snapshot().remember_quality {
            store::update_settings(|s| s.default_quality = q);
        }
        self.quality = q;
        let t = self.time;
        self.load_current(Some(t), true, cx);
    }

    pub fn set_audio_track(&mut self, lang: Option<String>, cx: &mut Context<Self>) {
        self.audio_track = lang;
        let t = self.time;
        self.load_current(Some(t), true, cx);
    }

    pub fn set_audio_only(&mut self, on: bool, cx: &mut Context<Self>) {
        self.audio_only = on;
        store::update_settings(|s| s.audio_only = on);
        if let Some(old) = self.frame.take() {
            cx.drop_image(old, None);
        }
        let t = self.time;
        self.load_current(Some(t), true, cx);
    }

    pub fn available_qualities(&self) -> Vec<u32> {
        let mut q: Vec<u32> = self
            .streams
            .as_ref()
            .map(|s| s.video.iter().filter(|f| f.height > 0).map(streams::fmt_class).collect())
            .unwrap_or_default();
        q.sort_unstable_by(|a, b| b.cmp(a));
        q.dedup();
        q
    }

    pub fn audio_tracks(&self) -> Vec<String> {
        let mut t: Vec<String> = self
            .streams
            .as_ref()
            .map(|s| s.audio.iter().filter_map(|a| a.audio_language.clone()).collect())
            .unwrap_or_default();
        t.sort();
        t.dedup();
        t
    }

    pub fn current_height(&self) -> u32 {
        if self.video_size.1 <= 0.0 {
            return 0;
        }
        streams::quality_class(self.video_size.0 as u32, self.video_size.1 as u32)
    }

    /// Playback details for the "Stats for nerds" overlay, read from mpv.
    pub fn stats(&self) -> Vec<(&'static str, String)> {
        let Some(m) = &self.mpv else { return Vec::new() };
        let f = |k: &str| m.get_f64(k);
        let st = |k: &str| m.get_string(k).filter(|s| !s.is_empty());
        let mut out: Vec<(&'static str, String)> = Vec::new();
        if let Some(v) = &self.video {
            let source = self.streams.as_ref().map(|s| s.source).unwrap_or("");
            out.push(("Video ID", if source.is_empty() { v.id.clone() } else { tf!("{0}, via {1}", v.id, source) }));
            if v.is_short {
                let bytes = super::shorts_preload::served_bytes(&v.id);
                out.push((
                    "Read from Shorts preload",
                    tf!("{0} MB", format_args!("{:.2}", bytes as f64 / 1048576.0)),
                ));
            }
        }
        if !self.audio_only {
            let (w, h) = (f("video-params/w").unwrap_or(0.0), f("video-params/h").unwrap_or(0.0));
            let fps = f("container-fps").or_else(|| f("estimated-vf-fps")).unwrap_or(0.0);
            if w > 0.0 {
                out.push(("Resolution", format!("{w:.0}x{h:.0}@{fps:.0}")));
            }
            let dropped = f("frame-drop-count").unwrap_or(0.0) + f("decoder-frame-drop-count").unwrap_or(0.0);
            let shown = f("estimated-frame-number").unwrap_or(0.0);
            out.push(("Frames", tf!("{0} dropped of {1}", format_args!("{:.0}", dropped), format_args!("{:.0}", shown))));
            if let Some(c) = st("video-codec") {
                let pf = st("video-params/hw-pixelformat").or_else(|| st("video-params/pixelformat")).unwrap_or_default();
                out.push(("Video codec", if pf.is_empty() { c } else { format!("{c}, {pf}") }));
            }
            let hw = st("hwdec-current").filter(|s| s != "no").unwrap_or_else(|| translate("off (software)").into());
            out.push(("Hardware decoding", hw));
            let color = [st("video-params/colormatrix"), st("video-params/primaries"), st("video-params/gamma")].into_iter().flatten().collect::<Vec<_>>();
            if !color.is_empty() {
                out.push(("Color", color.join(" / ")));
            }
        }
        if let Some(c) = st("audio-codec-name").or_else(|| st("audio-codec")) {
            let rate = f("audio-params/samplerate").map(|r| tf!(", {0} kHz", format_args!("{:.1}", r / 1000.0))).unwrap_or_default();
            let ch = f("audio-params/channel-count").map(|c| tf!(", {0} ch", format_args!("{:.0}", c))).unwrap_or_default();
            out.push(("Audio codec", format!("{c}{rate}{ch}")));
        }
        let kbps = |k: &str| f(k).filter(|b| *b > 0.0).map(|b| tf!("{0} kbps", format_args!("{:.0}", b / 1000.0)));
        let rates = [kbps("video-bitrate").map(|v| tf!("video {0}", v)), kbps("audio-bitrate").map(|a| tf!("audio {0}", a))].into_iter().flatten().collect::<Vec<_>>();
        if !rates.is_empty() {
            out.push(("Bitrate", rates.join(", ")));
        }
        if let Some(b) = f("demuxer-cache-duration") {
            out.push(("Buffer health", tf!("{0} s", format_args!("{:.2}", b))));
        }
        if let Some(sp) = f("cache-speed") {
            out.push(("Connection speed", tf!("{0} Kbps", format_args!("{:.0}", sp * 8.0 / 1000.0))));
        }
        if let Some(mb) = m.get_f64("demuxer-cache-state/fw-bytes") {
            out.push(("Buffered data", tf!("{0} MB", format_args!("{:.1}", mb / 1048576.0))));
        }
        out.push(("Volume", format!("{:.0}%{}", self.volume, if self.muted { translate(" (muted)") } else { "" })));
        out.push(("Speed", format!("{:.2}x", self.speed)));
        if let Some(a) = f("avsync") {
            out.push(("A/V sync", tf!("{0} ms", format_args!("{:+.1}", a * 1000.0))));
        }
        out.push(("Position", tf!("{0} / {1} s", format_args!("{:.1}", self.time), format_args!("{:.1}", self.duration))));
        out
    }

    // ───── transport controls ─────

    pub fn toggle_pause(&mut self, cx: &mut App) {
        if self.eof {
            self.eof = false;
            self.seek_to(0.0);
            self.set_paused(false, cx);
            return;
        }
        let p = !self.paused;
        self.set_paused(p, cx);
    }

    pub fn set_paused(&mut self, p: bool, cx: &mut App) {
        self.paused = p;
        if let Some(m) = &self.mpv {
            m.set_flag("pause", p);
        }
        if p {
            self.save_progress(true);
        }
        self.sync_idle(cx);
    }

    pub fn seek_to(&mut self, t: f64) {
        self.seek(t, "absolute+exact");
    }

    pub fn seek_interactive(&mut self, t: f64) {
        // Landing on a keyframe avoids fetching and decoding an extra GOP
        // before showing the picture after a large jump through an HLS VOD.
        let hls_vod = self.streams.as_ref().is_some_and(|s| s.hls_only && !s.is_live);
        let mode = playback::seek_mode(t, self.time, self.cache_time, hls_vod);
        self.seek(t, mode);
    }

    fn seek(&mut self, t: f64, mode: &str) {
        self.playback_timing = std::env::var_os("STREAMTUBE_PLAYBACK_TIMING").map(|_| ("seek", Instant::now()));
        self.frame_progress = None;
        self.preloaded_until = 0.0;
        let t = playback::clamp_seek(t, self.duration);
        if playback::keeps_back_cache(
            t,
            self.time,
            self.seek_cache_target.is_some(),
            self.loading,
            self.streams.as_ref().is_some_and(|s| !s.is_live),
            self.seek_disk_bytes,
        ) {
            let ranges = self.cache_ranges.as_deref().unwrap_or_default();
            let target = playback::seek_refill_target(t, ranges, self.seek_cache_target);
            self.set_seek_cache_target(target);
        }
        self.time = t;
        if let Some(m) = &self.mpv {
            self.hold_picture = Some(m.presented_frames());
            let _ = m.command(&["seek", &format!("{t}"), mode]);
        }
    }

    fn set_seek_cache_target(&mut self, target: Option<(f64, f64)>) {
        if self.seek_cache_target.is_some() != target.is_some() {
            if let Some(mpv) = &self.mpv {
                // Write incoming packets to a temporary file while closing the gap.
                // Their headers still use RAM even when the payload is on disk.
                mpv.set("file-local-options/demuxer-max-back-bytes", if target.is_some() { "20MiB" } else { BACK_CACHE_SIZE });
                mpv.set("file-local-options/cache-on-disk", if target.is_some() { "yes" } else { "no" });
            }
        }
        self.seek_cache_target = target;
    }

    pub fn seek_by(&mut self, d: f64) {
        let t = self.time + d;
        self.seek_interactive(t);
    }

    pub fn keyboard_feedback(&self) -> Option<KeyboardFeedback> {
        self.keyboard_feedback.as_ref().map(|(feedback, _)| *feedback)
    }

    pub fn show_keyboard_feedback(&mut self, feedback: KeyboardFeedback, cx: &mut Context<Self>) {
        let hide = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(900)).await;
            let _ = this.update(cx, |player, cx| {
                player.keyboard_feedback = None;
                cx.notify();
            });
        });
        self.keyboard_feedback = Some((feedback, hide));
    }

    pub fn set_volume(&mut self, v: f64) {
        let v = v.clamp(0.0, 150.0);
        self.volume = v;
        if let Some(m) = &self.mpv {
            m.set_f64("volume", v);
            if self.muted && v > 0.0 {
                m.set_flag("mute", false);
            }
        }
        store::update_settings(|s| s.volume = v.min(100.0));
    }

    pub fn set_eq_preset(&mut self, preset: &str) {
        let filter = eq_filter(preset);
        if let Some(m) = &self.mpv {
            m.set("af", filter);
        }
        let p = if EQ_PRESETS.iter().any(|&(k, _, _)| k == preset) { preset } else { "flat" };
        store::update_settings(|s| s.eq_preset = p.to_string());
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        if let Some(m) = &self.mpv {
            m.set_flag("mute", self.muted);
        }
    }

    /// Press-and-hold: plays at 2x until `end_boost`, without touching the saved speed.
    pub fn start_boost(&mut self, cx: &mut App) {
        if self.boost.is_some() || self.video.is_none() {
            return;
        }
        self.boost = Some(self.speed);
        if self.paused {
            self.set_paused(false, cx);
        }
        if let Some(m) = &self.mpv {
            m.set_f64("speed", (self.speed * 2.0).clamp(2.0, 4.0));
        }
    }

    pub fn end_boost(&mut self) {
        if let Some(prev) = self.boost.take() {
            if let Some(m) = &self.mpv {
                m.set_f64("speed", prev);
            }
        }
    }

    pub fn set_speed(&mut self, s: f64) {
        let s = (s * 100.0).round() / 100.0;
        let s = s.clamp(0.1, 5.0);
        self.boost = None;
        self.speed = s;
        if let Some(m) = &self.mpv {
            m.set_f64("speed", s);
        }
        if store::settings_snapshot().remember_speed_per_channel {
            if let Some(c) = &self.channel_id {
                store::set_channel_speed(c, s);
            }
        }
    }

    pub fn step_speed(&mut self, increase: bool) {
        let steps = self.speed / 0.25;
        let next = if increase { steps.floor() + 1.0 } else { steps.ceil() - 1.0 };
        self.set_speed(next * 0.25);
    }

    pub fn set_loop(&mut self, l: bool) {
        self.loop_video = l;
    }

    pub fn frame_step(&mut self, forward: bool) {
        if let Some(m) = &self.mpv {
            let _ = m.command(&[if forward { "frame-step" } else { "frame-back-step" }]);
        }
    }

    pub fn screenshot(&mut self, cx: &mut Context<Self>) {
        if let Some(m) = &self.mpv {
            let dir = dirs::picture_dir().unwrap_or_else(std::env::temp_dir).join("Stream Tube");
            let _ = std::fs::create_dir_all(&dir);
            let name = format!(
                "{}-{}.png",
                self.video.as_ref().map(|v| v.id.as_str()).unwrap_or("frame"),
                self.time as u64
            );
            let path = dir.join(name);
            let _ = m.command(&["screenshot-to-file", &path.to_string_lossy(), "video"]);
            toast(cx, tf!("Screenshot saved to {0}", path.display()));
        }
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.playback_timing = None;
        self.keyboard_feedback = None;
        self.preloaded_until = 0.0;
        self.set_playback_ready(false, cx);
        self.save_progress(true);
        if let Some(v) = &self.video {
            crate::api::web::report_stop(&v.id, self.time);
        }
        self.load_serial += 1;
        if let Some(m) = &self.mpv {
            let _ = m.command(&["stop"]);
        }
        self.video = None;
        self.streams = None;
        self.loading = false;
        self.queue.clear();
        self.queue_pos = None;
        if let Some(old) = self.frame.take() {
            cx.drop_image(old, None);
        }
        self.sync_idle(cx);
        cx.notify();
    }

    /// Jumps to the start of the next (`dir` > 0) or previous chapter, as Ctrl+Right
    /// and Ctrl+Left do on YouTube. False when there is no chapter that way.
    pub fn seek_chapter(&mut self, dir: i32) -> bool {
        let starts: Vec<f64> = self.chapters.iter().map(|c| c.start).collect();
        match playback::chapter_target(&starts, self.time, dir) {
            Some(t) => {
                self.seek_to(t);
                true
            }
            None => false,
        }
    }

    pub fn current_chapter(&self) -> Option<&Chapter> {
        self.chapters.iter().rev().find(|c| c.start <= self.time + 0.01)
    }

    pub fn highlight(&self) -> Option<&Segment> {
        self.segments.iter().find(|s| s.action == "poi" || s.category == "poi_highlight")
    }
}

/// Applies changed settings to the running player.
pub fn apply_settings(cx: &mut App) {
    player(cx).update(cx, |p, cx| p.apply_settings(cx));
}

struct Guard<F: FnMut()>(F);
impl<F: FnMut()> Drop for Guard<F> {
    fn drop(&mut self) {
        (self.0)()
    }
}
fn scopeguard<F: FnMut()>(f: F) -> Guard<F> {
    Guard(f)
}

/// Average colour of a BGRA frame from a sparse sample grid, slightly brightened
/// so dark scenes still give a visible glow.
fn average_color(data: &[u8], w: u32, h: u32) -> Option<Rgba> {
    if w < 4 || h < 4 || data.len() < (w * h * 4) as usize {
        return None;
    }
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    for gy in 0..12 {
        for gx in 0..16 {
            let x = (w - 1) * gx / 15;
            let y = (h - 1) * gy / 11;
            let i = ((y * w + x) * 4) as usize;
            b += data[i] as u64;
            g += data[i + 1] as u64;
            r += data[i + 2] as u64;
            n += 1;
        }
    }
    let f = |v: u64| ((v as f32 / n as f32) / 255.0 * 1.25).min(1.0);
    Some(Rgba { r: f(r), g: f(g), b: f(b), a: 1.0 })
}

fn strip_orig_suffix(code: &str) -> &str {
    code.strip_suffix("-orig").unwrap_or(code)
}

fn lang_matches(track: &str, target: &str) -> bool {
    let track = strip_orig_suffix(track);
    let target = strip_orig_suffix(target);
    if target.is_empty() {
        return false;
    }
    if track.eq_ignore_ascii_case(target) {
        return true;
    }
    if track.len() > target.len()
        && track.get(..target.len()).map(|p| p.eq_ignore_ascii_case(target)).unwrap_or(false)
        && track.as_bytes().get(target.len()) == Some(&b'-')
    {
        return true;
    }
    if let Some((t_base, _)) = target.split_once('-') {
        if track.eq_ignore_ascii_case(t_base) {
            return true;
        }
        if track.len() > t_base.len()
            && track.get(..t_base.len()).map(|p| p.eq_ignore_ascii_case(t_base)).unwrap_or(false)
            && track.as_bytes().get(t_base.len()) == Some(&b'-')
        {
            return true;
        }
    }
    false
}

fn pick_for_lang(captions: &[CaptionTrack], target: &str) -> Option<usize> {
    let target_clean = strip_orig_suffix(target);
    captions
        .iter()
        .position(|c| !c.auto && strip_orig_suffix(&c.lang).eq_ignore_ascii_case(target_clean))
        .or_else(|| captions.iter().position(|c| !c.auto && lang_matches(&c.lang, target)))
        .or_else(|| captions.iter().position(|c| c.auto && strip_orig_suffix(&c.lang).eq_ignore_ascii_case(target_clean)))
        .or_else(|| captions.iter().position(|c| c.auto && lang_matches(&c.lang, target)))
}

fn base_lang(code: &str) -> String {
    strip_orig_suffix(code)
        .split('-')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Strips URLs and handles from text so English links in descriptions don't bias language scoring.
fn clean_for_lang_detect(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(1024));
    for token in text.split_whitespace().take(180) {
        let low = token.to_ascii_lowercase();
        if low.starts_with("http://")
            || low.starts_with("https://")
            || low.starts_with("www.")
            || low.contains(".com")
            || low.contains(".org")
            || low.contains(".net")
            || low.contains("youtu.be")
            || token.starts_with('@')
        {
            continue;
        }
        out.push_str(token);
        out.push(' ');
    }
    out
}

/// Scores how strongly `text` matches a given base ISO language code.
fn score_text_for_lang(text: &str, base: &str) -> i32 {
    let mut score = 0i32;

    let mut hangul = 0i32;
    let mut kana = 0i32;
    let mut cjk = 0i32;
    let mut cyrillic = 0i32;
    let mut uk_chars = 0i32;
    let mut arabic = 0i32;
    let mut persian_chars = 0i32;
    let mut urdu_chars = 0i32;
    let mut greek = 0i32;
    let mut hebrew = 0i32;
    let mut devanagari = 0i32;
    let mut thai = 0i32;
    let mut tr_strong = 0i32;
    let mut tr_soft = 0i32;
    let mut az_chars = 0i32;
    let mut es_chars = 0i32;
    let mut de_chars = 0i32;
    let mut pt_chars = 0i32;
    let mut fr_chars = 0i32;
    let mut pl_chars = 0i32;
    let mut vi_chars = 0i32;

    for ch in text.chars() {
        match ch {
            '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' => hangul += 1,
            '\u{3040}'..='\u{30FF}' => kana += 1,
            '\u{4E00}'..='\u{9FFF}' => cjk += 1,
            '\u{0400}'..='\u{04FF}' => {
                cyrillic += 1;
                if matches!(ch, 'ї' | 'Ї' | 'є' | 'Є' | 'і' | 'І' | 'ґ' | 'Ґ') {
                    uk_chars += 1;
                }
            }
            '\u{0600}'..='\u{06FF}' => {
                arabic += 1;
                if matches!(ch, 'پ' | 'چ' | 'ژ' | 'گ' | 'ی' | 'ک') {
                    persian_chars += 1;
                }
                if matches!(ch, 'ٹ' | 'ڈ' | 'ڑ' | 'ں' | 'ے') {
                    urdu_chars += 1;
                }
            }
            '\u{0370}'..='\u{03FF}' => greek += 1,
            '\u{0590}'..='\u{05FF}' => hebrew += 1,
            '\u{0900}'..='\u{097F}' => devanagari += 1,
            '\u{0E00}'..='\u{0E7F}' => thai += 1,
            'ğ' | 'Ğ' | 'ş' | 'Ş' | 'ı' | 'İ' => tr_strong += 1,
            'ç' | 'Ç' | 'ö' | 'Ö' | 'ü' | 'Ü' => tr_soft += 1,
            'ə' | 'Ə' => az_chars += 1,
            'ñ' | 'Ñ' | '¿' | '¡' => es_chars += 1,
            'ß' => de_chars += 2,
            'ä' | 'Ä' => de_chars += 1,
            'ã' | 'Ã' | 'õ' | 'Õ' => pt_chars += 1,
            'é' | 'è' | 'ê' | 'ë' | 'à' | 'ù' | 'ô' | 'î' | 'œ' => fr_chars += 1,
            'ł' | 'Ł' | 'ś' | 'Ś' | 'ź' | 'Ź' | 'ż' | 'Ż' | 'ć' | 'Ć' | 'ń' | 'Ń' | 'ą' | 'ę' => pl_chars += 1,
            'ă' | 'đ' | 'ơ' | 'ư' | 'ệ' | 'ộ' | 'ữ' | 'ả' | 'ẻ' | 'ổ' | 'ớ' | 'ờ' | 'ứ' | 'ừ' => vi_chars += 1,
            _ => {}
        }
    }

    match base {
        "ko" => return hangul * 4,
        "ja" => return kana * 5 + if kana > 0 { cjk } else { 0 },
        "zh" => return if kana == 0 && hangul == 0 { cjk * 3 } else { 0 },
        "ru" => return if uk_chars == 0 { cyrillic * 3 } else { cyrillic },
        "uk" => return uk_chars * 6 + if uk_chars > 0 { cyrillic } else { 0 },
        "bg" | "sr" | "be" | "mk" | "kk" | "ky" | "mn" => return cyrillic * 2,
        "ar" => return if persian_chars == 0 && urdu_chars == 0 { arabic * 3 } else { arabic },
        "fa" => return persian_chars * 5 + if persian_chars > 0 { arabic } else { 0 },
        "ur" => return urdu_chars * 5 + if urdu_chars > 0 { arabic } else { 0 },
        "el" => return greek * 4,
        "iw" | "he" => return hebrew * 4,
        "hi" | "mr" | "ne" => return devanagari * 4,
        "th" => return thai * 4,
        "az" => score += az_chars * 8 + tr_strong * 2,
        "tr" => score += tr_strong * 5 + tr_soft,
        "es" => score += es_chars * 5,
        "de" => score += de_chars * 4 + tr_soft,
        "pt" => score += pt_chars * 5,
        "fr" => score += fr_chars * 2,
        "pl" => score += pl_chars * 5,
        "vi" => score += vi_chars * 5,
        _ => {}
    }

    let stopwords: &[&str] = match base {
        "tr" => &[
            "ve", "bir", "bu", "için", "icin", "ile", "ama", "değil", "degil", "nasıl", "nasil",
            "nedir", "neden", "gibi", "daha", "çok", "cok", "var", "yok", "ben", "sen", "biz",
            "siz", "olan", "olarak", "çünkü", "cunku", "kadar", "şimdi", "simdi", "yeni", "bölüm",
            "bolum", "izle", "kanal", "her", "şey", "sey", "son", "böyle", "boyle", "şu", "su",
            "benim", "senin", "bizim", "oldu", "olur", "olacak", "yapmak", "yaptım", "yaptim",
        ],
        "en" => &[
            "the", "and", "you", "that", "this", "with", "for", "are", "was", "have", "from",
            "your", "what", "how", "why", "when", "where", "about", "into", "just", "like",
            "more", "will", "can", "all", "not", "but", "they", "their", "our", "out", "who",
            "here", "there", "been", "were", "would", "could", "should", "every",
        ],
        "es" => &[
            "que", "los", "las", "por", "para", "con", "una", "uno", "del", "como", "pero",
            "más", "mas", "muy", "este", "esta", "todo", "porque", "cuando", "sobre", "desde",
            "hasta", "donde", "sin", "sus", "ese", "esa", "esto", "qué", "cómo",
        ],
        "fr" => &[
            "les", "des", "une", "pour", "dans", "avec", "pas", "que", "qui", "sur", "est",
            "sont", "plus", "tout", "mais", "comme", "vous", "nous", "cette", "aussi", "bien",
            "faire", "aux", "être", "avoir", "comment", "pourquoi",
        ],
        "de" => &[
            "der", "die", "das", "und", "ist", "ein", "eine", "mit", "für", "fur", "auf",
            "nicht", "von", "den", "dem", "sich", "des", "wie", "auch", "aber", "oder", "wenn",
            "noch", "nur", "über", "uber", "nach", "wir", "ihr", "warum",
        ],
        "pt" => &[
            "que", "não", "nao", "para", "com", "uma", "por", "mais", "como", "mas", "foi",
            "são", "sao", "isso", "este", "esta", "você", "voce", "dos", "das", "tem", "muito",
            "tudo", "pelo", "pela", "porque", "quando",
        ],
        "it" => &[
            "che", "non", "per", "con", "una", "sono", "della", "delle", "degli", "questo",
            "questa", "come", "più", "piu", "anche", "tutto", "nella", "essere", "stato",
            "molto", "perché", "perche", "dove",
        ],
        "pl" => &[
            "nie", "się", "sie", "jest", "jak", "tak", "ale", "czy", "dla", "przez", "tylko",
            "bardzo", "ten", "tego", "tej", "wszystko", "może", "moze", "dlaczego",
        ],
        "nl" => &[
            "het", "een", "van", "voor", "met", "niet", "dat", "zijn", "naar", "ook", "maar",
            "nog", "wel", "deze", "over", "worden", "mijn", "wat", "waarom",
        ],
        "id" | "ms" => &[
            "yang", "dan", "ini", "untuk", "dengan", "tidak", "dari", "pada", "akan", "bisa",
            "saya", "kamu", "ada", "juga", "dalam", "atau", "tapi", "kenapa", "bagaimana",
        ],
        _ => &[],
    };

    if !stopwords.is_empty() {
        for raw_word in text.split(|c: char| !c.is_alphabetic()) {
            if raw_word.len() < 2 {
                continue;
            }
            let w = raw_word.to_lowercase();
            if stopwords.contains(&w.as_str()) {
                score += 3;
            }
        }
    }
    score
}

/// Infers which of the available caption languages best matches the video's title and description.
/// Only returns a language when the match is clear and unambiguous.
fn infer_caption_lang_from_text(info: &StreamInfo) -> Option<String> {
    let title = clean_for_lang_detect(&info.title);
    let desc = clean_for_lang_detect(&info.description);
    if title.is_empty() && desc.is_empty() {
        return None;
    }

    let mut bases: Vec<String> = Vec::new();
    for c in &info.captions {
        let b = base_lang(&c.lang);
        if !b.is_empty() && !bases.contains(&b) {
            bases.push(b);
        }
    }
    if bases.len() < 2 {
        return None;
    }

    let mut best: Option<(String, i32)> = None;
    let mut second_score = 0i32;

    for b in bases {
        // Weight title higher than description because descriptions often contain boilerplate/credits
        let s = score_text_for_lang(&title, &b) * 2 + score_text_for_lang(&desc, &b);
        match &best {
            None => best = Some((b, s)),
            Some((_, bs)) if s > *bs => {
                second_score = *bs;
                best = Some((b, s));
            }
            Some(_) => {
                if s > second_score {
                    second_score = s;
                }
            }
        }
    }

    let (lang, best_score) = best?;
    if best_score >= 6 && (best_score >= second_score * 2 || best_score >= second_score + 6) {
        Some(lang)
    } else {
        None
    }
}

/// Picks the best caption track index for `pref` (either a language code like `"en"`
/// or `crate::locale::ORIGINAL_CAPTION_LANG` for the video's original language).
pub fn pick_caption(info: &StreamInfo, pref: &str) -> Option<usize> {
    if info.captions.is_empty() {
        return None;
    }
    if pref.eq_ignore_ascii_case(crate::locale::ORIGINAL_CAPTION_LANG) {
        let text_lang = infer_caption_lang_from_text(info);

        // 1. Explicit original audio language from stream metadata
        if let Some(orig) = info.original_language.as_deref() {
            if let Some(i) = pick_for_lang(&info.captions, orig) {
                // Guard against background-music misdetection: if `orig` only matched an auto (ASR)
                // track while the video's title/description clearly matches a MANUAL caption track
                // in a different language, prefer the manual track.
                if info.captions[i].auto {
                    if let Some(tl) = text_lang.as_deref() {
                        if !lang_matches(orig, tl) {
                            if let Some(manual_i) = info
                                .captions
                                .iter()
                                .position(|c| !c.auto && lang_matches(&c.lang, tl))
                            {
                                return Some(manual_i);
                            }
                        }
                    }
                }
                return Some(i);
            }
        }

        // 2. When stream metadata doesn't specify an original language (most single-audio videos),
        // use title/description language matching BEFORE blindly trusting auto-generated ASR
        // (which YouTube often misdetects from background music).
        if let Some(tl) = text_lang.as_deref() {
            if let Some(i) = pick_for_lang(&info.captions, tl) {
                return Some(i);
            }
        }

        // 3. Uploader / YouTube default caption track for the original audio track
        if let Some(i) = info.default_caption_index.filter(|&i| i < info.captions.len()) {
            return Some(i);
        }

        // 4. Auto-generated original ASR track (preferring a manual caption of the same language)
        if let Some(asr) = info
            .captions
            .iter()
            .find(|c| c.auto && c.lang.ends_with("-orig"))
            .or_else(|| info.captions.iter().find(|c| c.auto))
        {
            if let Some(i) = pick_for_lang(&info.captions, &asr.lang) {
                return Some(i);
            }
        }

        return Some(0);
    }
    pick_for_lang(&info.captions, pref)
}

/// The font for a caption font choice. libass looks fonts up by name, and finds
/// the nearest match when one isn't installed.
pub fn caption_font(key: &str) -> &'static str {
    if cfg!(windows) {
        match key {
            "serif" => "Georgia",
            "mono" => "Consolas",
            "casual" => "Comic Sans MS",
            _ => "Segoe UI",
        }
    } else {
        match key {
            "serif" => "DejaVu Serif",
            "mono" => "DejaVu Sans Mono",
            "casual" => "Comic Neue",
            _ => "DejaVu Sans",
        }
    }
}

/// 0xRRGGBB plus an opacity in percent, as mpv's "#AARRGGBB".
pub fn mpv_color(rgb: u32, opacity: u32) -> String {
    let a = (opacity.min(100) * 255 + 50) / 100;
    format!("#{a:02X}{:06X}", rgb & 0xFFFFFF)
}

fn mpv_media_title_option(title: &str) -> String {
    // mpv's %length% quoting takes a byte count and keeps punctuation literal.
    format!("force-media-title=%{}%{title}", title.len())
}

#[cfg(test)]
mod media_title_tests {
    #[::core::prelude::v1::test]
    fn quotes_title_using_utf8_byte_length() {
        assert_eq!(
            super::mpv_media_title_option("[É], 日本語"),
            "force-media-title=%15%[É], 日本語"
        );
    }
}

#[cfg(test)]
mod caption_tests {
    use crate::api::types::{CaptionTrack, StreamInfo};

    #[::core::prelude::v1::test]
    fn colors() {
        assert_eq!(super::mpv_color(0xFFFF00, 100), "#FFFFFF00");
        assert_eq!(super::mpv_color(0x000000, 75), "#BF000000");
        assert_eq!(super::mpv_color(0x123456, 0), "#00123456");
    }

    #[::core::prelude::v1::test]
    fn original_uses_stream_language_and_prefers_manual() {
        let info = StreamInfo {
            original_language: Some("ja".into()),
            captions: vec![
                CaptionTrack { lang: "en".into(), name: "English".into(), url: String::new(), json3_url: None, auto: false },
                CaptionTrack { lang: "ja".into(), name: "Japanese (auto-generated)".into(), url: String::new(), json3_url: None, auto: true },
                CaptionTrack { lang: "ja".into(), name: "Japanese".into(), url: String::new(), json3_url: None, auto: false },
            ],
            ..Default::default()
        };
        assert_eq!(super::pick_caption(&info, "original"), Some(2));
    }

    #[::core::prelude::v1::test]
    fn original_prefers_spoken_over_music_asr() {
        // Turkish video where background English song caused English ASR
        let info = StreamInfo {
            title: "Ev Genci Nedir? Gençler Neden Çalışmıyor?".into(),
            description: "Bu bölümde yeni neslin sorunlarını konuşuyoruz. https://instagram.com/test".into(),
            captions: vec![
                CaptionTrack { lang: "en".into(), name: "English (auto-generated)".into(), url: String::new(), json3_url: None, auto: true },
                CaptionTrack { lang: "tr".into(), name: "Turkish".into(), url: String::new(), json3_url: None, auto: false },
            ],
            ..Default::default()
        };
        assert_eq!(super::pick_caption(&info, "original"), Some(1));

        // English video with Turkish fan translation still picks English ASR
        let en_info = StreamInfo {
            title: "How Rockets Work and Why They Don't Fall".into(),
            description: "In this video we explain how orbital mechanics work for beginners.".into(),
            captions: vec![
                CaptionTrack { lang: "en".into(), name: "English (auto-generated)".into(), url: String::new(), json3_url: None, auto: true },
                CaptionTrack { lang: "tr".into(), name: "Turkish".into(), url: String::new(), json3_url: None, auto: false },
            ],
            ..Default::default()
        };
        assert_eq!(super::pick_caption(&en_info, "original"), Some(0));
    }
}
