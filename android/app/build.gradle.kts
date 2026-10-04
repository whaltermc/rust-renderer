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

        // RELATIVE name only (no nativePath). Zalith does:
        //   SDL_EGL_LIBRARY = "$nativeLibPath/$eglName"
        // If eglName is already absolute, the path is doubled and dlopen fails.
        // Relative "librust_gl.so" → correct pluginDir/librust_gl.so for SDL,
        // and POJAVEXEC_EGL falls back to system libEGL.so if relative dlopen fails.
        rendererEGLPath = "librust_gl.so",

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

        versionCode = 8
        versionName = "0.1.7"

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