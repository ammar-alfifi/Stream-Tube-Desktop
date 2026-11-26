# Release size

`app-release.apk` is built arm64-only, signed (APK Signature Scheme v2),
16 KB aligned, with fully stripped native libraries.

Before: 175,768,953 bytes (167.6 MiB), arm64 + x86_64.
After:   63,318,299 bytes  (60.4 MiB), arm64 only.

Changes:
- release profile: opt-level="s", fat LTO, codegen-units=1, panic=abort,
  strip="symbols".
- Gradle no longer keeps native debug symbols.
- all arm64 .so files stripped with llvm-strip --strip-unneeded.
- x86_64 payload omitted from release packaging.
- the x86_64 folder is restored after the build for local emulator work.

Measured composition:
- lib/arm64-v8a: 49.5 MiB (Rust 18.5 MiB, FFmpeg/libmpv ~30 MiB,
  libgpui_mobile 0.6 MiB).
- NotoColorEmoji: 9.4 MiB compressed.
- classes.dex: 1.1 MiB compressed.
- resources: ~0.3 MiB.
