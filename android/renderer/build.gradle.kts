plugins {
    id("com.android.library")
}

android {
    namespace = "dev.rustrenderer.plugin"
    compileSdk = 34

    defaultConfig {
        minSdk = 26
        targetSdk = 34

        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64")
        }
    }

    buildFeatures {
        resValues = true
    }

    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }

    libraryVariants.all {
        buildConfigField("String", "LIBRARY_NAME", "\"rust_gl\"")
    }
}
