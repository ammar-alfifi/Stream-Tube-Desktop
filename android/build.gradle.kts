// Root build.gradle.kts for the Stream Tube Android shell.
//
// The Rust library is compiled separately with cargo-ndk (see the buildRust*
// tasks below) and placed into app/src/main/jniLibs/<abi>/; Gradle only
// packages it — there is no CMake/ndk-build step.
//
// Build steps (from the repo root):
//   1. ./gradlew buildRustDebug      (or the cargo ndk command it prints)
//   2. ./gradlew assembleDebug
//   3. adb install app/build/outputs/apk/debug/app-debug.apk
//
// Playback libraries (libmpv & friends) are optional: scripts/fetch-libmpv.sh
// stages them into jniLibs; a Home-only debug build works without them.

buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:9.1.0")
    }
}

tasks.register("clean", Delete::class) {
    delete(rootProject.layout.buildDirectory)
}

// ── Convenience tasks: build the Rust shell + APK in one go ──────────────────

// After cargo-ndk finishes two cleanups are needed:
//   1. strip .debug sections from the shell library (a debug build carries
//      ~880 MB of DWARF; the stripped runtime is ~55 MB),
//   2. delete the stray libgpui_mobile.so cargo-ndk copies out — gpui-mobile
//      is itself a cdylib, but its code is statically linked into
//      libstreamtube_android.so, so the extra .so would only bloat the APK.
fun Exec.postProcessRust() {
    doLast {
        val ndk = System.getenv("ANDROID_NDK_HOME") ?: "${System.getenv("HOME")}/Android/Sdk/ndk/27.2.12479018"
        val llvmStrip = "$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip"
        listOf("arm64-v8a", "x86_64").forEach { abi ->
            val so = File(rootDir, "app/src/main/jniLibs/$abi/libstreamtube_android.so")
            if (so.exists()) {
                ProcessBuilder(llvmStrip, "--strip-debug", so.absolutePath)
                    .inheritIO().start().waitFor()
            }
            File(rootDir, "app/src/main/jniLibs/$abi/libgpui_mobile.so").delete()
        }
    }
}

tasks.register<Exec>("buildRustDebug") {
    group = "rust"
    description = "Compile libstreamtube_android.so for arm64-v8a + x86_64 (debug) via cargo-ndk, strip it, drop strays."
    workingDir = rootProject.projectDir.parentFile // repo root
    commandLine(
        "cargo", "ndk",
        "-t", "arm64-v8a", "-t", "x86_64",
        "-P", "26",
        "-o", "android/app/src/main/jniLibs",
        "build", "-p", "streamtube-android",
    )
    postProcessRust()
}

tasks.register<Exec>("buildRustRelease") {
    group = "rust"
    description = "Compile libstreamtube_android.so for arm64-v8a + x86_64 (release) via cargo-ndk, drop strays."
    workingDir = rootProject.projectDir.parentFile // repo root
    commandLine(
        "cargo", "ndk",
        "-t", "arm64-v8a", "-t", "x86_64",
        "-P", "26",
        "-o", "android/app/src/main/jniLibs",
        "build", "--release", "-p", "streamtube-android",
    )
    postProcessRust()
}

tasks.register("buildAll") {
    group = "rust"
    description = "Build the Rust shell (debug) and then assemble the debug APK."
    dependsOn("buildRustDebug")
    finalizedBy(":app:assembleDebug")
}
