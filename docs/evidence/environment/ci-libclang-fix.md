# Android CI libclang fix result

Commit: `e26f73d` (`ci: locate NDK libclang robustly for Android`)

Checked through GitHub API on 2026-11-26:

- `linux`: completed / success
- `android-core`: completed / success
- `windows`: completed / success

The preceding commit `4c7cce9` failed only in the `Check core for Android` step.
A clean local worktree at `4c7cce9` passed with an explicit NDK libclang path, so
the workflow now discovers `libclang.so` under `ANDROID_NDK_HOME` instead of
relying on GitHub Actions expression-time environment expansion.
