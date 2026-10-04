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
            // Our glGetString spoof (see gl-compat) reports 3.3 / GLSL 330.
            // LWJGL pointer checks. `NoChecks=true` (default) is needed for the 1.16 null fog
            // buffer, but turns any call to an unresolved GL function into a native
            // `SIGSEGV pc=0x0`. Pick `NoChecks=false` when diagnosing such a crash: the game
            // then throws a Java exception that names the exact GL/AL function.
            // Declared once, as a selectable, so it does not show up twice.
            selectable(
                key = "JAVA_TOOL_OPTIONS",
                items = RendererConfig.EnvItems(
                    defaultValue = "-Dorg.lwjgl.util.NoChecks=true",
                    values = listOf(
                        "-Dorg.lwjgl.util.NoChecks=true",
                        "-Dorg.lwjgl.util.NoChecks=false"
                    )
                )
            )

            // Override Zalith defaults that otherwise inject Mesa 4.6 + zink for
            // non-GL4ES renderers. Those contradict GLES passthrough and confuse
            // LWJGL / shader path selection.
            normal("MESA_GL_VERSION_OVERRIDE", "3.3")
            normal("MESA_GLSL_VERSION_OVERRIDE", "330")
            // Empty disables the zink loader override when the env is applied last.
            normal("MESA_LOADER_DRIVER_OVERRIDE", "")
            // Avoid treating our SO as a Mesa DRI driver.
            normal("LIB_MESA_NAME", "")

            // Renderer options. Each setting is declared exactly once: the previous build
            // sent RENDERER_BACKEND both as a fixed env var and as a selectable under a
            // different key, which is what made the backend appear twice in the list.
            // `vulkan` and `hybrid` cannot render the game yet (see vulkan-backend docs) and
            // the code logs why, then stays on GLES.
            selectable(
                key = "RENDERER_BACKEND",
                items = RendererConfig.EnvItems(
                    defaultValue = "gles",
                    values = listOf(
                        "gles",
                        "hybrid",
                        "vulkan",
                        "auto"
                    )
                )
            )
            // 1 = report OpenGL 3.3 / GLSL 330 so version checks pass; 0 = report the real
            // GLES strings (useful when diagnosing a driver-specific problem).
            // Explicit values rather than `toggleable`, because this variable defaults to
            // "on" in code: an unset value means spoof enabled, so a switch that removes the
            // variable when disabled could not turn it off.
            selectable(
                key = "RENDERER_SPOOF_GL",
                items = RendererConfig.EnvItems(
                    defaultValue = "1",
                    values = listOf(
                        "1",
                        "0"
                    )
                )
            )
            // Logs the last 16 forwarded GL calls whenever glGetError returns non-zero.
            // Minecraft only reports the numeric code, so this is what turns "OpenGL error
            // 1282" into a named call.
            selectable(
                key = "RENDERER_TRACE_GL",
                items = RendererConfig.EnvItems(
                    defaultValue = "0",
                    values = listOf(
                        "0",
                        "1"
                    )
                )
            ),
            selectable(
                key = "RENDERER_DEBUG",
                items = RendererConfig.EnvItems(
                    defaultValue = "0",
                    values = listOf(
                        "0",
                        "1"
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