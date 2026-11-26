# Stream Tube Android — phase report

Last updated: 2026-11-26

## Definition of done status

Tier 1 is **not complete yet**. No installable APK has been produced or verified.
The following prerequisites are proven with real output:

- **Phase 0/core split:** host tests, desktop release build, Android arm64 check, and remote Linux/Windows/Android CI passed.
- **A1/A5 extraction:** anonymous ANDROID_VR returned 22 video formats; 1080p VP9 and Opus Range requests returned HTTP 206 on host and Android emulator. Public HLS and MP4 fallbacks also served real bytes.
- **A2 solver:** bundled QuickJS solved a real YouTube `sig` and `n` challenge; the reconstructed URL returned HTTP 206. arm64/x86_64 Android checks passed.
- **B1 architecture:** gpui-mobile and gpui-kit resolve to one gpui-pre 0.3.12 graph; Android uses wgpu 30.0.2. No deep GPUI fork is required.
- **C1 playback policy:** seek/resume/queue/back-cache/SponsorBlock decisions are shared in core; desktop delegates to them.
- **C2 native media:** pinned mpv-android binaries verified by SHA-256, exported symbols, 16 KiB LOAD alignment, and live `mpv_initialize()=0` on the emulator. Physical arm64 remains pending.
- **Arabic core:** 837/837 catalog messages, placeholder parity, RTL flag, and 7/7 i18n tests.
- **SponsorBlock:** a real segment (82.143–98.102 s) produced a shared-policy skip target of 98.102 s; actual player seek remains to be verified in the Android shell.

## In progress

- Android GPUI shell (`streamtube-android` + Gradle) with real Home/search.
- Web + EJS extraction fallback wired into the Android chain.
- SurfaceView/libmpv hardware-decoded playback and actual SponsorBlock seeking.
- Arabic RTL/shaping/IME runtime verification.

## External blockers

- Physical arm64 device is not connected; emulator x86_64 and arm64 translation are not substitutes for the final device gate.
- Public distribution of the current GPL mpv/FFmpeg binaries requires either an LGPL build or complete corresponding-source compliance.

## Evidence index

- `docs/evidence/environment/`
- `docs/evidence/A1/`
- `docs/evidence/A2/`
- `docs/evidence/B1/`
- `docs/evidence/C1/`
- `docs/evidence/C2/`
- `docs/evidence/E-ar/`
- `docs/evidence/sponsorblock-live-skip.log`
