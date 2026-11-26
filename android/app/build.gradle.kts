// App module build.gradle.kts for the Stream Tube Android shell.
//
// Packages the pre-compiled Rust native library (libstreamtube_android.so,
// built via cargo-ndk) into an APK hosted by NativeActivity (GpuiActivity).
// The .so must exist in app/src/main/jniLibs/<abi>/ before assembleDebug;
// see the buildRust* tasks in the root build.gradle.kts.

import java.io.File
import java.util.Properties

plugins {
    id("com.android.application")
}

val releaseKeyProperties = Properties().apply {
    val file = File(System.getProperty("user.home"), ".android/streamtube-release.properties")
    if (file.isFile) file.inputStream().use { load(it) }
}

android {
    namespace = "dev.streamtube.android"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.streamtube.android"
        minSdk = 26          // Vulkan 1.0 is mandatory from API 26+
        targetSdk = 35
        versionCode = 2
        versionName = "0.7.5-alpha.2"

        ndk {
            // armv7 is out of scope for v1 (plan2.md Track G)
            abiFilters += listOf("arm64-v8a", "x86_64")
        }

        // Forward the library name to the manifest via a placeholder.
        // Must match the cdylib name WITHOUT the "lib" prefix / ".so" suffix.
        manifestPlaceholders["nativeLibraryName"] = "streamtube_android"
    }

    signingConfigs {
        if (releaseKeyProperties.containsKey("storeFile")) {
            create("release") {
                storeFile = file(releaseKeyProperties.getProperty("storeFile"))
                storePassword = releaseKeyProperties.getProperty("storePassword")
                keyAlias = releaseKeyProperties.getProperty("keyAlias")
                keyPassword = releaseKeyProperties.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.findByName("release")
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
        debug {
            isDebuggable = true
            isJniDebuggable = true
        }
    }

    // No CMake / ndk-build: the native library is compiled externally via
    // cargo-ndk into jniLibs. Leave the built-in native build system disabled.
    externalNativeBuild {
        // Intentionally left empty.
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }

    // Tell Gradle where the pre-built .so files live.
    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    packaging {
        // Native libraries stay uncompressed for 16 KB page alignment. They
        // are stripped by scripts/build-android-release-arm64.sh before Gradle
        // packages them; keeping debug symbols here used to add ~14 MB of
        // symtab/strtab to every Rust .so.
        jniLibs {
            useLegacyPackaging = false
        }
    }

    lint {
        abortOnError = false
        checkReleaseBuilds = false
    }
}

dependencies {
    // AndroidX core for NotificationCompat (GpuiNotifications)
    implementation("androidx.core:core:1.12.0")
    // AndroidX SplashScreen compat (GpuiActivity holds the splash until native init)
    implementation("androidx.core:core-splashscreen:1.0.1")
    // AndroidX Media for MediaSessionCompat (GpuiMediaSession)
    implementation("androidx.media:media:1.7.1")
}
