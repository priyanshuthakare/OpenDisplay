plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "org.usbdisplay.client"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.usbdisplay.client"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        resources {
            excludes += "META-INF/versions/9/OSGI-INF/MANIFEST.MF"
        }
    }
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    implementation("com.google.zxing:core:3.5.3")
    // Self-signed ECDSA P-256 cert generation for WiFi TLS (PR-3).
    // AndroidKeyStore cannot mint TLS server certs directly, so we use
    // BouncyCastle in-memory + persist DER in private prefs (documented in docs/wifi.md).
    implementation("org.bouncycastle:bcpkix-jdk18on:1.78.1")
    testImplementation("junit:junit:4.13.2")
}
