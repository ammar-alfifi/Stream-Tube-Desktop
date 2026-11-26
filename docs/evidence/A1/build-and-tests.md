# A1/A5 — build & test commands with real outputs

> **Integration update:** the concurrent rquickjs and playback issues recorded
> below were resolved. Final integrated results are 60 core tests + 1 data-dir
> test passed, desktop check exit 0, and Android arm64/x86_64 checks passed.
> `coordinator-host-rerun.txt` is a clean re-run after integration.

All commands run in the repo worktree unless noted "scratch tree"
(`/home/ammar/.hermes/cache/scratch/a1-emu` = committed HEAD `f795b66` + this
task's diff only — see `conflict-rquickjs-android.md` for why the Android
binary could not be built in the shared worktree).

## Compile checks (host)

```
$ cargo check -p streamtube-core --example a1_harness
    Checking streamtube-core v0.7.5-alpha (…/crates/streamtube-core)
    Finished `dev` profile [optimized + debuginfo] target(s) in 21.13s
CHECK_EXIT=0
```

## Core test suite (desktop regression)

```
$ cargo test -p streamtube-core
running 82 tests
test result: FAILED. 59 passed; 1 failed; 22 ignored; 0 measured; 0 filtered out

failures:
---- playback::tests::jump_ahead_finds_the_first_strong_local_peak stdout ----
thread panicked at crates/streamtube-core/src/playback.rs:217:9:
assertion `left == right` failed
  left: Some(20.0)
 right: Some(60.0)
```

The one failure is in `crates/streamtube-core/src/playback.rs`, a new untracked
file another agent is writing concurrently (Track C); no A1 file is involved.

```
$ cargo test -p streamtube-core --lib -- streams
test result: ok. 3 passed; 0 failed; 7 ignored; 0 measured; 72 filtered out

$ cargo test -p streamtube-core --lib -- http
test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 79 filtered out
```

## Desktop app check (golden-rule attempt)

`cargo check -p streamtube` (the GPUI desktop app) cannot complete **on this
host** for environmental reasons, independent of this diff (A1 touches only
pure-Rust core code, no build scripts, no C/C++):

```
$ cargo check -p streamtube
error occurred in cc-rs: failed to find tool "c++": No such file or directory   # freetype-sys
$ CXX=g++ cargo check -p streamtube
pkg-config output: Package fontconfig was not found in the pkg-config search path.
The system library `fontconfig` required by crate `yeslogic-fontconfig-sys` was not found.
```

Missing system packages (`c++` tool, `fontconfig`/`fontconfig.pc`), a host
setup gap for the environment/GUI tracks, not an A1 regression. The A1 change
keeps the desktop `resolve_anonymous` logic byte-identical (only extracted into
`android_vr_first`), and every core test that exercises the changed file passes.

## Android cross-builds (scratch tree, NDK 27.2.12479018)

```
$ ANDROID_NDK_HOME=~/Android/Sdk/ndk/27.2.12479018 \
  cargo ndk -t x86_64-linux-android -P 26 build -p streamtube-core --release --example a1_harness
X86_BUILD_OK
-rwxr-xr-x 6491512 … target/x86_64-linux-android/release/examples/a1_harness
ELF 64-bit LSB pie executable, x86-64   (pushed to emulator-5554, see emulator-a1-harness.txt)
```

First attempt at an aarch64 `cargo check --target aarch64-linux-android`
**without** cargo-ndk failed on the NDK env (own mistake, recorded):

```
error occurred in cc-rs: failed to find tool "aarch64-linux-android-clang": No such file or directory
```

Re-run correctly through cargo-ndk (build, not just check):

```
$ ANDROID_NDK_HOME=~/Android/Sdk/ndk/27.2.12479018 \
  cargo ndk -t aarch64-linux-android -P 26 build -p streamtube-core --release --example a1_harness --locked
   Compiling streamtube-core v0.7.5-alpha (…/crates/streamtube-core)
    Finished `release` profile [optimized] target(s) in 1m 05s
AARCH64_BUILD_OK
-rwxr-xr-x 6623592 … target/aarch64-linux-android/release/examples/a1_harness
```

The `#[cfg(target_os = "android")]` A1 path (`resolve_anonymous` →
`android_vr_first`) therefore compiles for the real Android target with the
locked dependency set of HEAD `f795b66`. (No arm64 device/emulator exists to
*run* it on — external blocker §5-3 in plan2.md; runtime proof above is the
x86_64 emulator.)
