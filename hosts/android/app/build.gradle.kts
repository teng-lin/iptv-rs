plugins {
    id("com.github.jk1.dependency-license-report") version "3.1.5"
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// Relative to this Gradle root (hosts/android): the nsh checkout next to iptv-rs.
val nshJava: File = rootProject.file(providers.gradleProperty("nsh.java").get())
// Filled by tools/sync-host.sh from the NSH build of the Rust crate.
val nshOut = layout.buildDirectory.dir("nsh").get().asFile

android {
    namespace = "com.hantekllc.iptvrs.host"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.hantekllc.iptvrs"
        minSdk = 26
        targetSdk = 35
        versionCode = System.getenv("GITHUB_RUN_NUMBER")?.toInt() ?: 1
        versionName = System.getenv("VERSION") ?: "0.1.0"
        ndk { abiFilters += listOf("x86_64", "arm64-v8a", "armeabi-v7a") }
    }

    signingConfigs {
        if (System.getenv("KEYSTORE_PATH") != null) {
            create("distribution") {
                storeFile = file(System.getenv("KEYSTORE_PATH"))
                storePassword = System.getenv("STORE_PASSWORD")
                keyAlias = System.getenv("KEY_ALIAS")
                keyPassword = System.getenv("KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        // What a viewer installs: not debuggable, so ART compiles the host's
        // and Media3's Java instead of interpreting it (a debuggable APK runs
        // it in the slow interpreter: seconds of a cold start on a TV box;
        // docs/perf.md). Signed with the debug key, so it installs over a
        // debug build and keeps the app's data. `profileable` (manifest)
        // keeps simpleperf working on it.
        getByName("release") {
            isMinifyEnabled = false
            signingConfig = if (System.getenv("IPTV_UNSIGNED_RELEASE") == "1") null
                else signingConfigs.findByName("distribution") ?: signingConfigs.getByName("debug")
        }
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
    lint {
        abortOnError = true
        checkReleaseBuilds = true
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
    implementation("androidx.core:core:1.15.0")
    val media3 = "1.5.1"
    implementation("androidx.media3:media3-exoplayer:$media3")
    implementation("androidx.media3:media3-exoplayer-hls:$media3")
    implementation("androidx.media3:media3-exoplayer-dash:$media3")
    implementation("androidx.media3:media3-ui:$media3")
}

licenseReport {
    renderers = arrayOf(
        com.github.jk1.license.render.InventoryHtmlReportRenderer(),
        com.github.jk1.license.render.JsonReportRenderer(),
    )
    configurations = arrayOf("releaseRuntimeClasspath")
    outputDir = layout.buildDirectory.dir("reports/dependency-license").get().asFile.path
}
