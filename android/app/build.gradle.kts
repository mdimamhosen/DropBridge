plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// Release signing comes from env vars (CI secrets); without them release builds use the debug key.
val ks: String? = System.getenv("DROPBRIDGE_KEYSTORE")

android {
    namespace = "dev.dropbridge.app"
    compileSdk = 35
    defaultConfig {
        applicationId = "dev.dropbridge.app"
        minSdk = 29
        targetSdk = 35
        versionCode = (System.getenv("DROPBRIDGE_VERSION_CODE") ?: "1").toInt()
        versionName = System.getenv("DROPBRIDGE_VERSION") ?: "0.1.0"
    }
    signingConfigs {
        if (ks != null) create("release") {
            storeFile = file(ks)
            storePassword = System.getenv("DROPBRIDGE_KEYSTORE_PASSWORD")
            keyAlias = System.getenv("DROPBRIDGE_KEY_ALIAS")
            keyPassword = System.getenv("DROPBRIDGE_KEY_PASSWORD")
        }
    }
    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.12.01"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("net.java.dev.jna:jna:5.15.0@aar")
    implementation("com.journeyapps:zxing-android-embedded:4.3.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")
}
