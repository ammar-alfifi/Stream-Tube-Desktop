# B1 — gpui-mobile compatibility reconnaissance

Date: 2026-11-26
Subject repo: https://github.com/itsbalamurali/gpui-mobile
Pinned HEAD: `635b7e7743b756a1cbe006ee239cc8f3762dc3d9` ("Bump gpui-pre from 0.3.6 to 0.3.12 (#73)", 2026-11-26) — see `gpui-mobile-head.txt`.
Clone location (scratch): `/home/ammar/.hermes/cache/scratch/gpui-mobile`
Integration probe (scratch): `/home/ammar/.hermes/cache/scratch/b1-integration`

## Verdict

**Compatible. No deep fork required. DR-1 not triggered.**
gpui-mobile HEAD targets the exact same snapshot our app uses (`gpui-pre =0.3.12`, `gpui-pre-wgpu =0.3.12`), and gpui-kit 0.10 pins `gpui-pre "=0.3.12"` as well, so the whole graph unifies to a single `gpui-pre 0.3.12`. The only compile delta on the aarch64-linux-android target is one line in our desktop entry point (`gpui_kit::application()` in `src/main.rs:86`), which gpui-kit itself deliberately cfg-gates out on android/ios — leaving room, by design, for gpui-mobile's platform bootstrap.

## 1. Version pins (upstream facts)

`gpui-mobile/Cargo.toml` (HEAD):

```toml
[dependencies]
gpui = { package = "gpui-pre", version = "=0.3.12", default-features = false }

[target.'cfg(target_os = "ios")'.dependencies]
gpui_wgpu = { package = "gpui-pre-wgpu", version = "=0.3.12", features = ["font-kit"] }
wgpu = "29.0.4"          # iOS-only direct pin

[target.'cfg(target_os = "android")'.dependencies]
gpui_wgpu = { package = "gpui-pre-wgpu", version = "=0.3.12" }   # NO direct wgpu pin on Android
```

`gpui-kit 0.10.0` (registry manifest, same file our lock resolves):

```toml
[dependencies.gpui]
version = "=0.3.12"
package = "gpui-pre"
```

`gpui-pre-wgpu 0.3.12` (registry manifest) requires wgpu `^30.0.1` on every non-wasm target
(`linux/freebsd`: `30.0.1` + features `std,metal,gles,vulkan,wgsl,webgpu`, default-features=false;
all other targets incl. android/ios: `30.0.1`, default-features=true).

## 2. Dependency graph per target (cargo tree, resolved)

Our project, host (linux):

```
$ cargo tree -e normal -i wgpu
wgpu v30.0.2
└── gpui-pre-wgpu v0.3.12
    └── gpui-pre-linux v0.3.12
        └── gpui-pre-platform v0.3.12
            └── gpui-kit v0.10.0
                └── streamtube v0.7.5-alpha
```

Our project, `--target aarch64-linux-android`: `gpui-pre-platform` / `gpui-pre-web` are NOT in
the android graph at all. gpui-kit's android subtree is pure UI:

```
$ cargo tree --target aarch64-linux-android -e normal -p gpui-kit --depth 1
gpui-kit v0.10.0
├── gpui-base v0.10.0
├── gpui-component v0.10.0
├── gpui-kit-assets v0.10.0
├── gpui-pre v0.3.12
└── gpui-pre-macros v0.3.12
```

→ On Android, nothing supplies a `Platform` implementation; gpui-mobile exists precisely to fill
that hole (implements `gpui::Platform` against gpui-pre 0.3.12, renderer `gpui-pre-wgpu`
`WgpuRenderer` Vulkan, text `CosmicTextSystem` cosmic-text+swash).

Integration probe (gpui-kit 0.10 default + gpui-mobile, B2 feature set), android target:

```
$ cargo tree --target aarch64-linux-android -i wgpu@30.0.2
wgpu v30.0.2
└── gpui-pre-wgpu v0.3.12
    └── gpui-mobile v0.1.0
        └── b1-integration-probe v0.0.0

$ cargo tree --target aarch64-linux-android -i wgpu@29.0.4
warning: nothing to print.   # wgpu 29 is NOT in the android graph

$ cargo tree --target all -i wgpu@29.0.4
wgpu v29.0.4
└── gpui-mobile v0.1.0       # reached only via the iOS-target pin
```

Probe `Cargo.lock`: exactly ONE `gpui-pre 0.3.12` and ONE `gpui-pre-wgpu 0.3.12`
(no split), plus TWO wgpu majors (29.0.4 iOS-only, 30.0.2 — the one we already use).
Resolution succeeds even with `--target all`; nothing to reconcile for linux/android builds.

**Correction to plan2.md wording:** "gpui-mobile pins wgpu 29" is true only for its iOS
target; the Android build uses wgpu **30.0.2 via gpui-pre-wgpu**, identical to our desktop lock.

## 3. Compile evidence

### 3a. Whole workspace, aarch64-linux-android

```
$ export CC_aarch64_linux_android=~/Android/Sdk/ndk/27.2.12479018/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android26-clang
$ export AR_aarch64_linux_android=…/llvm-ar
$ cargo check --target aarch64-linux-android --locked --workspace
```

Result (full log: `cargo-check-workspace-android.log`):

- `streamtube-core` (incl. ureq→rustls→ring with NDK CC env): **checked OK**
- `gpui-kit 0.10.0` + `gpui-pre 0.3.12` + `gpui-base`/`gpui-component`/`gpui-kit-assets`: **checked OK**
- the whole `streamtube` bin crate type-checked except ONE error:

```
error[E0425]: cannot find function `application` in crate `gpui_kit`
   --> src/main.rs:86:15
    gpui_kit::application()
   --> ~/.cargo/registry/.../gpui-kit-0.3.12…/gpui-kit-0.10.0/src/lib.rs:200:19
199 | #[cfg(not(any(target_os = "ios", target_os = "android")))]
200 | pub use platform::application;
```

i.e. the desktop platform bootstrap is intentionally unavailable on android — exactly the seam
gpui-mobile's `android_main` bootstrap replaces. 2 warnings only (unused vars, `src/ui/app_icon.rs`).

Note (environment): the first unconfigured run failed only at `ring v0.17.14`'s build script
("failed to find tool aarch64-linux-android-clang"). Fix = NDK clang via `CC_aarch64_linux_android`
(above), or just `cargo ndk`, which injects it for real builds (B3).

### 3b. Integration probe, aarch64-linux-android

Probe manifest:

```toml
[dependencies]
gpui-kit = "0.10"
gpui-mobile = { path = "/home/ammar/.hermes/cache/scratch/gpui-mobile", default-features = false,
                features = ["video_player", "media_session", "deeplink", "file_selector", "clipboard", "notifications"] }
```

```
$ cargo check --target aarch64-linux-android
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 23s
EXIT=0
```

Full log: `cargo-check-integration-probe-android.log` (439 crates checked, 2 warnings).
This compiles gpui-kit and gpui-mobile against the SAME unified `gpui-pre 0.3.12` with the
union of our features — the co-existence proof at compile level.

### 3c. Upstream CI checks the same combo

`.github/workflows/ci.yml` (HEAD) runs `cargo check --target aarch64-linux-android` for the
crate AND for `example/`, whose `Cargo.toml` is itself a gpui-kit app
(`gpui-kit = { version = "0.9.1", default-features = false, features = ["component"] }`
+ `gpui-pre =0.3.12` + `gpui-mobile`). The 0.3.12 bump landed at HEAD and is what CI exercises.

## 4. Features (B2 exact set)

All 30 features are empty (`url_launcher = []`, …) — they are `cfg` module gates, not extra
Rust dependencies. `src/packages/mod.rs` gates each module with `#[cfg(feature = "…")]`.
So `default-features = false` + the 6 wanted features adds zero packages; it only trims modules
and (in the Gradle shell) the Java helper classes that must be packaged.

Requested set — all present and JNI-implemented:

| feature | Rust module | Java helper | status |
|---|---|---|---|
| video_player | packages/video_player (Android MediaPlayer) | GpuiVideoPlayer.java | implemented — reference only; main video is mpv via platform-view (Track C) |
| media_session | packages/media_session | GpuiMediaSession.java (MediaSessionCompat) | implemented |
| deeplink | packages/deeplink | GpuiDeepLink.java | implemented (`get_initial_link`/`get_latest_link`; TODO.md's "Deeplinking?" is stale) |
| file_selector | packages/file_selector + GpuiPathPicker activity | GpuiPathPicker.java, GpuiFilePicker.java | implemented; path prompts via `Platform` also wired |
| clipboard | packages/clipboard | GpuiClipboard.java | implemented via JNI — NOTE: platform-level clipboard inside AndroidPlatform is an in-process stub, so UI must call `gpui_mobile::packages::clipboard::{set_text,get_text}` rather than `cx.write_to_clipboard` (candidate for a tiny local patch later, not a fork) |
| notifications | packages/notifications | GpuiNotifications.java (NotificationCompat) | implemented |

Also available later: `platform_view` module (not a feature, always compiled) — Flutter-style
hybrid composition registry to embed native views (SurfaceView/TextureView) in the GPUI tree —
the intended channel for mpv's `mediacodec_embed` surface (Track C3/C5).

## 5. Shell (Gradle) without forking gpui-mobile

Facts from `example/android/gradle/` (AGP `com.android.tools.build:gradle:9.1.0`,
Kotlin plugin 2.4.21, wrapper `gradle-9.9.2`):

- `minSdk = 26`, `targetSdk = 34`, `compileSdk = 34`, `abiFilters = ["arm64-v8a"]`
- NativeActivity lineage: `GpuiActivity extends GpuiInputActivity extends NativeActivity`:
  - `GpuiInputActivity` — UI-thread `InputConnection` proxy for multistage IME composition
    (`gpuiShowKeyboard`, composition/marked-text) — required for Arabic keyboards (B6)
  - `GpuiActivity` — AndroidX SplashScreen held until `NATIVE_INITIALIZED`, deep-link
    `onNewIntent`, volume-key routing, media-button events
- Manifest: `android.app.lib_name` placeholder, `supportsRtl="true"`,
  `windowSoftInputMode="adjustResize"`, `configChanges` covering rotation/density/uiMode,
  Vulkan `required="false"` + `glEsVersion 0x00030000`, optional helper activities
  (`GpuiPathPicker`, `GpuiPickerActivity`, `GpuiPermissionActivity`, `GpuiAuthActivity`)
- Native lib: pre-built via `cargo ndk -t arm64-v8a -o …/jniLibs build` — NO CMake/ndk-build
  (`externalNativeBuild` intentionally empty), `jniLibs.srcDirs("src/main/jniLibs")`,
  `keepDebugSymbols` for the Rust `.so`
- Java deps: `androidx.core:core:1.12.0`, `core-splashscreen:1.0.1`, `biometric:1.1.0`, `media:1.7.1`
- Assets: `assets/fonts/NotoColorEmoji.ttf` (CBDT color emoji fallback for API<33; platform.rs
  prefers system COLRv1 emoji on 33+)

Bootstrap pattern for our `streamtube-android` crate (from `example/src/lib.rs`):

```rust
#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    android_logger::init_once(...);          // logcat
    jni::install_panic_hook();
    let _platform = gpui_mobile::android::jni::init_platform(&app);
    let shared = gpui_mobile::android::jni::shared_platform().unwrap();
    gpui::Application::with_platform(shared.into_rc())   // gpui-pre 0.3.12 app.rs:242
        .with_http_client(ui::UreqHttp::new())           // same HttpClient we already ship
        .run(|cx| { /* open home window */ });
}
```

`Application::with_platform` and `with_http_client` verified present in
`gpui-pre-0.3.12/src/app.rs` (lines 242 / 282). Desktop keeps `gpui_kit::application()`;
android gets the platform explicitly — no source changes in any upstream crate.

Android text stack: platform.rs loads system fonts explicitly (Roboto, NotoSans, …,
**NotoSansArabic-Regular.ttf**, DroidSansMono, CJK…) because cosmic-text's fontdb doesn't scan
Android paths; shaping runs through cosmic-text (rustybuzz) — Arabic shaping/RTL is expected to
work but is a B6 runtime-verification item, not a B1 compile fact.

## 6. License

`gpui-mobile` declares `GPL-3.0-or-later OR AGPL-3.0-or-later OR Apache-2.0`
(LICENSE-GPL / LICENSE-AGPL / LICENSE-APACHE all present at HEAD).
Per plan2.md (Track B2 / DR-6) we adopt it under **Apache-2.0** —GPL obligations don't apply;
still keep the three files in the vendored/submodule copy for provenance.

## 7. Recommended integration mode (no fork)

1. Depend on gpui-mobile pinned to the audited rev:
   `gpui-mobile = { git = "https://github.com/itsbalamurali/gpui-mobile", rev = "635b7e7", default-features = false, features = [ ...6… ] }`
   (Cargo.lock then freezes it). A `third_party/` path copy or submodule is equally fine.
2. Copy `example/android/gradle/` as our `android/` shell template (rename package/namespace,
   set `nativeLibraryName = "streamtube_android"`, trim helper activities to the 6 features).
3. `cargo ndk -t arm64-v8a -P 26 -o android/app/src/main/jniLibs build --release -p streamtube-android`
   then `./gradlew assembleRelease`.
4. Desktop is untouched: same lock keeps `wgpu 30.0.2` on linux+android (probe-proven);
   the extra `wgpu 29.0.4` entry (iOS-only) never compiles on our targets. If a local patch is
   ever needed (e.g. wiring platform clipboard), the repo already has the pattern:
   `vendor/gpui-pre-windows` + `[patch.crates-io]`.
