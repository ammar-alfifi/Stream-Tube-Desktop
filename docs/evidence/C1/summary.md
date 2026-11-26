# C1 — shared playback policy evidence

Date: 2026-11-26

- Moved platform-independent seek/EOF/chapter/resume/queue/back-cache/SponsorBlock decisions into `streamtube-core::playback`.
- Desktop `src/player/controller.rs` keeps mpv, GPUI, toast, voting, and other side effects; it delegates policy decisions to the core.
- Focused playback policy suite: 11 passed, 0 failed (`playback-tests.log`).
- Integrated core suite: 60 passed, 0 failed, 22 ignored; `data_dir`: 1 passed.
- Desktop `cargo check --bin streamtube --locked`: exit 0 (8 pre-existing warnings).
- Intentionally not moved: mpv frame-progress/recovery, captions, and EQ, which remain tied to the desktop render path.
