import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

// The release key lives beside the project and is never committed (`keystore.properties` is
// ignored), so builds on this machine are signed while the repo stays free of it.
val keystoreProperties = Properties().apply {
    val file = rootProject.file("keystore.properties")
    if (file.exists()) file.inputStream().use { load(it) }
}

// One APK per architecture. arm64-v8a is the default; the others are built with
//   ./build.sh assembleRelease -PtargetAbi=armeabi-v7a
//   ./build.sh assembleRelease -PtargetAbi=x86_64
// after building the same ABI with cargo-ndk (the error below prints the exact command).
val targetAbi = providers.gradleProperty("targetAbi").orElse("arm64-v8a").get()
val rustTarget = when (targetAbi) {
    "arm64-v8a" -> "aarch64-linux-android"
    "armeabi-v7a" -> "armv7-linux-androideabi"
    "x86_64" -> "x86_64-linux-android"
    else -> error("ABI no soportada: $targetAbi")
}
val emulatorSo = rootProject.file("../src/OpenGMK/target/$rustTarget/release/libgm8emulator.so")
val jniLibDir = layout.projectDirectory.dir("src/main/jniLibs/$targetAbi")
val copyEmulatorSo by tasks.registering {
    inputs.file(emulatorSo).withPropertyName("emulatorSo").optional()
    outputs.dir(jniLibDir)
    doLast {
        if (!emulatorSo.isFile) error("falta ${emulatorSo}; compila con: cargo ndk -t $targetAbi --platform 29 build -p gm8emulator --release")
        val magic = emulatorSo.inputStream().use { it.readNBytes(4) }
        if (magic.size < 4 || magic[0] != 0x7F.toByte() || magic[1] != 'E'.code.toByte() || magic[2] != 'L'.code.toByte() || magic[3] != 'F'.code.toByte()) {
            error("${emulatorSo} no es un ELF valido (¿quedó a cero tras un corte?)")
        }
        val out = jniLibDir.asFile.apply { mkdirs() }.resolve("libgm8emulator.so")
        emulatorSo.copyTo(out, overwrite = true)
    }
}

android {
    namespace = "com.klisee.reloaded"
    compileSdk = 37

    defaultConfig {
        applicationId = "com.klisee.reloaded"
        minSdk = 29
        targetSdk = 36
        versionCode = 11
        versionName = "0.3.5"
        ndk { abiFilters += targetAbi }
    }

    signingConfigs {
        create("release") {
            if (!keystoreProperties.isEmpty) {
                storeFile = rootProject.file(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("storePassword")
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // Signed with the project's own key when it is present, so the APK shipped as a release
            // is a real release build and not a debug one.
            signingConfig = if (!keystoreProperties.isEmpty) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures { compose = true }

    packaging { jniLibs { useLegacyPackaging = false } }
}

tasks.named("preBuild") { dependsOn(copyEmulatorSo) }

dependencies {
    implementation("androidx.core:core-ktx:1.19.0")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.11.0")
    implementation("androidx.compose.ui:ui:1.13.0-alpha01")
    implementation("androidx.compose.ui:ui-graphics:1.13.0-alpha01")
    implementation("androidx.compose.foundation:foundation:1.13.0-alpha01")
    implementation("androidx.compose.runtime:runtime:1.13.0-alpha01")
    implementation("androidx.compose.material3:material3:1.5.0-alpha28")
    implementation("androidx.compose.material:material-icons-extended:1.7.6")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
}
