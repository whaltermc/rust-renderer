import com.launchers_plugin.renderer.buildscript.RendererConfig
import com.launchers_plugin.renderer.buildscript.buildEnvs
import com.launchers_plugin.renderer.buildscript.buildJsonValue
import com.launchers_plugin.renderer.buildscript.nativePath
import com.launchers_plugin.renderer.buildscript.renderer

buildscript {
    repositories {
        maven("https://jitpack.io")
        google()
        mavenCentral()
    }

    dependencies {
        classpath("com.github.ZalithLauncher.RendererPlugin-v2:dsl:1.0.1")
    }
}

plugins {
    id("com.android.application")
}

apply(plugin = "com.launchers_plugin.renderer.dsl")

val pluginRendererConfig = buildJsonValue {
    renderer(
        displayName = "Rust Renderer",

        // This becomes POJAV_RENDERER
        rendererId = "rust_renderer",

        // Your actual native renderer library
        rendererGLPath = nativePath("librust_gl.so"),

        // If librust_gl.so provides EGL as well:
        rendererEGLPath = "libEGL.so",

        dlopenLibPaths = emptyList(),

        env = buildEnvs {
            normal("LIBGL_ES", "3")
            // Advertise desktop GL 3.2 so Minecraft version checks pass (passthrough GLES).
            normal("RENDERER_SPOOF_GL", "1")

            selectable(
                key = "RENDERER_BACKEND",
                items = RendererConfig.EnvItems(
                    defaultValue = "gles",
                    values = listOf(
                        "gles",
                        "vulkan"
                    )
                )
            )
        },

        minMCVer = null,
        maxMCVer = null
    )
}

android {
    namespace = "dev.rustrenderer.plugin"
    compileSdk = 34

    defaultConfig {
        applicationId = "dev.rustrenderer.plugin"

        minSdk = 26
        targetSdk = 34

        versionCode = 5
        versionName = "0.1.4"

        resValue(
            "string",
            "config",
            pluginRendererConfig
        )

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
}