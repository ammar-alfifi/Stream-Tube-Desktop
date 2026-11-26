# C3 — real Android playback evidence

## Result

Verified on `emulator-5554` (API 35, x86_64 native): tapping a video in Home or
search opens a real libmpv player with video and audio. The final coordinator
run used the code in this change, not a stub.

### Proven runtime path

- `Mpv::new_external` with `vo=gpu`, `gpu-context=android`,
  `hwdec=mediacodec,mediacodec-copy`, `ao=audiotrack`, and the Surface jobject as
  `wid`; `set_java_vm` and `set_surface` succeed.
- A 1080p adaptive EDL played with:
  - `vo=Some("gpu")`
  - `hwdec=Some("mediacodec")`
  - `video-format=Some("h264")`
  - `avc1.640028` video + Opus audio
  - `time-pos` advancing 0 → 5 → 10 → 15 seconds
- Android `dumpsys audio` shows an `android.media.AudioTrack` for the app PID,
  `event:started`, stereo 48 kHz, and matching pause/stop events.
- Pause is proven by `player: pause toggled -> true`, the button changing to
  **Play**, and AudioTrack entering `event:paused`.
- Back is proven by `player: back requested`, clean `player: dropped`, and
  `surfaceDestroyed`; the screenshot returns to the cached search results.
- SponsorBlock is live: one segment was fetched for `GUD1uXn3-Gs`, then
  `sponsor skip: interaction 5.558s..8.978s → seeking to 8.978`; the next
  `time-pos` was 10 seconds and playback continued.

### Fallback path

When a googlevideo range probe fails, the player records the exact fallback and
loads muxed itag 18 (360p H.264/AAC). A real non-black frame and advancing
`time-pos` were captured. The failure is intermittent: later runs passed both
probes and played the 1080p EDL. A PO token is therefore not always required,
but its absence can still cause probe failures and mid-stream stalls.

## Coordinator verification

```bash
cargo ndk -t arm64-v8a -t x86_64 -P 26 check \
  -p streamtube-core -p streamtube-android --locked
cargo test -p streamtube-core --locked
cargo ndk -t arm64-v8a -t x86_64 -P 26 \
  -o android/app/src/main/jniLibs build -p streamtube-android --locked
cd android && ./gradlew assembleDebug
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
```

Final checked APK:

- Size: 197,156,176 bytes.
- SHA-256: `239a6ed27eb2acedb0d42cb368cc337cc47f93efde9ac68aee709a5a11c5e4d1`
- APK Signature Scheme v2: verified.
- `zipalign -c -P 16`: verified.

Review hardening:

- Surface callbacks use unique per-player view ids; stale callbacks and an old
  player's Drop cannot clear a newer player's sender.
- JNI global references are RAII-owned until transferred to the player, so an
  undelivered/dropped message cannot leak a Surface.
- Network probes no longer keep an `Arc<Mpv>` alive after player teardown.
- Surface destruction detaches/stops mpv synchronously in the JNI callback,
  before Android invalidates the Surface; recreation rebinds and reloads the
  selected source. If detachment/rebinding fails, mpv is synchronously shut down
  and recreated with the new Surface as its pre-init `wid`. This was exercised
  with Home → resume on the emulator.
- Reload and Restart reset time, logging, buffering, and SponsorBlock skipped
  state; the final runtime log shows the same sponsor segment skipped again
  after Restart.
- Resolved sources, mpv paths, and native pointers are not logged. Range probes
  are capped at 1 MiB and require the full expected chunk.

Key evidence files:

- `coordinator-final-runtime.txt` — final reviewed build: 1080p EDL, hwdec, live SponsorBlock seek.
- `coordinator-final-resume.txt` — synchronous Surface destroy/create, reload, and time reset.
- `coordinator-restart-runtime.txt` — Restart resets to 0 and SponsorBlock skips again.
- `coordinator-controls-runtime.txt` — advancing time, Pause, Back, clean drop.
- `coordinator-audiotrack.txt` — real AudioTrack playback/pause/stop.
- `45-reviewed-playing.png`, `49-final-resumed.png`, `48-reviewed-back.png`.
- `coordinator-build-final-reviewed.log`, `coordinator-apk-final-reviewed.txt`.

## Remaining limits

- Physical arm64 verification is still required; the emulator is smoke evidence only.
- PO-token failures can still stall long videos around later 8 MiB chunks.
- gpui-pre can stop synthesizing clicks after an in-player seek; this was
  observed in the child run and is not fixed here. Pause/Back are proven on a
  fresh session before any seek.
- Background playback, PiP, MediaSession, downloads, and the remaining Tier 1
  settings/resume/history UI are not part of C3.
