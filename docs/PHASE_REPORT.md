# Stream Tube Android — phase report

Last updated: 2026-11-26

## Definition of done status

Tier 1 is **not complete yet**. A signed **debug** APK is built, installed, and verified on the x86_64 emulator with real video/audio playback; the release APK and physical-device gates remain.
The following prerequisites are proven with real output:

- **Phase 0/core split:** host tests, desktop release build, Android arm64 check, and remote Linux/Windows/Android CI passed.
- **A1/A5 extraction:** anonymous ANDROID_VR returned 22 video formats; 1080p VP9 and Opus Range requests returned HTTP 206 on host and Android emulator. Public HLS and MP4 fallbacks also served real bytes.
- **A2 solver + web fallback:** bundled QuickJS solved real YouTube `sig` and `n` challenges; the reconstructed URL returned HTTP 206. The Android chain is ANDROID_VR → web+EJS → visionOS → iOS, and both Android targets compile.
- **A3 EJS updater:** latest yt-dlp/ejs assets are SHA-256 verified before installation; the installed scripts solved a real `sig`/`n` challenge. Settings UI is pending.
- **B1 architecture:** gpui-mobile and gpui-kit resolve to one gpui-pre 0.3.12 graph; Android uses wgpu 30.0.2. No deep GPUI fork is required.
- **B2/B3/B4/B5 shell:** `streamtube-android` + Gradle produce a signed, 16 KiB-aligned debug APK for arm64/x86_64. Real Home and search render live content on the emulator with no FATAL crash.
- **C1 playback policy:** seek/resume/queue/back-cache/SponsorBlock decisions are shared in core; desktop delegates to them.
- **C2 native media:** pinned mpv-android binaries verified by SHA-256, exported symbols, 16 KiB LOAD alignment, and live `mpv_initialize()=0` on the emulator. Physical arm64 remains pending.
- **C3 playback:** SurfaceView + libmpv played 1080p H.264/Opus with `vo=gpu`, `hwdec=mediacodec`, real AudioTrack output, advancing `time-pos`, Pause/Back, and a 360p fallback. Physical arm64 remains pending.
- **Arabic core:** 837/837 catalog messages, placeholder parity, RTL flag, and 7/7 i18n tests.
- **SponsorBlock:** live Android playback fetched a real segment and skipped 5.558–8.978 s to 8.978 s; `time-pos` jumped to 10 s and continued.

## In progress

- C6 background playback/PiP/MediaSession, A4 PO tokens, local history/resume/settings UI, and the signed release APK.
- Arabic RTL/shaping/IME runtime verification plus B6 RAM/lifecycle measurements. Debug Home currently misses the RAM budget at 435,390 KB PSS; release measurement and memory bounds are pending.

## External blockers

- Physical arm64 device is not connected; emulator x86_64 and arm64 translation are not substitutes for the final device gate.
- Public distribution of the current GPL mpv/FFmpeg binaries requires either an LGPL build or complete corresponding-source compliance.

## Evidence index

- `docs/evidence/environment/`
- `docs/evidence/A1/`
- `docs/evidence/A2/`
- `docs/evidence/A2-web/`
- `docs/evidence/A3/`
- `docs/evidence/B1/`
- `docs/evidence/B2/`
- `docs/evidence/C1/`
- `docs/evidence/C2/`
- `docs/evidence/C3/player/`
- `docs/evidence/E-ar/`
- `docs/evidence/sponsorblock-live-skip.log`
