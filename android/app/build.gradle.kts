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
        rendererId = "opengles3_rust",

        // Your actual native renderer library
        rendererGLPath = nativePath("librust_gl.so"),

        // RELATIVE name only (no nativePath). Zalith does:
        //   SDL_EGL_LIBRARY = "$nativeLibPath/$eglName"
        // If eglName is already absolute, the path is doubled and dlopen fails.
        // Relative "librust_gl.so" → correct pluginDir/librust_gl.so for SDL,
        // and POJAVEXEC_EGL falls back to system libEGL.so if relative dlopen fails.
        // MobileGL-style relative basename:
        // POJAVEXEC_EGL=librust_gl.so  SDL_EGL_LIBRARY=<pluginDir>/librust_gl.so
        // Relative basename only — Zalith sets SDL_EGL_LIBRARY = "$nativeLibPath/$eglName".
        // An absolute eglName gets doubled and dlopen fails.
        rendererEGLPath = "librust_gl.so",

        dlopenLibPaths = emptyList(),

        env = buildEnvs {
            // Real backend is GLES 3.x passthrough — do NOT claim Mesa/Zink/GL 4.6.
            normal("LIBGL_ES", "3")
            // Default stays gles: the GL entry points the game calls are served by the
            // GLES driver. `vulkan` and `hybrid` are selectable below but cannot render the
            // game yet (see vulkan-backend docs); the code logs why and stays on GLES.
            normal("RENDERER_BACKEND", "gles")
            // Our glGetString spoof (see gl-compat) reports 3.3 / GLSL 330.
            normal("RENDERER_SPOOF_GL", "1")
            normal("JAVA_TOOL_OPTIONS", "-Dorg.lwjgl.util.NoChecks=true")

            // Override Zalith defaults that otherwise inject Mesa 4.6 + zink for
            // non-GL4ES renderers. Those contradict GLES passthrough and confuse
            // LWJGL / shader path selection.
            normal("MESA_GL_VERSION_OVERRIDE", "3.3")
            normal("MESA_GLSL_VERSION_OVERRIDE", "330")
            // Empty disables the zink loader override when the env is applied last.
            normal("MESA_LOADER_DRIVER_OVERRIDE", "")
            // Avoid treating our SO as a Mesa DRI driver.
            normal("LIB_MESA_NAME", "")

            // Keep selectable backend for future vulkan work; default stays gles.
            selectable(
                key = "RENDERER_BACKEND_SELECT",
                items = RendererConfig.EnvItems(
                    defaultValue = "gles",
                    values = listOf(
                        "gles",
                        "vulkan",
                        "hybrid",
                        "auto"
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

        versionCode = 23
        versionName = "0.3.0"

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