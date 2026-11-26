# A2-web — watch-page EJS fallback proof

Date: 2026-11-26 (12:01 UTC)

## What was built

- `crates/streamtube-core/src/api/web_stream.rs`: fetches the watch page, takes
  its embedded `ytInitialPlayerResponse`, downloads the player JS the page names
  (`PLAYER_JS_URL`, iframe-API fallback), extracts every format's
  `signatureCipher`/`url`, solves all `sig` + `n` challenges in **one** EJS call
  (`api::nsig::solve`), rewrites the URLs, and returns a `StreamInfo` through
  the same `parse_player` the Innertube clients use. Player JS is cached 12 h.
- `streams.rs` (Android only): the chain is now
  **ANDROID_VR → web+EJS → visionOS → iOS** (`resolve_anonymous` under
  `cfg(target_os = "android")`). Desktop `resolve_anonymous`, `android_vr_first`
  and every desktop backend mode are untouched.
- `examples/web_stream_probe.rs`: live harness — resolves a video through the
  web fallback, prints every step, then range-tests a deciphered URL
  (`bytes=0-1023`) and exits non-zero unless it gets HTTP 206.

## Result (real network)

- Watch page `dQw4w9WgXcQ`: HTTP 200, 1,544,729 bytes, `playabilityStatus=OK`.
- Player JS: `https://www.youtube.com/s/player/b485e332/player_es6.vflset/en_US/base.js`
  (2,721,418 bytes).
- Solved 1 real `sig` and 1 real `n` challenge with the bundled EJS scripts
  inside rquickjs (`EJS solve: 13.99 s` in the **debug** profile).
- Reconstructed googlevideo URL → **HTTP 206**, `video/mp4`,
  `Content-Range: bytes 0-1023/11829048` (`web-probe.log`, `signed-url-check.log`).
- Live `#[ignore]` test `api::web_stream::tests::live_web_fallback_solves_and_streams`:
  **passed** (`live-test.log`).
- Offline unit tests (cipher parsing, n splicing, embedded-JSON extraction,
  collect/rewrite): 10 passed; full desktop lib suite 70 passed / 0 failed.
- Android: `cargo ndk -t arm64-v8a -t x86_64 -P 26 check -p streamtube-core --locked`
  passed for both targets (`android-check.log`).

## Findings and limits (documented, not hidden)

- The anonymous WEB `/youtubei/v1/player` Innertube call is bot-gated from this
  network: HTTP 200 with `playabilityStatus=UNPLAYABLE`, reason "The page needs
  to be reloaded." — with and without `visitorData`. The watch page HTML still
  serves a full player response, so the HTML is the extraction source.
- In today's watch-page player response only the muxed itag 18 carries a
  `signatureCipher`; the 26 `adaptiveFormats` have **no url and no cipher**
  (PO-token-gated), so the fallback's current yield for that video is the
  360p muxed stream. Any future embedded ciphers/URLs are deciphered by the
  same code path.
- `jNQXAC9IVRw` ("Me at the zoo"): watch page `OK` but zero formats embed URLs;
  the fallback fails cleanly with `web: the watch page carries no decipherable
  formats`, and the Android chain proceeds to visionOS. Failure is reported,
  never faked.
- One harness run hit a transient 30 s timeout fetching the watch page
  (the shared agent's global request timeout); the retry and all later runs
  fetched it in ~1 s.

## Hashes

- player JS (downloaded during verification, same sha256 as the A2 run):
  `6fb3b876821b6f9737119baa78dbfb79fb53e881f212ee421fd3a20b3167e109`
- `yt.solver.core.min.js`: `18da6ce0758b416e7ae645084f4f8801f9f9d59d6c477c05eaa0ff94ebd8cc00`
- `yt.solver.lib.min.js`: `c55987fe697e5b9ee18830163f7af85327e9bb5c3e674b969d38c8d205eaa577`

## Commands

```bash
# offline tests
cargo test -p streamtube-core --lib --locked

# live harness (network)
cargo run -p streamtube-core --example web_stream_probe --locked -- dQw4w9WgXcQ

# live ignored test (network)
cargo test -p streamtube-core --lib --locked live_web -- --ignored --nocapture

# Android cross-check
LIBCLANG_PATH=$NDK/toolchains/llvm/prebuilt/linux-x86_64/musl/lib \
  cargo ndk -t arm64-v8a -t x86_64 -P 26 check -p streamtube-core --locked
```

Note: the main working tree was concurrently mid-change (another agent's
uncommitted `crates/streamtube-android` workspace member broke `cargo` at the
root), so every build/test above ran in an isolated `git worktree` at commit
`e008d7d` with this track's four files copied in.
