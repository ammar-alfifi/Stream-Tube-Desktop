//! Track C3: real libmpv playback into an Android `SurfaceView`.
//!
//! The video surface is a gpui-mobile *platform view* of type `"mpv_surface"`
//! ([`ManagedPlatformView`]): GPUI reserves the layout box, and Java creates a
//! [`StreamTubeSurfaceView`] over the NativeActivity surface. When Android
//! reports the surface ready, the JNI callback below pins the `Surface`
//! jobject with a global reference and sends its pointer to the player, which
//! wires it into libmpv exactly like upstream mpv-android does:
//!
//! - `Mpv::new_external` — no software render thread, mpv renders itself;
//! - options `vo=gpu`, `gpu-context=android`, `hwdec=mediacodec,mediacodec-copy`
//!   (tries direct MediaCodec, falls back to copy), `ao=audiotrack`;
//! - the `Surface` jobject pointer as `wid`, applied through
//!   [`Mpv::set_surface`] (and pre-init as a `wid` option for libmpv builds
//!   that only accept it before `mpv_initialize`);
//! - stream resolution via `streams::get_streams` → `playback_stream` →
//!   adaptive EDL / muxed / proxied HLS, the same selection the desktop uses.
//!
//! SponsorBlock runs live on every `time-pos` update with the shared
//! `playback::sb_decision` policy: segments are fetched once when the player
//! opens, and each skip seeks to the segment end.

use std::collections::HashSet;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicI64, AtomicU64, Ordering},
};

use futures::StreamExt;
use futures::channel::mpsc::UnboundedSender;
use gpui::{EventEmitter, px, rgb};
use gpui_kit::prelude::*;
use gpui_kit::*;
use gpui_mobile::components::platform_view_element::ManagedPlatformView;
use streamtube_core::api::services::{self, Segment};
use streamtube_core::api::streams::{self, PlaybackStream};
use streamtube_core::api::types::{StreamInfo, VideoItem};
use streamtube_core::playback::sb_decision;
use streamtube_core::player::mpv::{Mpv, MpvEvent, PropValue, set_java_vm};
use streamtube_core::player::proxy;
use streamtube_core::store;

use crate::{ACCENT, BG, BORDER, TEXT, TEXT2, blocking};

// ───────────────────── Java → Rust surface events ─────────────────────

/// Messages that wake the GPUI player entity from other threads (the mpv
/// event thread, the Android main thread's JNI callbacks).
enum Msg {
    Mpv(MpvEvent),
    SurfaceCreated {
        view_id: i64,
        surface: SurfaceGlobal,
    },
    SurfaceDestroyed {
        view_id: i64,
    },
}

/// Owns a JNI global reference until it is transferred into [`Player`].
/// Dropping an undelivered message therefore cannot leak the Surface object.
struct SurfaceGlobal(usize);

impl Drop for SurfaceGlobal {
    fn drop(&mut self) {
        delete_surface_ref(self.0);
    }
}

/// The player currently listening for `StreamTubeSurfaceView` events. The
/// per-player view id prevents stale callbacks or an old player's Drop from
/// affecting a newer player.
static SURFACE_TX: Mutex<Option<(i64, UnboundedSender<Msg>)>> = Mutex::new(None);
static NEXT_VIEW_ID: AtomicU64 = AtomicU64::new(1);
/// Set before `surfaceDestroyed` waits for the Surface state lock, so mpv
/// creation cannot bind a Surface whose destruction callback has begun.
static DESTROYING_VIEW_ID: AtomicI64 = AtomicI64::new(0);

struct SurfaceState {
    valid: Option<i64>,
    mpv: Option<(i64, Weak<Mpv>)>,
}

/// Synchronizes Surface callbacks with mpv creation/rebinding. In particular,
/// `surfaceDestroyed` detaches mpv before Android invalidates the Surface.
static SURFACE_STATE: Mutex<SurfaceState> = Mutex::new(SurfaceState {
    valid: None,
    mpv: None,
});

fn next_view_id() -> i64 {
    NEXT_VIEW_ID.fetch_add(1, Ordering::Relaxed) as i64
}

/// `StreamTubeSurfaceView.nativeSurfaceCreated` — runs on the Android main
/// thread. The `Surface` jobject is only valid for this call, so it is pinned
/// with a JNI global reference whose raw pointer is handed to the player; the
/// player deletes the reference when it drops.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_mobile_StreamTubeSurfaceView_nativeSurfaceCreated<'local>(
    mut unowned_env: jni::EnvUnowned<'local>,
    _class: jni::objects::JClass<'local>,
    view_id: i64,
    surface: jni::objects::JObject<'local>,
) {
    let raw = unowned_env
        .with_env(|env| -> jni::errors::Result<usize> {
            let global = env.new_global_ref(&surface)?;
            let raw = global.as_raw() as usize;
            // deleted by the player via `global_from_raw` on drop
            std::mem::forget(global);
            Ok(raw)
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
    log::info!("StreamTubeSurfaceView surfaceCreated: view_id={view_id}");
    if raw != 0 {
        let surface = SurfaceGlobal(raw);
        let current = SURFACE_TX.lock().unwrap();
        if let Some((_id, tx)) = current.as_ref().filter(|(id, _)| *id == view_id) {
            let _ = DESTROYING_VIEW_ID.compare_exchange(
                view_id,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            SURFACE_STATE.lock().unwrap().valid = Some(view_id);
            let _ = tx.unbounded_send(Msg::SurfaceCreated { view_id, surface });
        } else {
            drop(surface);
        }
    }
}

/// Detaches an Android Surface before its JNI callback returns. If runtime
/// `wid` updates are unsupported, synchronously quit mpv instead.
fn detach_or_shutdown(mpv: &Mpv) -> bool {
    if let Err(e) = mpv.set_surface(std::ptr::null_mut()) {
        log::error!("mpv surface detach failed: {e:#}; shutting mpv down");
        if let Err(quit) = mpv.command_sync(&["quit"]) {
            log::error!("mpv synchronous shutdown failed: {quit:#}");
        }
        false
    } else {
        if let Err(e) = mpv.command_sync(&["stop"]) {
            log::error!("mpv synchronous stop failed: {e:#}");
        }
        true
    }
}

/// Deletes a JNI global reference created by `nativeSurfaceCreated`.
fn delete_surface_ref(surface: usize) {
    if surface == 0 {
        return;
    }
    let _ = gpui_mobile::android::jni::with_env(|env| {
        let global = unsafe { env.global_from_raw::<jni::objects::JObject>(surface as _) };
        drop(global);
        Ok::<(), String>(())
    });
}

/// `StreamTubeSurfaceView.nativeSurfaceDestroyed` — the player keeps its
/// global reference until it drops (it owns the mpv that still renders into
/// the surface); this only clears the current pointer.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_mobile_StreamTubeSurfaceView_nativeSurfaceDestroyed<'local>(
    _unowned_env: jni::EnvUnowned<'local>,
    _class: jni::objects::JClass<'local>,
    view_id: i64,
) {
    log::info!("StreamTubeSurfaceView surfaceDestroyed: view_id={view_id}");
    DESTROYING_VIEW_ID.store(view_id, Ordering::Release);
    {
        let mut state = SURFACE_STATE.lock().unwrap();
        if state.valid == Some(view_id) {
            state.valid = None;
        }
        let active = state
            .mpv
            .as_ref()
            .filter(|(id, _)| *id == view_id)
            .map(|(_, mpv)| mpv.clone());
        if let Some(mpv) = active.and_then(|weak| weak.upgrade()) {
            if !detach_or_shutdown(&mpv) {
                state.mpv = None;
            }
        }
    }
    if let Some((id, tx)) = SURFACE_TX.lock().unwrap().as_ref() {
        if *id == view_id {
            let _ = tx.unbounded_send(Msg::SurfaceDestroyed { view_id });
        }
    }
}

// ───────────────────── one-time wiring at app start ─────────────────────

/// Wires FFmpeg's Android JNI helpers (`av_jni_set_java_vm`, needed by
/// MediaCodec decoders and mpv's android VO) and warms up the jni crate's
/// JavaVM singleton so later global-ref deletes work from GPUI threads.
pub fn init() {
    let vm = gpui_mobile::android::jni::java_vm();
    if vm.is_null() {
        log::error!("player::init: JavaVM pointer unavailable");
        return;
    }
    match set_java_vm(vm) {
        Ok(()) => log::info!("player::init: av_jni_set_java_vm ok"),
        Err(e) => log::error!("player::init: set_java_vm failed: {e:#}"),
    }
    // initializes the jni crate's JavaVM singleton (JavaVM::from_raw)
    if let Err(e) = gpui_mobile::android::jni::with_env(|_| Ok::<(), String>(())) {
        log::warn!("player::init: with_env warmup failed: {e}");
    }
}

/// Registers the `"mpv_surface"` platform-view factory with gpui-mobile's
/// global registry. The Java side (`GpuiPlatformView.createViewForType`)
/// creates a `StreamTubeSurfaceView` for this type.
pub fn register_platform_view() {
    let registry = gpui_mobile::platform_view::PlatformViewRegistry::global();
    if !registry.has_factory("mpv_surface") {
        registry.register(
            "mpv_surface",
            Box::new(
                gpui_mobile::android::platform_view::AndroidPlatformViewFactory::new("mpv_surface"),
            ),
        );
        log::info!("player: mpv_surface platform view factory registered");
    }
}

// ───────────────────────────── the player ─────────────────────────────

#[derive(Clone)]
pub enum PlayerEvent {
    /// The Back button was pressed.
    Back,
}

enum Phase {
    /// Resolving streams or opening the file.
    Loading,
    Playing,
    Ended,
    Error(String),
}

pub struct Player {
    video: VideoItem,
    view_id: i64,
    view: ManagedPlatformView,
    /// Sender cloned into the mpv event callback.
    tx: UnboundedSender<Msg>,
    /// Raw JNI global reference to the current `Surface` jobject, if any.
    surface: Option<usize>,
    /// Whether Android still considers the surface valid. The global reference
    /// remains owned until replacement/drop so libmpv never sees it freed early.
    surface_active: bool,
    streams: Option<Arc<StreamInfo>>,
    source: Option<String>,
    segments: Vec<Segment>,
    skipped: HashSet<String>,
    mpv: Option<Arc<Mpv>>,
    phase: Phase,
    time: f64,
    duration: f64,
    paused: bool,
    /// Set when the adaptive pick failed and a muxed format was retried.
    fell_back: Option<String>,
    /// mpv paused-for-cache: upstream chunk stalled (PO-token 403s).
    buffering: bool,
    /// A source probe/fallback is currently running on the blocking pool.
    resolving: bool,
    started: std::time::Instant,
    /// Last `time-pos` value written to the log (throttled to ~5 s).
    logged_at: f64,
}

impl Player {
    pub fn new(video: VideoItem, cx: &mut Context<Self>) -> Self {
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Msg>();
        let view_id = next_view_id();
        *SURFACE_TX.lock().unwrap() = Some((view_id, tx.clone()));
        log::info!("player: opening {} ({})", video.title, video.id);
        let vid = video.id.clone();
        let vid_sb = video.id.clone();

        cx.spawn(async move |this, cx| {
            let res = blocking(move || streams::get_streams(&vid)).await;
            let _ = this.update(cx, |p, cx| {
                match res {
                    Ok(info) => {
                        let info = Arc::new(info);
                        log::info!(
                            "streams: {} ({} video-only, {} audio, {} muxed, hls={:?}, live={})",
                            info.source,
                            info.video.len(),
                            info.audio.len(),
                            info.muxed.len(),
                            info.hls_url.is_some(),
                            info.is_live
                        );
                        if p.duration <= 0.0 {
                            p.duration = info.length_secs as f64;
                        }
                        p.streams = Some(info);
                        p.try_start(cx);
                    }
                    Err(e) => {
                        log::error!("streams: {e:#}");
                        p.phase = Phase::Error(format!("Stream resolution failed: {e:#}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();

        // SponsorBlock: fetch the segments once, then decide live on
        // time-pos (same policy as the desktop player).
        let s = store::settings_snapshot();
        if s.sponsorblock {
            let server = s.sponsorblock_server.clone();
            let cats: Vec<String> = s
                .sb_actions
                .iter()
                .filter(|(_, a)| a.as_str() != "off")
                .map(|(c, _)| c.clone())
                .collect();
            cx.spawn(async move |this, cx| {
                let segs = blocking(move || {
                    services::sponsor_segments(&vid_sb, &server, &cats).unwrap_or_default()
                })
                .await;
                let _ = this.update(cx, |p, cx| {
                    log::info!("sponsorblock: {} segments for {}", segs.len(), p.video.id);
                    p.segments = segs;
                    cx.notify();
                });
            })
            .detach();
        }

        cx.spawn(async move |this, cx| {
            while let Some(msg) = rx.next().await {
                // Err => the player entity is gone; drain and stop
                if this.update(cx, |p, cx| p.on_msg(msg, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();

        let duration = video.duration_secs.unwrap_or(0) as f64;
        Self {
            video,
            view_id,
            view: ManagedPlatformView::new("mpv_surface")
                .with_param("view_id", view_id.to_string()),
            tx,
            surface: None,
            surface_active: false,
            streams: None,
            source: None,
            segments: Vec::new(),
            skipped: HashSet::new(),
            mpv: None,
            phase: Phase::Loading,
            time: 0.0,
            duration,
            paused: false,
            fell_back: None,
            buffering: false,
            resolving: false,
            started: std::time::Instant::now(),
            logged_at: -10.0,
        }
    }

    /// Creates the mpv client once both the Surface and the streams are in.
    fn try_start(&mut self, cx: &mut Context<Self>) {
        let (Some(surface), Some(_)) = (self.surface, self.streams.as_ref()) else {
            return;
        };
        if !self.surface_active || self.mpv.is_some() {
            return;
        }
        // mpv-android's proven option set: mpv renders into the Surface and
        // decodes through MediaCodec (direct, falling back to copy).
        let wid = surface.to_string();
        let opts: Vec<(&str, &str)> = vec![
            ("vo", "gpu"),
            ("gpu-context", "android"),
            ("hwdec", "mediacodec,mediacodec-copy"),
            ("ao", "audiotrack"),
            // wid pre-init (upstream pattern); set_surface repeats the same
            // pointer through the option API right after creation.
            ("wid", &wid),
            ("idle", "yes"),
            ("keep-open", "yes"),
            ("ytdl", "no"),
            ("terminal", "no"),
            ("osc", "no"),
            ("cache", "yes"),
            ("network-timeout", "20"),
            ("user-agent", streamtube_core::api::http::BROWSER_UA),
            ("volume", "80"),
        ];
        let mut surface_state = SURFACE_STATE.lock().unwrap();
        if surface_state.valid != Some(self.view_id)
            || DESTROYING_VIEW_ID.load(Ordering::Acquire) == self.view_id
        {
            return;
        }
        let tx = self.tx.clone();
        match Mpv::new_external(&opts, move |ev| {
            let _ = tx.unbounded_send(Msg::Mpv(ev));
        }) {
            Ok(m) => {
                if let Err(e) = m.set_surface(surface as *mut std::ffi::c_void) {
                    // Some libmpv builds refuse runtime wid writes; the same
                    // value was already set pre-init above, so playback is
                    // unaffected either way.
                    log::warn!("mpv set_surface failed (pre-init wid stands): {e:#}");
                } else {
                    log::info!("mpv set_surface ok");
                }
                surface_state.mpv = Some((self.view_id, Arc::downgrade(&m)));
                self.mpv = Some(m);
                drop(surface_state);
                self.load(cx);
            }
            Err(e) => {
                drop(surface_state);
                log::error!("Mpv::new_external: {e:#}");
                self.phase = Phase::Error(format!("mpv init failed: {e:#}"));
            }
        }
        cx.notify();
    }

    /// Picks the source the way the desktop does and starts playback. Direct
    /// googlevideo URLs are probed with a ranged GET first: YouTube refuses
    /// PO-token-less adaptive URLs with HTTP 403 before serving data, which
    /// otherwise leaves mpv retrying for ~90 s. Probing runs on the blocking
    /// pool so a slow network never freezes the GPUI thread.
    fn load(&mut self, cx: &mut Context<Self>) {
        self.resolve_and_load(None, cx);
    }

    /// The adaptive (EDL) pick failed — usually YouTube throttling separate
    /// streams for lack of a PO token. Retry once with the smallest muxed
    /// format and record the exact limit for the UI and the evidence.
    fn retry_muxed(&mut self, err: &str, cx: &mut Context<Self>) {
        self.resolve_and_load(Some(err.to_string()), cx);
    }

    fn resolve_and_load(&mut self, fallback: Option<String>, cx: &mut Context<Self>) {
        if self.resolving {
            return;
        }
        let Some(info) = self.streams.clone() else {
            return;
        };
        self.resolving = true;
        let settings = store::settings_snapshot();
        cx.spawn(async move |this, cx| {
            let resolved = blocking(move || resolve_source(info, settings, fallback)).await;
            let _ = this.update(cx, |p, cx| p.apply_resolved_source(resolved, cx));
        })
        .detach();
    }

    fn reset_playback(&mut self) {
        self.time = 0.0;
        self.logged_at = -10.0;
        self.skipped.clear();
        self.paused = false;
        self.buffering = false;
    }

    fn apply_resolved_source(
        &mut self,
        resolved: Result<(String, Option<String>), String>,
        cx: &mut Context<Self>,
    ) {
        self.resolving = false;
        if !self.surface_active {
            return;
        }
        let Some(mpv) = self.mpv.clone() else {
            return;
        };
        match resolved {
            Ok((src, note)) => {
                self.fell_back = note;
                self.reset_playback();
                if let Err(e) = mpv.command(&["loadfile", &src, "replace"]) {
                    log::error!("loadfile: {e:#}");
                    self.phase = Phase::Error(format!("loadfile failed: {e:#}"));
                } else {
                    self.source = Some(src);
                    log::info!("loadfile ok");
                }
            }
            Err(e) => {
                log::error!("no playable source: {e}");
                self.phase = Phase::Error(format!("Playback failed: {e}"));
            }
        }
        cx.notify();
    }

    fn on_msg(&mut self, msg: Msg, cx: &mut Context<Self>) {
        match msg {
            Msg::SurfaceCreated { view_id, surface } if view_id == self.view_id => {
                let raw = surface.0;
                let mut needs_load = false;
                let mut needs_start = false;
                {
                    let mut state = SURFACE_STATE.lock().unwrap();
                    if state.valid != Some(self.view_id)
                        || DESTROYING_VIEW_ID.load(Ordering::Acquire) == self.view_id
                    {
                        return;
                    }
                    let registered = state
                        .mpv
                        .as_ref()
                        .is_some_and(|(id, mpv)| *id == self.view_id && mpv.strong_count() > 0);
                    if self.mpv.is_some() && !registered {
                        // A previous detach failure synchronously quit this mpv.
                        self.mpv = None;
                        needs_start = true;
                    }
                    // Point an already-running mpv at the replacement surface
                    // before releasing the previous JNI global reference.
                    if let Some(m) = self.mpv.clone() {
                        if let Err(e) = m.set_surface(raw as *mut std::ffi::c_void) {
                            log::warn!("mpv surface rebind failed: {e:#}; recreating mpv");
                            detach_or_shutdown(&m);
                            state.mpv = None;
                            self.mpv = None;
                            needs_start = true;
                        }
                    }
                    std::mem::forget(surface); // ownership transfers to `self.surface`
                    if let Some(old) = self.surface.replace(raw) {
                        delete_surface_ref(old);
                    }
                    self.surface_active = true;
                    if !needs_start {
                        if self.mpv.is_none() {
                            needs_start = true;
                        } else if let Some(src) = self.source.clone() {
                            if let Some(m) = self.mpv.clone() {
                                self.reset_playback();
                                if let Err(e) = m.command(&["loadfile", &src, "replace"]) {
                                    log::error!("surface reload failed: {e:#}");
                                    self.phase = Phase::Error(format!("Reload failed: {e:#}"));
                                } else {
                                    self.phase = Phase::Loading;
                                }
                            }
                        } else if self.streams.is_some() && !self.resolving {
                            needs_load = true;
                        }
                    }
                }
                if needs_start {
                    self.try_start(cx);
                } else if needs_load {
                    self.load(cx);
                }
            }
            Msg::SurfaceDestroyed { view_id } if view_id == self.view_id => {
                // The JNI callback already detached/stopped mpv synchronously.
                self.surface_active = false;
                self.phase = Phase::Loading;
                self.buffering = false;
            }
            Msg::SurfaceCreated { .. } | Msg::SurfaceDestroyed { .. } => {}
            Msg::Mpv(ev) => self.on_mpv_event(ev, cx),
        }
        cx.notify();
    }

    fn on_mpv_event(&mut self, ev: MpvEvent, cx: &mut Context<Self>) {
        if !self.surface_active {
            return;
        }
        match ev {
            MpvEvent::FileLoaded => {
                if let Some(m) = &self.mpv {
                    let vo = m.get_string("current-vo");
                    let hwdec = m.get_string("hwdec-current");
                    let format = m.get_string("video-format");
                    let ao = m.get_string("current-ao");
                    let audio = m.get_string("audio-codec");
                    log::info!(
                        "mpv file loaded after {} ms: vo={vo:?} hwdec={hwdec:?} video-format={format:?} ao={ao:?} audio-codec={audio:?}",
                        self.started.elapsed().as_millis(),
                    );
                }
                self.phase = Phase::Playing;
            }
            MpvEvent::PlaybackRestart => {
                self.buffering = false;
                self.phase = Phase::Playing;
            }
            MpvEvent::EndFile { eof, error } => {
                if eof {
                    self.phase = Phase::Ended;
                    self.paused = true;
                } else if let Some(e) = error {
                    if !self.resolving && self.fell_back.is_none() {
                        self.retry_muxed(&e, cx);
                        return;
                    }
                    self.phase = Phase::Error(format!("Playback failed: {e}"));
                }
            }
            MpvEvent::Prop("time-pos", PropValue::Double(t)) => {
                self.time = t;
                // SponsorBlock, live on every position update
                let s = store::settings_snapshot();
                let d = sb_decision(&s, &self.segments, t, &self.skipped);
                for seg in d.skips {
                    self.skipped.insert(seg.uuid.clone());
                    log::info!(
                        "sponsor skip: {} {}s..{}s → seeking to {:.3}",
                        seg.category,
                        seg.start,
                        seg.end,
                        seg.end
                    );
                    if let Some(m) = &self.mpv {
                        let target = format!("{:.3}", seg.end);
                        let _ = m.command(&["seek", &target, "absolute+exact"]);
                    }
                }
                if (t - self.logged_at) >= 5.0 {
                    self.logged_at = t;
                    log::info!(
                        "time-pos {:.1}s / {:.1}s (paused={})",
                        t,
                        self.duration,
                        self.paused
                    );
                }
            }
            MpvEvent::Prop("duration", PropValue::Double(d)) => self.duration = d,
            MpvEvent::Prop("pause", PropValue::Flag(p)) => self.paused = p,
            MpvEvent::Prop("paused-for-cache", PropValue::Flag(b)) => self.buffering = b,
            _ => {}
        }
        cx.notify();
    }

    fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.mpv.clone() else {
            return;
        };
        self.paused = !self.paused;
        log::info!("player: pause toggled -> {}", self.paused);
        m.set_flag("pause", self.paused);
        cx.notify();
    }

    /// Jumps playback by `delta` seconds, clamped to the timeline.
    fn seek_by(&mut self, delta: f64, cx: &mut Context<Self>) {
        let t = (self.time + delta).clamp(0.0, (self.duration - 0.5).max(0.0));
        if let Some(m) = &self.mpv {
            let target = format!("{t:.3}");
            log::info!("seek → {t:.1}s");
            let _ = m.command(&["seek", &target, "absolute+exact"]);
        }
        cx.notify();
    }
}

impl EventEmitter<PlayerEvent> for Player {}

impl Render for Player {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vw = f32::from(window.viewport_size().width);
        let vh = (vw * 9.0 / 16.0).max(180.).min(vw * 3.0 / 4.0);
        let status = match &self.phase {
            Phase::Loading if self.streams.is_none() => "Resolving streams…".to_string(),
            Phase::Loading => "Opening stream…".to_string(),
            Phase::Playing if self.buffering => {
                format!(
                    "Buffering… {} / {}",
                    fmt_time(self.time),
                    fmt_time(self.duration)
                )
            }
            Phase::Playing => format!("{} / {}", fmt_time(self.time), fmt_time(self.duration)),
            Phase::Ended => format!("Ended — {}", fmt_time(self.duration)),
            Phase::Error(e) => format!("Error: {e}"),
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            // video surface (composited above this tree by Android)
            .child(self.view.render().w_full().h(px(vh)).flex_none())
            // title row with back button
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .child(
                        div()
                            .id("back")
                            .flex_none()
                            .px_3()
                            .py_1p5()
                            .rounded_lg()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .cursor_pointer()
                            .bg(rgb(BORDER))
                            .on_click(cx.listener(|_, _, _, cx| {
                                log::info!("player: back requested");
                                cx.emit(PlayerEvent::Back);
                            }))
                            .child("← Back"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .child(self.video.title.clone()),
                    ),
            )
            // channel + status
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_3()
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(TEXT2))
                            .child(self.video.channel_name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(TEXT2))
                            .child(status),
                    )
                    .when(self.segments.len() > 0, |d| {
                        d.child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(TEXT2))
                                .child(format!("SponsorBlock: {} segments", self.segments.len())),
                        )
                    })
                    .children(
                        self.fell_back.clone().map(|note| {
                            div().text_size(px(12.)).text_color(rgb(ACCENT)).child(note)
                        }),
                    ),
            )
            // controls
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .child(
                        div()
                            .id("play-pause")
                            .flex_none()
                            .px_3()
                            .py_1p5()
                            .rounded_lg()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .cursor_pointer()
                            .bg(rgb(ACCENT))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx)))
                            .child(if self.paused { "Play" } else { "Pause" }),
                    )
                    .child(
                        div()
                            .id("restart")
                            .flex_none()
                            .px_3()
                            .py_1p5()
                            .rounded_lg()
                            .text_sm()
                            .cursor_pointer()
                            .bg(rgb(BORDER))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let Some(m) = this.mpv.clone() else {
                                    return;
                                };
                                this.reset_playback();
                                this.phase = Phase::Loading;
                                if let Err(e) = m.command(&["seek", "0", "absolute"]) {
                                    log::error!("restart seek failed: {e:#}");
                                    this.phase = Phase::Error(format!("Restart failed: {e:#}"));
                                    cx.notify();
                                    return;
                                }
                                if let Err(e) = m.command(&["set", "pause", "no"]) {
                                    log::error!("restart unpause failed: {e:#}");
                                    this.phase = Phase::Error(format!("Restart failed: {e:#}"));
                                    cx.notify();
                                    return;
                                }
                                this.phase = Phase::Playing;
                                cx.notify();
                            }))
                            .child("Restart"),
                    )
                    .child(
                        div()
                            .id("seek-back")
                            .flex_none()
                            .px_3()
                            .py_1p5()
                            .rounded_lg()
                            .text_sm()
                            .cursor_pointer()
                            .bg(rgb(BORDER))
                            .on_click(cx.listener(|this, _, _, cx| this.seek_by(-10.0, cx)))
                            .child("−10s"),
                    )
                    .child(
                        div()
                            .id("seek-fwd")
                            .flex_none()
                            .px_3()
                            .py_1p5()
                            .rounded_lg()
                            .text_sm()
                            .cursor_pointer()
                            .bg(rgb(BORDER))
                            .on_click(cx.listener(|this, _, _, cx| this.seek_by(10.0, cx)))
                            .child("+10s"),
                    )
                    .child(
                        div()
                            .id("seek-fwd-60")
                            .flex_none()
                            .px_3()
                            .py_1p5()
                            .rounded_lg()
                            .text_sm()
                            .cursor_pointer()
                            .bg(rgb(BORDER))
                            .on_click(cx.listener(|this, _, _, cx| this.seek_by(60.0, cx)))
                            .child("+60s"),
                    ),
            )
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // Serialize with Surface callbacks, then terminate mpv before releasing
        // the JNI global reference and Java view.
        {
            let mut state = SURFACE_STATE.lock().unwrap();
            if state
                .mpv
                .as_ref()
                .is_some_and(|(id, _)| *id == self.view_id)
            {
                state.mpv = None;
            }
            if state.valid == Some(self.view_id) {
                state.valid = None;
            }
            // Keep the callback serialized until mpv is actually destroyed.
            self.mpv.take();
        }
        let _ = DESTROYING_VIEW_ID.compare_exchange(
            self.view_id,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        let mut current = SURFACE_TX.lock().unwrap();
        if current.as_ref().is_some_and(|(id, _)| *id == self.view_id) {
            *current = None;
        }
        drop(current);
        if let Some(surface) = self.surface.take() {
            delete_surface_ref(surface);
        }
        log::info!("player: dropped (mpv terminated, surface ref released)");
    }
}

fn resolve_source(
    info: Arc<StreamInfo>,
    settings: Arc<store::Settings>,
    fallback: Option<String>,
) -> Result<(String, Option<String>), String> {
    if let Some(err) = fallback {
        return fallback_source(&info, &err);
    }
    let quality = settings.default_quality;
    // prefer_avc: MediaCodec decodes H.264 most reliably on Android.
    let selected =
        streams::playback_stream(&info, quality, &settings.codec_pref, None, false, true);
    match selected {
        Some(PlaybackStream::Hls(url)) => {
            let audio = streams::hls_audio_choice(&info, None);
            log::info!("source: proxied HLS (audio pick {:?})", audio.as_deref());
            Ok((
                proxy::playback_hls(url, quality, audio.as_deref(), true),
                None,
            ))
        }
        Some(PlaybackStream::Muxed(f)) => {
            log::info!("source: muxed itag {} {}p {}", f.itag, f.height, f.codecs);
            if url_playable(&f.url) {
                Ok((proxy::local_url(&f.url, f.content_length, &f.mime), None))
            } else {
                fallback_hls(&info, "muxed itag refused (range probe failed)")
            }
        }
        Some(PlaybackStream::Adaptive(v, a)) => {
            let (hv, ha) = (
                v.map(|f| (f.height, f.codecs.clone())),
                a.map(|f| f.codecs.clone()),
            );
            let video_ok = v.is_none_or(|f| url_playable(&f.url));
            let audio_ok = a.is_none_or(|f| url_playable(&f.url));
            if video_ok && audio_ok {
                log::info!("source: adaptive {hv:?} + audio {ha:?} (probe ok)");
                streams::mpv_source(v, a, info.length_secs)
                    .map(|src| (src, None))
                    .ok_or_else(|| "No playable stream found".to_string())
            } else {
                let note = format!(
                    "adaptive {hv:?} + {ha:?} refused (range probe failed; PO token may be required)"
                );
                log::warn!("{note}");
                fallback_source(&info, &note)
            }
        }
        None => Err("No playable stream found".to_string()),
    }
}

fn fallback_source(info: &StreamInfo, err: &str) -> Result<(String, Option<String>), String> {
    let pick = info.muxed.iter().min_by_key(|f| f.height);
    if let Some(f) = pick.filter(|f| url_playable(&f.url)) {
        let note = format!(
            "{err}; using muxed itag {} {}p {}",
            f.itag, f.height, f.codecs
        );
        log::warn!("{note}");
        Ok((
            proxy::local_url(&f.url, f.content_length, &f.mime),
            Some(note),
        ))
    } else {
        fallback_hls(info, err)
    }
}

fn fallback_hls(info: &StreamInfo, err: &str) -> Result<(String, Option<String>), String> {
    if let Some(hls) = info.hls_url.as_deref() {
        let note = format!("{err}; using the HLS manifest");
        log::warn!("{note}");
        Ok((proxy::playback_hls(hls, 720, None, true), Some(note)))
    } else {
        Err(format!("Playback failed: {err}"))
    }
}

fn fmt_time(t: f64) -> String {
    if !t.is_finite() || t <= 0.0 {
        return "0:00".into();
    }
    let s = t as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// One ranged GET through the app's HTTP stack, sized like the stream
/// proxy's first chunk (1 MiB). Today YouTube answers ANDROID_VR adaptive
/// googlevideo URLs with HTTP 403 for ranges this large when the request
/// carries no PO token (a 1 KiB range still gets 206 — the probe must test
/// the size that will actually be fetched, or mpv gets stuck in its ~90 s
/// demuxer retry loop). Non-googlevideo URLs always pass.
fn url_playable(url: &str) -> bool {
    if !url.contains("googlevideo.com") {
        return true;
    }
    match streamtube_core::api::http::get_bytes_limited(
        url,
        &[("Range", "bytes=0-1048575")],
        1024 * 1024,
    ) {
        Ok(b) if b.len() == 1024 * 1024 => {
            log::info!("googlevideo range probe ok ({} bytes)", b.len());
            true
        }
        Ok(b) => {
            log::warn!("googlevideo range probe returned {} bytes", b.len());
            false
        }
        Err(_) => {
            log::warn!("googlevideo range probe failed");
            false
        }
    }
}
