plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val nshJava: String = providers.gradleProperty("nsh.java").get()
// Filled by tools/sync-host.sh from the NSH build of the Rust crate.
val nshOut = layout.buildDirectory.dir("nsh").get().asFile

android {
    namespace = "com.hantekllc.iptvrs.host"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.hantekllc.iptvrs"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("x86_64", "arm64-v8a") }
    }

    sourceSets["main"].java.srcDirs("src/main/kotlin", "src/main/java", nshJava)
    sourceSets["main"].jniLibs.srcDirs(File(nshOut, "jniLibs"))
    sourceSets["main"].assets.srcDirs(File(nshOut, "assets"))

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }

    packaging { jniLibs { useLegacyPackaging = false } }
    lint { abortOnError = false }
}

dependencies {
    val media3 = "1.5.1"
    implementation("androidx.media3:media3-exoplayer:$media3")
    implementation("androidx.media3:media3-exoplayer-hls:$media3")
    implementation("androidx.media3:media3-ui:$media3")
}
