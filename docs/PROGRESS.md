# Android execution progress

Last updated: 2026-11-26

| Task | Attempts | Status | Evidence | Next |
|---|---:|---|---|---|
| Phase 0 | - | done | commit `5440e39`; host/Android/release gates | Track A1 + B1 in parallel |
| A1 android_vr/visionOS extraction | 1 | done | `docs/evidence/A1/` (real 1080p VP9 + Opus HTTP 206 on host/emulator; fallback HLS/MP4 proven) | web+EJS fallback integration (A2) |
| A2 EJS solver + web fallback | 2 | done | `docs/evidence/A2/` + `docs/evidence/A2-web/` (real sig+n solved; signed URL HTTP 206; Android chain integrated) | A3 solver update mechanism / A4 PO tokens |
| B1 gpui-mobile alignment | 1 | done | `docs/VERSION_ALIGNMENT.md`; `docs/evidence/B1/` (gpui-pre 0.3.12 unified; probe exit 0) | B6 lifecycle/RAM/IME measurements |
| B2/B3/B4 Android shell | 1 | done | commit `6fbe979`; `docs/evidence/B2/` (both ABIs, 16KB, signed debug APK, live Home/search) | dedicated Android CI + release signing |
| Android CI/release packaging | 2 | remote CI green; release build pending | `.github/workflows/android.yml`; `docs/INSTALL.md`; all four checks green on `af2f35f` | C3 remote APK build + signed release |
| B6 Home RAM | 1 | debug budget missed | `docs/evidence/B6/meminfo-home-debug.log` (435,390 KB PSS > 250 MB) | release measurement + thumbnail/feed memory bounds |
| B5 Home/search | 1 | network UI + cached Back done; video preload pending | `docs/evidence/B2/smoke/`; `docs/evidence/C3/player/43-back-after-pause.png` | optional video preload |
| C1 controller state split | 1 | done | `docs/evidence/C1/` (11 playback tests; integrated core/desktop checks green) | reuse from Android player |
| C2 libmpv binaries | 1 | done for emulator; CI LGPL build pending | `docs/evidence/C2/` (29 hashes OK, 16KB, emulator mpv_initialize=0) | physical arm64 + LGPL/CI build |
| C3 SurfaceView playback | 1 | done on emulator; physical arm64 pending | `docs/evidence/C3/player/` (1080p EDL, hwdec=mediacodec, AudioTrack, Pause/Back, live SponsorBlock seek) | A4 PO tokens + C6 system integration |
| A3 EJS updater | 1 | core + live verification done; settings UI pending | `docs/evidence/A3/` (latest release hash-verified; real sig/n solved) | expose updater in Android settings |
| Arabic core locale | 1 | core done; UI/IME audit pending | `docs/evidence/E-ar/` (837 keys, 7/7 i18n tests) | RTL/shaping/IME and dynamic plurals in Android shell |
| SponsorBlock live API + player | 2 | live decision and actual mpv seek proven | `docs/evidence/sponsorblock-live-skip.log`; `docs/evidence/C3/player/coordinator-sponsorblock-runtime.txt` | retain across quality/fallback changes |
| Environment/fork/emulator/signing | 1 | done | `docs/evidence/environment/setup.md`; fork PR #1; emulator-5554 booted | keep emulator warm for APK verification |
