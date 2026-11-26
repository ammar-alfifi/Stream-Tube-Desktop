#!/usr/bin/env bash
# Build the phone release APK: arm64 only, fully stripped native libraries.
# The emulator ABI (x86_64) is deliberately not packaged; debug builds can
# still produce it with cargo-ndk when needed.
set -euo pipefail
cd "$(dirname "$0")/.."

export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export CXX="${CXX:-$HOME/.local/bin/g++}"
export RUST_FONTCONFIG_DLOPEN=on
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/27.2.12479018}"
export LIBCLANG_PATH="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/musl/lib"
STRIP="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip"

cargo ndk -t arm64-v8a -P 26 -o android/app/src/main/jniLibs \
  build --release -p streamtube-android --locked

# Cargo's strip = "symbols" applies to the Rust cdylib. FFmpeg/libmpv arrive
# prebuilt with their own symbol tables; strip them too.
for so in android/app/src/main/jniLibs/arm64-v8a/*.so; do
  "$STRIP" --strip-unneeded "$so"
done

# A phone never loads the emulator ABI. Leaving it in the folder made every
# release carry an extra ~81 MB. Move it aside for packaging, then restore it so
# the next emulator build still has its libraries.
X86="android/app/src/main/jniLibs/x86_64"
X86_ASIDE="android/app/src/main/x86_64.release-skip"
if [ -d "$X86" ]; then
  mv "$X86" "$X86_ASIDE"
  trap 'mv "$X86_ASIDE" "$X86" 2>/dev/null || true' EXIT
fi

cd android
./gradlew assembleRelease --console=plain
