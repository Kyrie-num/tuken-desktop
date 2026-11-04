import groovy.json.JsonSlurper

plugins {
    id("com.android.application")
}

// The version is 兔啃's own (package.json): 0.4.1 → versionCode 401.
val appVersion = (JsonSlurper().parse(rootDir.resolve("../package.json")) as Map<*, *>)["version"] as String
val (major, minor, patch) = appVersion.split(".").map { it.toInt() }

android {
    namespace = "site.tuken.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "site.tuken.app"
        minSdk = 24
        // 34, not 35: Android 15 would draw the page under the system bars.
        targetSdk = 34
        versionCode = major * 10000 + minor * 100 + patch
        versionName = appVersion
    }

    // CI signs with the secrets ANDROID_KEYSTORE (base64), ANDROID_KEYSTORE_PASSWORD,
    // ANDROID_KEY_ALIAS and ANDROID_KEY_PASSWORD; updates need the same key.
    signingConfigs {
        create("release") {
            val store = System.getenv("ANDROID_KEYSTORE_FILE")
            if (store != null) {
                storeFile = file(store)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("release").takeIf { it.storeFile != null }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
