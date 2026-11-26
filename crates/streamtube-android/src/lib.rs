//! Stream Tube — Android shell (plan2.md Tracks B2/B3/B5).
//!
//! Boots GPUI through [gpui-mobile](https://github.com/itsbalamurali/gpui-mobile)
//! (pinned rev `635b7e7`, six features only), points the streamtube-core store
//! at the app-private `filesDir` **before** `store::init`, and renders a real
//! Home feed plus simple search — all networking goes through streamtube-core,
//! exactly like the desktop. The desktop entry point is untouched: on every
//! other target this crate compiles to nothing.
//!
//! Build (from the repo root):
//!
//! ```text
//! cargo ndk -t arm64-v8a -t x86_64 -P 26 \
//!     -o android/app/src/main/jniLibs build -p streamtube-android
//! ```
//!
//! then `android/gradlew assembleDebug`.

#![cfg(target_os = "android")]

// Pull gpui-mobile's JNI bootstrap symbols (ANativeActivity_onCreate etc.)
// into this .so.
extern crate gpui_mobile;

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::Root;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::prelude::*;
use gpui_kit::*;
use streamtube_core::api::innertube::{self, SearchFilter};
use streamtube_core::api::types::{Item, VideoItem};
use streamtube_core::store;

// ───────────────────────── Android entry point ─────────────────────────

/// Called by `android-activity` on a dedicated native thread after
/// `GpuiActivity` (→ `GpuiInputActivity` → `NativeActivity`) loads the .so.
#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("streamtube"),
    );
    gpui_mobile::android::jni::install_panic_hook();
    log::info!("android_main: entered");

    // The store must live in the app-private `context.filesDir`, which
    // android-activity exposes as `internal_data_path`. Set it (plus the
    // STREAMTUBE_DATA_DIR fallback every crate in the workspace honours)
    // before any store use.
    let files_dir: PathBuf = app
        .internal_data_path()
        .map(|p| p.to_path_buf())
        .expect("AndroidApp::internal_data_path() — the activity must provide filesDir");
    // SAFETY: set before any other thread starts, like the desktop's main().
    unsafe { std::env::set_var("STREAMTUBE_DATA_DIR", &files_dir) };
    if !store::set_data_dir(&files_dir) {
        log::warn!("store::set_data_dir rejected — the store was initialised elsewhere");
    }
    store::init();
    log::info!("store initialised at {}", files_dir.display());

    let _platform = gpui_mobile::android::jni::init_platform(&app);
    log::info!("android_main: platform initialised");
    let shared = match gpui_mobile::android::jni::shared_platform() {
        Some(s) => s,
        None => {
            log::error!("android_main: shared_platform() returned None — aborting");
            return;
        }
    };

    Application::with_platform(shared.into_rc())
        .with_http_client(UreqHttp::new())
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
            match cx.open_window(
                WindowOptions { window_bounds: None, ..Default::default() },
                |window, cx| {
                    let shell = cx.new(|cx| Shell::new(window, cx));
                    cx.new(|cx| Root::new(shell, window, cx))
                },
            ) {
                Ok(_) => log::info!("window opened — Home is loading"),
                Err(e) => log::error!("open_window failed: {e:#}"),
            }
            cx.activate(true);
        });
    log::info!("android_main: run loop exited");
}

// ───────────── HTTP client for gpui img() (same as the desktop) ─────────────

/// The desktop ships this in `src/ui/mod.rs`; the Android shell needs the same
/// bridge so `img()` thumbnails go through streamtube-core's ureq stack
/// (browser UA, settings proxy) instead of an extra reqwest tree.
pub struct UreqHttp {
    ua: http_client::http::HeaderValue,
}

impl UreqHttp {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { ua: http_client::http::HeaderValue::from_static(streamtube_core::api::http::BROWSER_UA) })
    }
}

impl http_client::HttpClient for UreqHttp {
    fn user_agent(&self) -> Option<&http_client::http::HeaderValue> {
        Some(&self.ua)
    }

    fn proxy(&self) -> Option<&http_client::Url> {
        None
    }

    fn send(
        &self,
        req: http_client::Request<http_client::AsyncBody>,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<http_client::Response<http_client::AsyncBody>>> {
        let uri = req.uri().to_string();
        Box::pin(async move {
            let bytes = blocking(move || streamtube_core::api::http::get_bytes(&uri, &[])).await?;
            Ok(http_client::Response::builder()
                .status(200)
                .body(http_client::AsyncBody::from(bytes))?)
        })
    }
}

// ───────────────────── background work (same as the desktop) ─────────────────────

/// Runs blocking work on a dedicated OS thread and returns a future for its
/// result (`src/ui/mod.rs` on the desktop).
pub fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> impl std::future::Future<Output = T> {
    let (tx, rx) = futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    async move { rx.await.expect("worker thread panicked") }
}

// ───────────────────────────── palette ─────────────────────────────

const BG: u32 = 0x0f0f0f;
const SURFACE: u32 = 0x212121;
const BORDER: u32 = 0x303030;
const TEXT: u32 = 0xf1f1f1;
const TEXT2: u32 = 0xaaaaaa;
const ACCENT: u32 = 0xff0033; // the store's default accent

// ───────────────────────────── the shell ─────────────────────────────

enum Load<T> {
    Loading,
    Ready(T),
    Error(String),
}

#[derive(Clone)]
enum Mode {
    Home,
    Search(String),
}

struct Shell {
    search: Entity<InputState>,
    mode: Mode,
    items: Load<Arc<Vec<Item>>>,
    continuation: Option<String>,
    serial: u64,
}

impl Shell {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search YouTube"));
        cx.subscribe_in(&search, window, |this, state, ev: &InputEvent, _window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                let q = state.read(cx).value().trim().to_string();
                this.search_for(&q, cx);
            }
        })
        .detach();
        let mut shell = Self { search, mode: Mode::Home, items: Load::Loading, continuation: None, serial: 0 };
        // A `streamtube://search?q=…` deep link opens straight into search
        // results (the desktop's streamtube: scheme; more routes land with
        // Track F).
        if let Ok(Some(url)) = gpui_mobile::packages::deeplink::get_initial_link() {
            log::info!("deeplink: {url}");
            if let Some(q) = url
                .strip_prefix("streamtube://search")
                .and_then(|rest| rest.strip_prefix("?q=").or_else(|| rest.strip_prefix("?query=")))
            {
                let q = streamtube_core::api::http::urldecode(q);
                shell.search_for(&q, cx);
                return shell;
            }
        }
        shell.load_home(cx);
        shell
    }

    /// The unsigned home feed, exactly like the desktop's no-sign-in Home:
    /// a few Explore categories fetched in parallel, interleaved round-robin,
    /// de-duplicated.
    fn load_home(&mut self, cx: &mut Context<Self>) {
        self.mode = Mode::Home;
        self.items = Load::Loading;
        self.continuation = None;
        self.serial += 1;
        let serial = self.serial;
        cx.spawn(async move |this, cx| {
            let res = blocking(move || -> anyhow::Result<Vec<Item>> {
                let lists: Vec<Vec<Item>> = std::thread::scope(|s| {
                    let hs: Vec<_> = innertube::EXPLORE
                        .iter()
                        .take(4)
                        .map(|(_, id)| s.spawn(move || innertube::explore(id).map(|p| p.items).unwrap_or_default()))
                        .collect();
                    hs.into_iter().map(|h| h.join().unwrap_or_default()).collect()
                });
                let mut iters: Vec<_> = lists.into_iter().map(|l| l.into_iter()).collect();
                let mut out: Vec<Item> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                loop {
                    let mut any = false;
                    for it in iters.iter_mut() {
                        if let Some(x) = it.next() {
                            any = true;
                            if let Item::Video(v) = &x {
                                if !seen.insert(v.id.clone()) {
                                    continue;
                                }
                            }
                            out.push(x);
                        }
                    }
                    if !any {
                        break;
                    }
                }
                Ok(out)
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                if this.serial != serial {
                    return;
                }
                this.items = match res {
                    Ok(items) if items.is_empty() => {
                        Load::Error("The home feed came back empty. Check the network and retry.".into())
                    }
                    Ok(items) => Load::Ready(Arc::new(items)),
                    Err(e) => Load::Error(e.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn search_for(&mut self, q: &str, cx: &mut Context<Self>) {
        log::info!("search_for: {q:?}");
        if q.is_empty() {
            self.load_home(cx);
            return;
        }
        self.mode = Mode::Search(q.to_string());
        self.items = Load::Loading;
        self.continuation = None;
        self.serial += 1;
        let serial = self.serial;
        let (q, f) = (q.to_string(), SearchFilter::default());
        let q_err = q.clone();
        cx.spawn(async move |this, cx| {
            let res = blocking(move || innertube::search(&q, &f).map(|p| (p.items, p.continuation))).await;
            let _ = this.update(cx, |this, cx| {
                if this.serial != serial {
                    return;
                }
                match res {
                    Ok((items, cont)) => {
                        this.items = if items.is_empty() {
                            Load::Error(format!("No results for \u{201c}{q_err}\u{201d}"))
                        } else {
                            Load::Ready(Arc::new(items))
                        };
                        this.continuation = cont;
                    }
                    Err(e) => this.items = Load::Error(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Retry whatever the current view was trying to load.
    fn reload(&mut self, cx: &mut Context<Self>) {
        match self.mode.clone() {
            Mode::Home => self.load_home(cx),
            Mode::Search(q) => self.search_for(&q, cx),
        }
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match &self.mode {
            Mode::Home => "Home".to_string(),
            Mode::Search(q) => format!("Search: {q}"),
        };
        let body: AnyElement = match &self.items {
            Load::Loading => div()
                .flex()
                .w_full()
                .justify_center()
                .py_8()
                .text_color(rgb(TEXT2))
                .child("Loading…")
                .into_any_element(),
            Load::Error(e) => div()
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .py_8()
                .child(div().text_color(rgb(TEXT2)).text_size(px(13.)).child(e.clone()))
                .child(
                    div()
                        .id("retry")
                        .flex_none()
                        .px_3()
                        .py_1p5()
                        .rounded_lg()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .bg(rgb(ACCENT))
                        // gpui-pre synthesizes a ClickEvent::Touch from a tap
                        // gesture (interactive.rs) — on_click works on Android,
                        // on_mouse_down does not (it only sees real mouse input).
                        .on_click(cx.listener(|this, _, _, cx| this.reload(cx)))
                        .child("Retry"),
                )
                .into_any_element(),
            Load::Ready(items) => div()
                .flex_1()
                .overflow_y_scrollbar()
                .child(div().flex().flex_col().children(items.iter().map(item_row)))
                .into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(
                // search bar
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(div().flex_1().child(Input::new(&self.search).cleanable(true)))
                    .child(
                        div()
                            .id("search-btn")
                            .flex_none()
                            .px_3()
                            .py_1p5()
                            .rounded_lg()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .cursor_pointer()
                            .bg(rgb(ACCENT))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let q = this.search.read(cx).value().trim().to_string();
                                log::info!("search-btn pressed, query: {q:?}");
                                this.search_for(&q, cx);
                            }))
                            .child("Search"),
                    ),
            )
            .child(
                // section header
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .child(div().text_sm().font_weight(FontWeight::BOLD).child(title))
                    .when(matches!(self.mode, Mode::Search(_)), |d| {
                        d.child(
                            div()
                                .id("back-home")
                                .flex_none()
                                .px_2()
                                .py_1()
                                .rounded_lg()
                                .cursor_pointer()
                                .text_sm()
                                .text_color(rgb(TEXT2))
                                .on_click(cx.listener(|this, _, _, cx| this.load_home(cx)))
                                .child("Back to Home"),
                        )
                    }),
            )
            .child(body)
    }
}

// ───────────────────────────── list rows ─────────────────────────────

fn item_row(item: &Item) -> Div {
    match item {
        Item::Video(v) => video_row(v),
        Item::Channel(c) => div()
            .flex()
            .items_center()
            .gap_3()
            .p_2()
            .child(
                match &c.avatar {
                    Some(u) => img(u.clone()).size_10().flex_none().rounded_full().into_any_element(),
                    None => div().size_10().flex_none().rounded_full().bg(rgb(SURFACE)).into_any_element(),
                },
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_size(px(14.)).child(c.name.clone()))
                    .when(
                        !c.subscribers_text.as_deref().unwrap_or("").is_empty(),
                        |d| {
                            d.child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(TEXT2))
                                    .child(c.subscribers_text.clone().unwrap_or_default()),
                            )
                        },
                    ),
            ),
        Item::Playlist(p) => div()
            .flex()
            .gap_3()
            .p_2()
            .child(
                match &p.thumbnail {
                    Some(u) => img(u.clone()).w(px(120.)).h(px(68.)).flex_none().rounded_md().into_any_element(),
                    None => div().w(px(120.)).h(px(68.)).flex_none().rounded_md().bg(rgb(SURFACE)).into_any_element(),
                },
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_size(px(14.)).child(p.title.clone()))
                    .when(!p.video_count_text.as_deref().unwrap_or("").is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(TEXT2))
                                .child(p.video_count_text.clone().unwrap_or_default()),
                        )
                    }),
            ),
    }
}

fn video_row(v: &VideoItem) -> Div {
    let meta = [
        v.views_text.as_deref().unwrap_or(""),
        v.published_text.as_deref().unwrap_or(""),
        v.duration_text.as_deref().unwrap_or(""),
    ]
    .iter()
    .filter(|s| !s.is_empty())
    .cloned()
    .collect::<Vec<_>>()
    .join("  ·  ");
    div()
        .flex()
        .gap_3()
        .p_2()
        .child(
            img(v.thumbnail.clone())
                .w(px(140.))
                .h(px(79.))
                .flex_none()
                .rounded_md()
                .into_any_element(),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .min_w_0()
                .child(div().text_size(px(14.)).child(v.title.clone()))
                .child(div().text_size(px(12.)).text_color(rgb(TEXT2)).child(v.channel_name.clone()))
                .when(!meta.is_empty(), |d| {
                    d.child(div().text_size(px(12.)).text_color(rgb(TEXT2)).child(meta))
                }),
        )
}
