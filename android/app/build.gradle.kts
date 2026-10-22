plugins {
    id("com.android.application")
}

fun quoted(value: String): String =
    "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"") + "\""

fun property(name: String, fallback: String): String =
    providers.gradleProperty(name).orElse(fallback).get()

android {
    namespace = "com.intimeui.client.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.intimeui.client"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "1.0.0"

        buildConfigField("String", "API_BASE_URL",
            quoted(property("intimeui.apiBaseUrl", "https://api.intimeui.example")))
        buildConfigField("String", "AUTH_TOKEN",
            quoted(property("intimeui.authToken", "dev:usr_9527")))
        buildConfigField("String", "AUTH_USER_ID",
            quoted(property("intimeui.userId", "usr_9527")))
        buildConfigField("String", "REPLAY_SECRET",
            quoted(property("intimeui.replaySecret", "")))
        buildConfigField("boolean", "DEMO_MODE",
            property("intimeui.demoMode", "true"))
        buildConfigField("String", "SCENE_CODE",
            quoted(property("intimeui.sceneCode", "ORDER_DETAIL_PAGE")))
        buildConfigField("String", "DEMO_ASSET",
            quoted(property("intimeui.demoAsset", "demo/order-detail.html")))
    }

    buildFeatures {
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    lint {
        abortOnError = true
        checkReleaseBuilds = false
    }
}

dependencies {
    implementation(project(":core"))
    implementation("com.google.code.gson:gson:2.10.1")
}
