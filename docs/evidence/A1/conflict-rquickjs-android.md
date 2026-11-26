# Shared-tree conflict: A2's in-flight rquickjs dep blocks Android cross-builds

> **Resolved after this reconnaissance:** commit `2203484` enabled `rquickjs`'s
> `bindgen` feature and set NDK libclang in CI. Both arm64-v8a and x86_64 Android
> checks now pass in the shared worktree. The original failure is retained below
> as historical evidence.

The parallel Track A2 agent added an unconditional `rquickjs` dependency and
`api::nsig` module to `crates/streamtube-core` (uncommitted at the time).
`rquickjs-sys 0.14` ships **no prebuilt bindings for any Android target**:

```
$ ls ~/.cargo/registry/src/index.crates.io-*/rquickjs-sys-0.14.0/src/bindings/
aarch64-apple-darwin.rs aarch64-pc-windows-msvc.rs aarch64-unknown-linux-gnu.rs
aarch64-unknown-linux-musl.rs armv7-unknown-linux-gnueabihf.rs ...
x86_64-apple-darwin.rs x86_64-pc-windows-gnu.rs x86_64-pc-windows-msvc.rs
x86_64-unknown-linux-gnu.rs x86_64-unknown-linux-musl.rs
(no *-linux-android entries)
```

So building the shared worktree for the emulator target fails before any of the
A1 code is reached:

```
$ ANDROID_NDK_HOME=~/Android/Sdk/ndk/27.2.12479018 cargo ndk -t x86_64-linux-android -P 26 \
    build -p streamtube-core --release --example a1_harness
   Compiling ureq v3.5.2
error: couldn't find file `~/.cargo/registry/src/index.crates.io-*/rquickjs-sys-0.14.0/src/bindings/x86_64-linux-android.rs`
  --> rquickjs-sys-0.14.0/src/lib.rs:19:1
   |
19 | include!(concat!("bindings/", bindings_env!("TARGET"), ".rs"));
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: rquickjs-sys@0.14.0: rquickjs probably doesn't ship bindings for platform
         `x86_64-linux-android(n/a)`. try the `bindgen` feature instead.
error: could not compile `rquickjs-sys` (lib) due to 1 previous error
```

Plan2.md Track A2 explicitly targets "rquickjs عبر NDK", so enabling the
`bindgen` feature (or gating the dependency) for Android is that track's own
todo; this file records the conflict so the A1 emulator proof is not mistaken
for an A1 defect.

**A1 workaround (no other agent's file touched):** the emulator binary is built
in an isolated scratch tree containing committed HEAD
(`f795b66` = `5440e39` + docs-only commits) plus exactly the A1/A5 diff
(`crates/streamtube-core/src/api/streams.rs` and
`crates/streamtube-core/examples/a1_harness.rs`, verified byte-identical to the
worktree copies). Scratch build commands are recorded in `emulator-run.md`.
