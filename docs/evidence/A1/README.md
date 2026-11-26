# A1/A5 evidence — YouTube extraction without yt-dlp (Android path) + general HLS/MP4 fallback

Task: plan2.md Track A1 ("android_vr ثم visionOS بلا yt-dlp… أثبت ستريم صالحاً على المحاكي")
and Track A5 ("خط تشغيل بفيديو HLS/MP4 عام معروف الصلاحية… حتى لو حُجب يوتيوب (DR-2)").

Code change (smallest diff): `crates/streamtube-core/src/api/streams.rs` —
`resolve_anonymous` is now `#[cfg(target_os = "android")]`-split: the Android
path is exactly `android_vr_first()` (ANDROID_VR → visionOS → iOS, no yt-dlp,
no backend dispatch — there is no yt-dlp binary on Android); the desktop path
is unchanged. The `"android_vr"` desktop setting mode now delegates to the same
extracted `pub fn android_vr_first` (byte-identical chain). `FALLBACK_HLS` /
`FALLBACK_MP4` consts were added for Track A5. No `Cargo.toml`, no UI files
touched. Harness: `crates/streamtube-core/examples/a1_harness.rs` (prints real
HTTP status / client / links; exit 0 = stream proven, 3 = DR-2 + fallback
proven, 1 = nothing proven).

## Result

| Claim | Verdict | Evidence |
|---|---|---|
| A1 host: ANDROID_VR extraction serves real bytes, no yt-dlp | ✅ 1080p vp9 itag=248 → HTTP 206 (1,048,576 B); opus itag=251 → HTTP 206 (262,144 B) | `host-a1-harness.txt` |
| A1 emulator: same on Android 15 (API 35) x86_64, real android binary | ✅ same 206s; exit 0 | `emulator-a1-harness.txt` |
| A5 fallback HLS/MP4 general streams | ✅ mux master 200 (752 B, 5 variants) → variant 200 → segment 200 (1,915,156 B); sintel MP4 Range → 206 (1,048,576 B) — on host **and** emulator | both harness files |
| DR-2 (YouTube bot-check / all clients refused) | **not triggered** from this network — YouTube served the anonymous ANDROID_VR client (22 video formats, Rick Astley 4K Remaster). Client identity, HTTP statuses and full googlevideo links are recorded verbatim in the harness outputs. | both harness files |
| Desktop regression | integrated core suite: 60 passed / 22 ignored plus `data_dir` 1 passed; desktop `cargo check --bin streamtube --locked`: exit 0 | coordinator re-run, 2026-11-26 |

## Files in this directory

- `host-a1-harness.txt` — final host run (clean, exit 0, `A1_OK … fallback_ok=true`).
- `emulator-a1-harness.txt` — final emulator run (exit 0, `A1_OK … fallback_ok=true`).
- `network-troubleshooting.md` — real curl isolation of the two A5 flakiness causes
  (media.w3.org answers 403 to browser UAs; ureq→Fastly stalls intermittently
  while curl succeeds 7/7) and the harness fixes (plain UA off-YouTube, 30 s
  budget like `http::agent`, transport retry ×3).
- `conflict-rquickjs-android.md` — the shared-tree conflict: the parallel A2
  agent's unconditional `rquickjs` dep has no Android bindings, so the *shared
  worktree* cannot cross-build for Android until A2 enables `bindgen`/gates it.
  The emulator binary was therefore built in an isolated scratch tree
  (committed HEAD `f795b66` + this task's diff only); commands below.
- `build-and-tests.md` — build/test commands and outputs (host + android targets).

## Reproduction commands

```bash
# host
cargo run -p streamtube-core --example a1_harness -- dQw4w9WgXcQ

# emulator (x86_64) — from HEAD f795b66 + A1/A5 diff (scratch tree, see conflict note)
ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/27.2.12479018 \
  cargo ndk -t x86_64-linux-android -P 26 build -p streamtube-core --release --example a1_harness
adb push target/x86_64-linux-android/release/examples/a1_harness /data/local/tmp/
adb shell 'TMPDIR=/data/local/tmp /data/local/tmp/a1_harness dQw4w9WgXcQ'
```

Harness semantics: `client` line = the Innertube client that actually resolved
(`source` field: `innertube` = ANDROID_VR 1.65.10 / Quest 3, `visionos` =
VISIONOS 1.02 / RealityDevice17,1, `ios` = IOS 20.10.4 / iPhone16,2); every
"HTTP nnn" line is a real status code returned to that exact request; the
googlevideo URLs are the real signed links served to the anonymous client
(they expire after ~6 h).

Scope note (honest limitation): "ستريم صالح" here is proven at the
network/byte level — the resolved links serve real media bytes on the emulator.
End-to-end mpv/Media3 playback of those links needs the Android shell (Track
B/C), which does not exist yet; that is the wrapper's next step, not A1's.
