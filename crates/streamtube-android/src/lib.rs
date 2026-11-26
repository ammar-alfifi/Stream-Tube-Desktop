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

mod player;

use std::mem;
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::Root;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::prelude::*;
use gpui_kit::*;
use gpui_kit::{ImgResourceLoader, Resource};
use streamtube_core::api::innertube::{self, SearchFilter};
use streamtube_core::api::types::{Item, VideoItem};
use streamtube_core::i18n;
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
    // Track C3: FFmpeg's MediaCodec helpers need the JavaVM, and the
    // "mpv_surface" platform view type must exist before a player opens.
    // FFmpeg itself is loaded lazily by player::try_start, not at app startup.
    player::register_platform_view();
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
            if i18n::language().is_rtl() {
                gpui_kit::base::LayoutDirection::Rtl.set_global(cx);
            } else {
                gpui_kit::base::LayoutDirection::Ltr.set_global(cx);
            }
            gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
            match cx.open_window(
                WindowOptions {
                    window_bounds: None,
                    ..Default::default()
                },
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
        Arc::new(Self {
            ua: http_client::http::HeaderValue::from_static(streamtube_core::api::http::BROWSER_UA),
        })
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
    ) -> futures::future::BoxFuture<
        'static,
        anyhow::Result<http_client::Response<http_client::AsyncBody>>,
    > {
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

// Material 3 dark color roles (baseline purple scheme).
pub(crate) const BG: u32 = 0x131318;
pub(crate) const SURFACE: u32 = 0x1d1b20;
pub(crate) const SURFACE_HIGH: u32 = 0x29272f;
pub(crate) const SURFACE_VARIANT: u32 = 0x49454f;
pub(crate) const TEXT: u32 = 0xe6e1e5;
pub(crate) const TEXT2: u32 = 0xcac4d0;
pub(crate) const ACCENT: u32 = 0xd0bcff;
pub(crate) const ON_ACCENT: u32 = 0x381e72;
pub(crate) const PRIMARY_CONTAINER: u32 = 0x4f378b;
pub(crate) const ON_PRIMARY_CONTAINER: u32 = 0xeadcff;
pub(crate) const ERROR: u32 = 0xf2b8b5;
const MAX_FEED_ITEMS: usize = 18;

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
    image_urls: Vec<String>,
    /// Open player (Track C3); the feed stays cached underneath for instant back.
    player: Option<Entity<player::Player>>,
}

impl Shell {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search YouTube"));
        cx.subscribe_in(
            &search,
            window,
            |this, state, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    let q = state.read(cx).value().trim().to_string();
                    this.search_for(&q, window, cx);
                }
            },
        )
        .detach();
        let mut shell = Self {
            search,
            mode: Mode::Home,
            items: Load::Loading,
            continuation: None,
            serial: 0,
            image_urls: Vec::new(),
            player: None,
        };
        // A `streamtube://search?q=…` deep link opens straight into search
        // results (the desktop's streamtube: scheme; more routes land with
        // Track F).
        if let Ok(Some(url)) = gpui_mobile::packages::deeplink::get_initial_link() {
            log::info!("deeplink: {url}");
            if let Some(q) = url.strip_prefix("streamtube://search").and_then(|rest| {
                rest.strip_prefix("?q=")
                    .or_else(|| rest.strip_prefix("?query="))
            }) {
                let q = streamtube_core::api::http::urldecode(q);
                shell.search_for(&q, window, cx);
                return shell;
            }
        }
        shell.load_home(window, cx);
        shell
    }

    fn item_image_urls(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .filter_map(|item| match item {
                Item::Video(v) => Some(v.thumbnail.clone()),
                Item::Channel(c) => c.avatar.clone(),
                Item::Playlist(p) => p.thumbnail.clone(),
            })
            .filter(|url| !url.is_empty())
            .collect()
    }

    fn evict_image(&mut self, cx: &mut App, url: String, window: Option<&mut Window>) {
        let resource = Resource::Uri(url.as_str().into());
        // Drop the decoded GPU texture as well as the cached load. Removing
        // only the asset entry leaves the painted image in the window atlas.
        if let Some(Ok(image)) = cx.fetch_asset::<ImgResourceLoader>(&resource) {
            cx.drop_image(image, window);
        }
        cx.remove_asset::<ImgResourceLoader>(&resource);
    }

    fn clear_images(&mut self, cx: &mut App, mut window: Option<&mut Window>) {
        for url in mem::take(&mut self.image_urls) {
            self.evict_image(cx, url, window.as_deref_mut());
        }
    }

    fn replace_items(&mut self, items: Arc<Vec<Item>>, cx: &mut App) {
        let new_urls = Self::item_image_urls(&items);
        for url in mem::take(&mut self.image_urls) {
            if !new_urls.contains(&url) {
                self.evict_image(cx, url, None);
            }
        }
        self.image_urls = new_urls;
        self.items = Load::Ready(items);
    }

    /// The unsigned home feed, exactly like the desktop's no-sign-in Home:
    /// a few Explore categories fetched in parallel, interleaved round-robin,
    /// de-duplicated.
    fn load_home(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_images(cx, Some(window));
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
                        .map(|(_, id)| {
                            s.spawn(move || {
                                innertube::explore(id).map(|p| p.items).unwrap_or_default()
                            })
                        })
                        .collect();
                    hs.into_iter()
                        .map(|h| h.join().unwrap_or_default())
                        .collect()
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
                            if out.len() >= MAX_FEED_ITEMS {
                                break;
                            }
                        }
                    }
                    if out.len() >= MAX_FEED_ITEMS || !any {
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
                        this.clear_images(cx, None);
                        Load::Error(
                            "The home feed came back empty. Check the network and retry.".into(),
                        )
                    }
                    Ok(items) => {
                        this.replace_items(Arc::new(items), cx);
                        cx.notify();
                        return;
                    }
                    Err(e) => {
                        this.clear_images(cx, None);
                        Load::Error(e.to_string())
                    }
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn search_for(&mut self, q: &str, window: &mut Window, cx: &mut Context<Self>) {
        log::info!("search_for: {q:?}");
        if q.is_empty() {
            self.load_home(window, cx);
            return;
        }
        self.clear_images(cx, Some(window));
        self.mode = Mode::Search(q.to_string());
        self.items = Load::Loading;
        self.continuation = None;
        self.serial += 1;
        let serial = self.serial;
        let (q, f) = (q.to_string(), SearchFilter::default());
        let q_err = q.clone();
        cx.spawn(async move |this, cx| {
            let res =
                blocking(move || innertube::search(&q, &f).map(|p| (p.items, p.continuation)))
                    .await;
            let _ = this.update(cx, |this, cx| {
                if this.serial != serial {
                    return;
                }
                match res {
                    Ok((mut items, cont)) => {
                        items.truncate(MAX_FEED_ITEMS);
                        this.continuation = cont;
                        if items.is_empty() {
                            this.clear_images(cx, None);
                            this.items =
                                Load::Error(format!("No results for \u{201c}{q_err}\u{201d}"));
                        } else {
                            this.replace_items(Arc::new(items), cx);
                        }
                    }
                    Err(e) => {
                        this.clear_images(cx, None);
                        this.items = Load::Error(e.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.search.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    }

    /// Retry whatever the current view was trying to load.
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.mode.clone() {
            Mode::Home => self.load_home(window, cx),
            Mode::Search(q) => self.search_for(&q, window, cx),
        }
    }

    /// Opens the Track C3 player for a video tapped in Home/search.
    fn open_video(&mut self, v: VideoItem, window: &mut Window, cx: &mut Context<Self>) {
        if self.player.is_some() {
            return;
        }
        log::info!("open_video: {} ({})", v.title, v.id);
        self.clear_images(cx, Some(window));
        let player = cx.new(|cx| player::Player::new(v, cx));
        cx.subscribe_in(
            &player,
            window,
            |this, _p, ev: &player::PlayerEvent, _w, cx| {
                if matches!(ev, player::PlayerEvent::Back) {
                    this.player = None; // Player::drop tears mpv and the surface view down
                    if let Load::Ready(items) = &this.items {
                        // The feed was cached while its textures were evicted for
                        // playback. Re-track the thumbnails that render again now.
                        this.image_urls = Self::item_image_urls(items);
                    }
                    cx.notify();
                }
            },
        )
        .detach();
        self.player = Some(player);
        cx.notify();
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Track C3: an open player takes over the window; the feed stays
        // cached underneath, so Back is instant.
        if let Some(player) = self.player.clone() {
            return div().size_full().child(player);
        }
        let (title, subtitle) = match &self.mode {
            Mode::Home => ("Home".to_string(), "Recommended for you".to_string()),
            Mode::Search(q) => ("Search results".to_string(), q.clone()),
        };
        let searching = matches!(self.mode, Mode::Search(_));
        let body: AnyElement = match &self.items {
            Load::Loading => loading_state().into_any_element(),
            Load::Error(e) => error_state(e, cx).into_any_element(),
            Load::Ready(items) => div()
                .flex_1()
                .overflow_y_scrollbar()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .p_4()
                        .children(items.iter().map(|item| item_row(item, cx))),
                )
                .into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .px_4()
                    .pt_3()
                    .pb_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .font_weight(FontWeight::BOLD)
                                            .child("Stream Tube"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(rgb(TEXT2))
                                            .child("Open source video player"),
                                    ),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_full()
                                    .bg(rgb(PRIMARY_CONTAINER))
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(ON_PRIMARY_CONTAINER))
                                    .child("BETA"),
                            ),
                    )
                    .child(
                        div()
                            .h(px(48.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .pl_4()
                            .rounded_full()
                            .bg(rgb(SURFACE_HIGH))
                            .border_1()
                            .border_color(rgb(SURFACE_VARIANT))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&self.search).cleanable(true)),
                            )
                            .child(
                                div()
                                    .id("search-btn")
                                    .flex_none()
                                    .px_4()
                                    .py_2()
                                    .rounded_full()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .cursor_pointer()
                                    .bg(rgb(ACCENT))
                                    .text_color(rgb(ON_ACCENT))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let q = this.search.read(cx).value().trim().to_string();
                                        this.search_for(&q, window, cx);
                                    }))
                                    .child("Go"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_4()
                    .pb_2()
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::BOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(TEXT2))
                            .child(subtitle),
                    ),
            )
            .child(body)
            .child(bottom_nav(searching, cx))
    }
}

// ───────────────────────────── Material 3 states ─────────────────────────────

fn loading_state() -> impl IntoElement {
    div().flex_1().min_h_0().overflow_y_scrollbar().child(
        div()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .children((0..4).map(|_| {
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(SURFACE))
                    .child(
                        div()
                            .w_full()
                            .aspect_ratio(16. / 9.)
                            .rounded_md()
                            .bg(rgb(SURFACE_HIGH)),
                    )
                    .child(
                        div()
                            .w(relative(0.82))
                            .h(px(14.))
                            .rounded_full()
                            .bg(rgb(SURFACE_HIGH)),
                    )
                    .child(
                        div()
                            .w(relative(0.48))
                            .h(px(10.))
                            .rounded_full()
                            .bg(rgb(SURFACE_VARIANT)),
                    )
            })),
    )
}

fn error_state(message: &str, cx: &mut Context<Shell>) -> Div {
    div()
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .p_4()
        .gap_3()
        .child(
            div().flex_1().min_h_0().overflow_y_scrollbar().child(
                div()
                    .min_h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_2()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(420.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_3()
                            .p_6()
                            .rounded_md()
                            .bg(rgb(SURFACE))
                            .border_1()
                            .border_color(rgb(SURFACE_VARIANT))
                            .child(
                                div()
                                    .size(px(40.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap_1()
                                    .rounded_full()
                                    .bg(rgb(ERROR))
                                    .text_color(rgb(ON_ACCENT))
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::BOLD)
                                    .child("!"),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(TEXT))
                                    .text_center()
                                    .child("Something went wrong"),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(rgb(TEXT2))
                                    .text_center()
                                    .child(message.to_string()),
                            ),
                    ),
            ),
        )
        .child(
            div()
                .id("retry")
                .flex_none()
                .w_full()
                .max_w(px(420.))
                .mx_auto()
                .h(px(48.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(rgb(ACCENT))
                .text_color(rgb(ON_ACCENT))
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| this.reload(window, cx)))
                .child("Retry"),
        )
}

fn bottom_nav(searching: bool, cx: &mut Context<Shell>) -> Div {
    let home_active = !searching;
    div()
        .h(px(64.))
        .flex_none()
        .flex()
        .border_t_1()
        .border_color(rgb(SURFACE_VARIANT))
        .bg(rgb(SURFACE))
        .child(
            div()
                .id("nav-home")
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| this.load_home(window, cx)))
                .child(
                    div()
                        .h(px(40.))
                        .px_5()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap_1()
                        .rounded_full()
                        .bg(if home_active {
                            rgb(PRIMARY_CONTAINER)
                        } else {
                            rgb(SURFACE)
                        })
                        .text_color(if home_active {
                            rgb(ON_PRIMARY_CONTAINER)
                        } else {
                            rgb(TEXT2)
                        })
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child("Home"),
                ),
        )
        .child(
            div()
                .id("nav-search")
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| this.focus_search(window, cx)))
                .child(
                    div()
                        .h(px(40.))
                        .px_5()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap_1()
                        .rounded_full()
                        .bg(if searching {
                            rgb(PRIMARY_CONTAINER)
                        } else {
                            rgb(SURFACE)
                        })
                        .text_color(if searching {
                            rgb(ON_PRIMARY_CONTAINER)
                        } else {
                            rgb(TEXT2)
                        })
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child("Search"),
                ),
        )
}

// ───────────────────────────── list rows ─────────────────────────────

fn item_row(item: &Item, cx: &mut Context<Shell>) -> AnyElement {
    match item {
        Item::Video(v) => video_row(v, cx).into_any_element(),
        Item::Channel(c) => div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .p_4()
            .rounded_md()
            .bg(rgb(SURFACE))
            .border_1()
            .border_color(rgb(SURFACE_VARIANT))
            .child(match &c.avatar {
                Some(u) => img(u.clone())
                    .size_10()
                    .flex_none()
                    .rounded_full()
                    .into_any_element(),
                None => div()
                    .size_10()
                    .flex_none()
                    .rounded_full()
                    .bg(rgb(SURFACE))
                    .into_any_element(),
            })
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
            )
            .into_any_element(),
        Item::Playlist(p) => div()
            .w_full()
            .flex()
            .gap_3()
            .p_4()
            .rounded_md()
            .bg(rgb(SURFACE))
            .border_1()
            .border_color(rgb(SURFACE_VARIANT))
            .child(match &p.thumbnail {
                Some(u) => img(u.clone())
                    .w(px(120.))
                    .h(px(68.))
                    .flex_none()
                    .rounded_md()
                    .into_any_element(),
                None => div()
                    .w(px(120.))
                    .h(px(68.))
                    .flex_none()
                    .rounded_md()
                    .bg(rgb(SURFACE))
                    .into_any_element(),
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_size(px(14.)).child(p.title.clone()))
                    .when(
                        !p.video_count_text.as_deref().unwrap_or("").is_empty(),
                        |d| {
                            d.child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(TEXT2))
                                    .child(p.video_count_text.clone().unwrap_or_default()),
                            )
                        },
                    ),
            )
            .into_any_element(),
    }
}

fn video_row(v: &VideoItem, cx: &mut Context<Shell>) -> Stateful<Div> {
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
    let video = v.clone();
    div()
        .w_full()
        .flex()
        .flex_col()
        .rounded_md()
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(SURFACE_VARIANT))
        .overflow_hidden()
        // gpui-pre synthesizes ClickEvent::Touch from taps, so on_click works
        // where on_mouse_down does not.
        .id(v.id.clone())
        .cursor_pointer()
        .on_click(
            cx.listener(move |this, _, window, cx| this.open_video(video.clone(), window, cx)),
        )
        .child(
            img(v.thumbnail.clone())
                .w_full()
                .aspect_ratio(16. / 9.)
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(v.title.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(TEXT2))
                        .child(v.channel_name.clone()),
                )
                .when(!meta.is_empty(), |d| {
                    d.child(div().text_size(px(11.)).text_color(rgb(TEXT2)).child(meta))
                }),
        )
}
