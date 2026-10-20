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

// One APK per architecture, plus a universal one with all of them:
//   ./build.sh assembleRelease                         (arm64-v8a, the default)
//   ./build.sh assembleRelease -PtargetAbi=armeabi-v7a
//   ./build.sh assembleRelease -PtargetAbi=x86_64
//   ./build.sh assembleRelease -PtargetAbi=universal
// after building the same ABIs with cargo-ndk (the error below prints the exact commands).
val appVersion = "0.3.5"
val allAbis = listOf("arm64-v8a", "armeabi-v7a", "x86_64")
val rustTargetOf = mapOf(
    "arm64-v8a" to "aarch64-linux-android",
    "armeabi-v7a" to "armv7-linux-androideabi",
    "x86_64" to "x86_64-linux-android",
)
val targetAbi = providers.gradleProperty("targetAbi").orElse("arm64-v8a").get()
val universal = targetAbi == "universal"
require(universal || targetAbi in allAbis) { "ABI no soportada: $targetAbi" }
val buildAbis = if (universal) allAbis else listOf(targetAbi)
val emulatorSos = buildAbis.associateWith { abi ->
    rootProject.file("../src/OpenGMK/target/${rustTargetOf.getValue(abi)}/release/libgm8emulator.so")
}
val copyEmulatorSo by tasks.registering {
    inputs.files(emulatorSos.values.map { it }).optional()
    outputs.dirs(buildAbis.map { layout.projectDirectory.dir("src/main/jniLibs/$it") })
    doLast {
        for (abi in buildAbis) {
            val so = emulatorSos.getValue(abi)
            if (!so.isFile) error("falta $so; compila con: cargo ndk -t $abi --platform 29 build -p gm8emulator --release")
            val magic = so.inputStream().use { it.readNBytes(4) }
            if (magic.size < 4 || magic[0] != 0x7F.toByte() || magic[1] != 'E'.code.toByte() || magic[2] != 'L'.code.toByte() || magic[3] != 'F'.code.toByte()) {
                error("$so no es un ELF valido (¿quedó a cero tras un corte?)")
            }
            val out = layout.projectDirectory.dir("src/main/jniLibs/$abi").asFile.apply { mkdirs() }.resolve("libgm8emulator.so")
            so.copyTo(out, overwrite = true)
        }
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
        versionName = appVersion
        ndk { abiFilters += buildAbis }
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

// Name every release APK after the full ABI (arm64-v8a, not just "arm64"), so the files that come
// out of the build are the ones that get published.
androidComponents {
    onVariants { variant ->
        if (variant.buildType != "release") return@onVariants
        variant.outputs.forEach { output ->
            output.outputFileName.set("pokemon-reloaded-$appVersion-${if (universal) "universal" else targetAbi}.apk")
        }
    }
}

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
