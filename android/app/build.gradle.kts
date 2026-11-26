plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "org.roynit.music"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.roynit.music"
        // The Rust core links Android's AAudio, which arrived with Android 8.
        minSdk = 26
        targetSdk = 35
        versionCode = 7
        versionName = "0.1.6"
    }

    // Phones are arm64; the x86_64 build is only for emulators. One small APK per CPU.
    splits { abi { isEnable = gradle.startParameter.taskNames.any { it.contains("Release") }; reset(); include("arm64-v8a", "x86_64"); isUniversalApk = false } }

    // The key stays out of the repository: pass -Proy.keystore=... -Proy.storePass=... -Proy.keyAlias=... -Proy.keyPass=...
    signingConfigs {
        if (project.hasProperty("roy.keystore")) {
            create("release") {
                storeFile = file(project.property("roy.keystore") as String)
                storePassword = project.property("roy.storePass") as String
                keyAlias = project.property("roy.keyAlias") as String
                keyPassword = project.property("roy.keyPass") as String
            }
        }
    }

    buildTypes {
        release {
            signingConfigs.findByName("release")?.let { signingConfig = it }
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    packaging { jniLibs { useLegacyPackaging = false } }
}

dependencies {
    // Only the audio side of Media3: no video, no UI widgets, no extra codecs.
    implementation("androidx.media3:media3-exoplayer:1.6.1")
    implementation("androidx.media3:media3-session:1.6.1")
    // SoundCloud plays as a segmented (HLS) stream.
    implementation("androidx.media3:media3-exoplayer-hls:1.6.1")
    // FileProvider, to hand a downloaded update to the installer.
    implementation("androidx.core:core:1.15.0")
}
