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
            abiFilters += "arm64-v8a"
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
