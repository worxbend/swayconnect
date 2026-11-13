plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// No serialization plugin: every serializable class lives in :protocol (docs/DESIGN.md 8.2).
// Lint warnings do not fail the build in stage 1 (docs/DESIGN.md 8.1).
android {
    namespace = "com.swayconnect.android"
    compileSdk = 37

    defaultConfig {
        applicationId = "com.swayconnect.android"
        minSdk = 29
        // 36, not 37: targeting 37 needs the runtime permission ACCESS_LOCAL_NETWORK (docs/DESIGN.md 8.1).
        targetSdk = 36
        // The release workflow passes the tag: -PversionName=0.2.0 -PversionCode=<run number>.
        versionCode = (findProperty("versionCode") as String?)?.toInt() ?: 1
        versionName = (findProperty("versionName") as String?) ?: "0.1.0"
    }

    // Release signing comes only from the environment (CI secrets or a local release.env).
    // Without it, assembleRelease produces an unsigned APK that Android will not install.
    val releaseKeystore = System.getenv("SWAYCONNECT_KEYSTORE")
    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = System.getenv("SWAYCONNECT_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("SWAYCONNECT_KEY_ALIAS")
                keyPassword = System.getenv("SWAYCONNECT_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            // shortcut: no R8 yet; kotlinx.serialization keep rules are untested. Enable once a
            // release build with minify passes the protocol tests on a device.
            isMinifyEnabled = false
            if (releaseKeystore != null) signingConfig = signingConfigs.getByName("release")
        }
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

dependencies {
    implementation(project(":protocol"))

    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.material3)

    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.kotlinx.coroutines.android)

    testImplementation(libs.junit)
}
