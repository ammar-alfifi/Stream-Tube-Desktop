# B2/B3/B5 — streamtube-android crate + Gradle shell + real Home

Date: 2026-11-26
Branch: `chore/android-core-split` (working tree, **uncommitted**)
Scope: plan2.md Track B — B2 (adopt gpui-mobile, 6 features), B3 (Gradle shell
+ `streamtube-android` crate), B5 (real Home from the network + simple search).
Desktop untouched: no desktop source modified (only root `Cargo.toml` workspace
members + `Cargo.lock` + `.gitignore` + new `crates/streamtube-android/`,
`android/`, `scripts/fetch-libmpv.sh`).

## Verdict

- **B2 done:** `gpui-mobile` adopted as a git dependency pinned to the audited
  rev, `default-features = false`, exactly the six features
  (`video_player, media_session, deeplink, file_selector, clipboard,
  notifications`). `Cargo.lock` records
  `git+https://github.com/itsbalamurali/gpui-mobile?rev=635b7e7#635b7e7743b756a1cbe006ee239cc8f3762dc3d9`.
- **B3 done:** `crates/streamtube-android` (cdylib `streamtube_android`) +
  `android/` Gradle shell adapted from the gpui-mobile example template.
  Launcher activity `dev.gpui.mobile.GpuiActivity` (← `GpuiInputActivity`
  ← `NativeActivity` — the IME lineage is intact). minSdk 26 / targetSdk 35 /
  compileSdk 35, `abiFilters arm64-v8a + x86_64`, `supportsRtl=true`,
  `windowSoftInputMode=adjustResize`, Vulkan optional + GLES3 required,
  minimal permissions (INTERNET, ACCESS_NETWORK_STATE, POST_NOTIFICATIONS).
  `cargo ndk` builds both ABIs → jniLibs; `./gradlew assembleDebug` →
  **BUILD SUCCESSFUL**; APK verified with `aapt2 dump badging`.
- **B5 done:** the shell renders a real Home feed over the network (the
  desktop's unsigned-home path: 4 `EXPLORE` categories via
  `innertube::explore` fetched in parallel threads, interleaved, deduped) and
  working simple search (`innertube::search` via the Search button or Enter).
  Verified live on the x86_64 API-35 emulator with real network content
  (screenshots + logcat below). Row→watch flow is Track C (mpv surface) —
  rows are not tappable-to-play yet.

## Files added/changed (all uncommitted)

```
Cargo.toml                        +1 line: workspace member crates/streamtube-android
Cargo.lock                        gpui-mobile@635b7e7 + android-activity/… deps recorded
.gitignore                        android build outputs + jniLibs + 10MB emoji font excluded
crates/streamtube-android/
  Cargo.toml                      cdylib; 6-feature gpui-mobile git dep; all deps android-gated
  src/lib.rs                      android_main → store data dir → GPUI boot → Shell UI
android/
  settings.gradle.kts             rootProject StreamTubeAndroid
  build.gradle.kts                AGP 9.1.0; buildRust{Debug,Release} incl. post-processing
  gradle.properties, gradlew(.bat), gradle/wrapper/*   template (Gradle 9.9.2)
  local.properties                sdk.dir (gitignored)
  app/build.gradle.kts            namespace dev.streamtube.android, minSdk 26 / target 35,
                                  abiFilters arm64-v8a+x86_64, nativeLibraryName placeholder,
                                  jniLibs keepDebugSymbols for libstreamtube_android.so
  app/proguard-rules.pro          from template
  app/src/main/AndroidManifest.xml  INTERNET + ACCESS_NETWORK_STATE + POST_NOTIFICATIONS only;
                                  GpuiActivity launcher + streamtube:// deeplink filter;
                                  path/picker/permission helper activities kept; unused
                                  biometric helper removed so no biometric permissions merge
  app/src/main/java/dev/gpui/mobile/*.java   16 required template helpers (the native side hardcodes
                                  the dev.gpui.mobile.* class names — the package must stay)
  app/src/main/res/**             template res; app_name "Stream Tube", surface 0x0f0f0f,
                                  play-triangle splash icon
  app/src/main/assets/fonts/NotoColorEmoji.ttf   10MB CBDT emoji fallback (gitignored)
  app/src/main/jniLibs/<abi>/     build output (gitignored): libstreamtube_android.so
                                  + the 9 optional mpv libs from scripts/fetch-libmpv.sh
scripts/fetch-libmpv.sh           stages the 9 C2-verified playback libs into jniLibs
scripts/fetch-android-fonts.sh    fetches the SHA-256-pinned CBDT emoji fallback
docs/evidence/B2/                 this report + logs + smoke screenshots
```

## The bootstrap contract (B3 core)

`android_main` in `crates/streamtube-android/src/lib.rs`:

1. `android_logger::init_once` (logcat tag `streamtube`, Debug) +
   `jni::install_panic_hook()`.
2. `files_dir = AndroidApp::internal_data_path()` (== `Context.getFilesDir()`),
   then **before any store call**: `std::env::set_var("STREAMTUBE_DATA_DIR", …)`
   (edition-2024 `unsafe` block, same as the desktop's `main()`),
   `store::set_data_dir(&files_dir)`, `store::init()`. The SQLite store lives
   in `/data/user/0/dev.streamtube.android/files` — proven by the smoke logcat.
3. `gpui_mobile::android::jni::init_platform(&app)` → `shared_platform()` →
   `Application::with_platform(shared.into_rc()).with_http_client(UreqHttp::new()).run(…)`.
4. Inside `run`: `gpui_kit::init`, dark `Theme`, one fullscreen window wrapping
   `Shell` in `gpui_kit::component::Root`.
5. Deeplink: a `streamtube://search?q=…` launch intent opens straight into the
   search results (exercises the enabled `deeplink` feature;
   `smoke/logcat-key-lines.txt` shows it end-to-end).

## Commands (reproducible)

```bash
# env (NDK r27c; LIBCLANG_PATH is required by rquickjs-sys bindgen — the same
# fix the android-core CI job applies; desktop checks need RUST_FONTCONFIG_DLOPEN=on
# because fontconfig-devel is not installed on this host)
export ANDROID_HOME=$HOME/Android/Sdk
export ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/27.2.12479018
export LIBCLANG_PATH="$(dirname "$(find $ANDROID_NDK_HOME/toolchains/llvm/prebuilt \
    -name libclang.so -print -quit)")"
# → …/toolchains/llvm/prebuilt/linux-x86_64/musl/lib

# type-check
cargo ndk -t arm64-v8a -P 26 check -p streamtube-android      # EXIT=0

# build both ABIs into jniLibs (debug)
cargo ndk -t arm64-v8a -t x86_64 -P 26 \
    -o android/app/src/main/jniLibs build -p streamtube-android   # EXIT=0
# post-process (the gradle buildRustDebug task does this automatically):
llvm-strip --strip-debug …/jniLibs/*/libstreamtube_android.so
rm …/jniLibs/*/libgpui_mobile.so        # cargo-ndk stray (see notes)

# APK
cd android && ./gradlew assembleDebug                          # BUILD SUCCESSFUL

# desktop safety
RUST_FONTCONFIG_DLOPEN=on cargo check --workspace             # EXIT=0

# optional playback payload (Track C2's verified libs)
ST_APK_DIR=…/streamtube-native ../scripts/fetch-libmpv.sh
```

`platforms;android-35` was installed via `sdkmanager "platforms;android-35"`
(only android-37.0 had been present).

## Build results

- `cargo ndk -t arm64-v8a -P 26 check -p streamtube-android`: **EXIT=0**
  (`cargo-check-arm64.log`; whole graph incl. gpui-kit/gpui-mobile on the
  unified gpui-pre 0.3.12). First run failed only at `rquickjs-sys` bindgen
  (`Unable to find libclang`) — fixed by `LIBCLANG_PATH` from the NDK.
- `cargo ndk -t arm64-v8a -t x86_64 -P 26 -o … build -p streamtube-android`:
  **EXIT=0**, ~17 min (`cargo-build-both.log`).
- **Debug `.so` sizes:** 921,743,864 B (arm64) / 924,347,576 B (x86_64) as
  built (DWARF dominates). `llvm-strip --strip-debug` → **55,113,208 B /
  54,239,024 B**. The strip + stray-cleanup is automated in the
  `buildRustDebug`/`buildRustRelease` gradle tasks.
- **16KB page-size check (B4-style, bonus):** `llvm-readelf -lW` on both
  built shells: every LOAD segment has **Align 0x4000** ✓
  (`readelf-16kb.txt`; the mpv set was already
  verified in docs/evidence/C2).
- `ANativeActivity_onCreate` is exported by `libstreamtube_android.so` ✓
  (android-activity's bootstrap is linked in).
- `./gradlew assembleDebug`: **BUILD SUCCESSFUL in 2m 38s** first run
  (AGP 9.1.0 / Gradle 9.9.2 / JDK 25), incremental ~4-6s after.
- **Final debug APK after coordinator cleanup:** 248,125,984 B (~237 MB; both stripped Rust shells +
  the 67 MB mpv payload + 10 MB emoji font),
  sha256 `1a71bb0be6787d18b0445ccb97f8cf2dd60338700bddcafdedcb0bc9678e6b57`.
- `aapt2 dump badging/xmltree`: `package dev.streamtube.android`,
  `versionName 0.7.5-alpha`, `minSdkVersion 26`, `targetSdkVersion 35`,
  `supportsRtl true`, `glEsVersion 0x00030000`,
  `native-code arm64-v8a+x86_64`, launcher `dev.gpui.mobile.GpuiActivity`,
  permissions INTERNET + ACCESS_NETWORK_STATE + POST_NOTIFICATIONS
  (the unused biometric helper/dependency was removed by the coordinator, so
  USE_BIOMETRIC/USE_FINGERPRINT no longer merge).
- Coordinator re-verification: APK signature v2 valid, 16 KiB zipalign valid,
  reinstall succeeded, and `smoke/coordinator-home.png` shows the populated
  live Home feed with no crash. See `coordinator-gradle-assemble-debug.log`
  and `coordinator-desktop-check.log`.

## Optional playback payload

`scripts/fetch-libmpv.sh` (adapted from `docs/evidence/C2/fetch-libmpv.sh`)
stages the 9 C2-verified libraries (libmpv + 8 ffmpeg/libc++ libs,
`libplayer.so` dropped) into `android/app/src/main/jniLibs/<abi>/`. Verified
run: both APK SHA-256 checks passed, 9 libs staged per ABI (67 MB total).
**Gradle does not assume them** — nothing links libmpv until Track C wires
playback; the Home-only debug APK builds without them (the first
`assembleDebug` validated the whole Java/manifest side before any Rust code
was even built, and the shell itself never dlopens libmpv).

## Emulator smoke (API 35, x86_64 — "محقّق على محاكي؛ الجهاز الحقيقي معلّق")

Environment note: the first smoke attempt on the default
`-gpu swiftshader_indirect` emulator **crashed the emulator process** right
after wgpu initialised VkDevice — SwiftShader's SPIR-V validator rejects the
shader `OpSource` operand wgpu 30 emits (`SPIR-V ERROR: Invalid source
language operand: 10`) and qemu dies with it. Relaunching with
**`-gpu host`** (this host has an AMD Radeon 820M / RADV Vulkan 1.4) makes the
app render through the real GPU: logcat shows
`Created VkInstance for application:'wgpu'` → `Selected GPU (passed
configuration test): AMD Radeon 820M Graphics (RADV GFX1153) (Vulkan)`.
Devices without usable Vulkan remain a real deployment risk to track.

`smoke/` holds the evidence: `home.png` (home feed live), `home2.png`,
`search.png` (search results), `logcat-smoke.log`, `logcat-key-lines.txt`.

- install → `am start -n dev.streamtube.android/dev.gpui.mobile.GpuiActivity`:
  `android_main: entered` → `store initialised at
  /data/user/0/dev.streamtube.android/files` → `platform initialised` →
  `window opened` → `NATIVE_INITIALIZED = true`. Fonts load from
  /system/fonts + the bundled CBDT emoji asset.
- Home renders real network content (vision-verified earlier against
  `home.png`: GTA 6 reaction, Macy's parade postponement, SAFF semi-final —
  i.e. live Explore feed items with thumbnails/titles/channels).
- Search: `am start -d "streamtube://search?q=mars%20rover"` →
  `deeplink: streamtube://search?q=mars%20rover` → `search_for: "mars rover"`
  → results list renders. The same core call from the host probe
  (`cargo run` in scratch/b2-search-probe) returns 39 items with titles like
  *“Mars Rover Last Words. NASA's Opportunity rover”*, *“Every Mars Rover
  Explained — Supernova”* — the items the app displays.
- Tapping the “Back to Home” chip returns to the Home header + feed (pixel
  diff confirms the header text reverts and the feed reloads).
- `adb logcat -d | grep -E "FATAL|AndroidRuntime"` → **0 hits**; the app
  process survives the whole session.
- A stray first-run Calendar “What's New” popup on the Google-APIs image
  stole focus mid-test once (it ate taps and eventually paused/destroyed our
  activity — the clean lifecycle log shows Pause→TerminateWindow→Stop→
  Destroy→quit, a useful B6 lifecycle sample). `am force-stop
  com.android.calendar` removes it.

## Findings for the next tracks (important)

1. **Taps = synthesized mouse presses.** gpui-pre 0.3.12's
   `TouchGestureRecognizer` converts taps into synthesized mouse down/up so
   **`on_click` works on Android**; `on_mouse_down` does **not** fire for
   touches. (The gpui-mobile example's `on_mouse_down` pattern is misleading
   here; its Android demo screens are not tap-verified by CI.) Any mobile UI
   in this repo should use `on_click`/click-style handlers.
2. **IME (Gboard) eats adb-injected text/Enter while open.** Typing works
   when key events arrive raw (no IME bound). Full IME composition
   (commitText through `GpuiInputActivity`'s InputConnection, Arabic keyboard)
   is unverified — exactly the B6 measurement item. For scripted UI tests,
   prefer deeplinks (`streamtube://search?q=…`) or disable the IME.
3. **Relaunch after a clean quit needs a process kill.** After BACK exits the
   activity cleanly, a new `am start` re-enters `android_main` in the still-
   alive process, but gpui-mobile's global platform state (OnceLock) is not
   re-initialisable, so no window opens (`Ignoring spurious JVM callback…`).
   `am force-stop dev.streamtube.android` before relaunch fixes it. Worth a
   small upstream patch or an onDestroy process exit for B6's 100-cycle test.
4. **cargo-ndk copies every cdylib it built**, including gpui-mobile's own
   `libgpui_mobile.so` (514 MB debug), into jniLibs — dead weight, since the
   example pattern (`extern crate gpui_mobile`) statically links gpui-mobile
   into the shell. The gradle rust tasks delete it and strip the shell's
   DWARF automatically.
5. Desktop is unaffected: the crate is `#![cfg(target_os = "android")]`, so
   host builds compile it to an empty cdylib; `RUST_FONTCONFIG_DLOPEN=on
   cargo check --workspace` is green (`desktop-check.log`; the fontconfig env
   is a pre-existing host quirk, not from this change — the desktop release
   binary in `target/release` predates this work and the same env was needed
   without it).
6. The `wgpu 29.0.4` lock entry (iOS-only pin inside gpui-mobile) is the
   known cosmetic extra from B1 — never compiled on linux/android.

## Not done / deferred

- Row→watch flow and “instant open from the cached feed”: Track C (mpv
  surface + platform_view). The Home cache (store-backed `home_cache`) also
  lands with playback work.
- CI: the existing `android-core` gate still checks `-p streamtube-core`
  and stays green; a dedicated `android-shell` job (cargo ndk check both
  ABIs + assembleDebug) is a natural follow-up but was skipped to keep this
  diff minimal.
- Nothing committed (per task instructions); jniLibs, the emoji font and all
  build outputs are gitignored.
