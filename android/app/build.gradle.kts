import com.launchers_plugin.renderer.buildscript.RendererConfig
import com.launchers_plugin.renderer.buildscript.buildEnvs
import com.launchers_plugin.renderer.buildscript.buildJsonValue
import com.launchers_plugin.renderer.buildscript.nativePath
import com.launchers_plugin.renderer.buildscript.renderer

// Modeled on MobileGL's android-plugin/app/build.gradle.kts (which uses dsl 1.0-alpha6).
// 1.0.1 is the latest release listed in the RendererPlugin-v2 README. If the launcher
// misbehaves, try 1.0-alpha6 to match MobileGL exactly.
buildscript {
    repositories { maven("https://jitpack.io") }
    dependencies {
        classpath("com.github.ZalithLauncher.RendererPlugin-v2:dsl:1.0.1")
    }
}

plugins {
    id("com.android.application")
}

apply(plugin = "com.launchers_plugin.renderer.dsl")

// Serialized to JSON and written to @string/config, which the manifest's fclPlugin_V2 points at.
val pluginRendererConfig = buildJsonValue {
    renderer(
        displayName = "Rust Renderer",
        rendererId = "opengles3",
        rendererGLPath = nativePath("librust_gl.so"),
        rendererEGLPath = nativePath("librust_gl.so"),
        dlopenLibPaths = emptyList(),
        env = buildEnvs {
            normal("LIBGL_ES", "3")
            selectable(
                key = "RENDERER_BACKEND",
                items = RendererConfig.EnvItems("auto", listOf("gles", "vulkan")),
            )
        },
        minMCVer = null,
        maxMCVer = null,
    )
}

android {
    namespace = "dev.rustrenderer.plugin"
    compileSdk = 34

    defaultConfig {
        applicationId = "dev.rustrenderer.plugin"
        minSdk = 26
        targetSdk = 34
        versionCode = 4
        versionName = "0.1.3"
        resValue("string", "config", pluginRendererConfig)
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    buildFeatures { resValues = true }

    buildTypes {
        getByName("release") {
            isDebuggable = false
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    packaging {
        jniLibs { useLegacyPackaging = true }
    }
}
