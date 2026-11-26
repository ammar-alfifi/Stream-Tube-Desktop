# Install Stream Tube for Android

## Current status

A debug APK is verified on the API 35 x86_64 emulator. A release APK and physical
arm64 verification are still pending; do not treat the debug build as the final
Tier 1 deliverable.

## Build from source

Requirements: Rust stable, JDK 17+, Android SDK platform 35, NDK r27c, and
`cargo-ndk 4.1.2`.

```bash
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --version 4.1.2 --locked
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.2.12479018"
export LIBCLANG_PATH="$(dirname "$(find "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt" -name libclang.so -print -quit)")"

scripts/fetch-android-fonts.sh
scripts/fetch-libmpv.sh

cargo ndk -t arm64-v8a -t x86_64 -P 26 \
  -o android/app/src/main/jniLibs build -p streamtube-android --locked

# Strip debug sections and remove cargo-ndk's stray libgpui_mobile.so.
STRIP="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip"
for abi in arm64-v8a x86_64; do
  "$STRIP" --strip-debug "android/app/src/main/jniLibs/$abi/libstreamtube_android.so"
  rm -f "android/app/src/main/jniLibs/$abi/libgpui_mobile.so"
done

cd android
./gradlew assembleDebug
```

The Gradle `buildAll` task runs the Rust build and APK packaging together.
`buildRustRelease` plus `assembleRelease` creates the release variant.

## Release signing

Release signing reads `~/.android/streamtube-release.properties` (mode `0600`).
The file and keystore are deliberately outside git. It must contain:

```properties
storeFile=/absolute/path/to/streamtube-release.jks
storePassword=...
keyAlias=streamtube
keyPassword=...
```

Without that file, Gradle still configures, but the release APK is unsigned.
Never commit the properties file or keystore.

## Install

```bash
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n dev.streamtube.android/dev.gpui.mobile.GpuiActivity
```

Package: `dev.streamtube.android`. Launcher activity:
`dev.gpui.mobile.GpuiActivity`.
