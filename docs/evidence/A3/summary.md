# A3 — verified EJS updater

## What changed

- `api/nsig::update_from_github()` reads the latest `yt-dlp/ejs` release, requires HTTPS asset URLs under the expected GitHub repository path, requires SHA-256 asset digests, downloads both solver scripts, and verifies them.
- Each install is written to a unique generation directory. The final `CURRENT` pointer is renamed only after both scripts and `SHA256SUMS` are complete, so readers see either the old generation or the new one, never a mixed pair. Previous generations remain intact.
- `solve()` prefers the selected generation only when both files match valid recorded hashes; otherwise it uses the bundled 0.8.0 scripts.
- Trust boundary: the app data directory is private to the app/user. Hashes detect corruption, not a malicious process with the same OS identity. Downloaded scripts execute in QuickJS with no host functions, a memory limit, and a deadline.
- Added generation, URL, malformed-hash, missing-file, and non-UTF-8 tests plus an isolated live probe example.

## Verified

- `cargo test -p streamtube-core api::nsig --locked`: 5 passed.
- Live latest-release probe installed EJS `0.8.0`:
  - `yt.solver.lib.min.js`: 151,561 bytes
  - `yt.solver.core.min.js`: 6,945 bytes
  - `SHA256SUMS`: 175 bytes
- The installed scripts then solved a real YouTube player `sig` and `n` challenge; both response maps were non-empty.
- Android checks for arm64-v8a and x86_64 passed after the change.

Evidence: `live-update.log`, `live-real-challenge.log`.

## Remaining

The updater is callable from core but not yet exposed in the Android settings UI. Automatic update policy and user-visible status remain Track A3 UI work.
