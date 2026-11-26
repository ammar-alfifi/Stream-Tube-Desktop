// Settings for the Stream Tube Android shell.
//
// Minimal single-module Gradle project that packages the Rust native library
// (compiled via cargo-ndk) into an APK using NativeActivity.

pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "StreamTubeAndroid"
include(":app")
