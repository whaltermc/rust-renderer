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
        displayName = "RustGL",

        // This becomes POJAV_RENDERER
        rendererId = "opengles3_rust",

        // Your actual native renderer library
        rendererGLPath = nativePath("librust_gl.so"),

        // Relative basename only — Zalith sets SDL_EGL_LIBRARY = "$nativeLibPath/$eglName"
        rendererEGLPath = "librust_gl.so",

        dlopenLibPaths = emptyList(),

        env = buildEnvs {
            // Real backend is GLES 3.x passthrough
            normal("LIBGL_ES", "3")

            // Our glGetString spoof reports 3.3 / GLSL 330
            normal("MESA_GL_VERSION_OVERRIDE", "4.4")
            normal("MESA_GLSL_VERSION_OVERRIDE", "330")
            normal("MESA_LOADER_DRIVER_OVERRIDE", "")
            normal("LIB_MESA_NAME", "")

            // GLES is the only backend that currently draws Minecraft frames
            normal("RENDERER_BACKEND", "gles")

            // Always enabled so Iris/Minecraft sees OpenGL 4.4 Core Profile
            normal("RENDERER_SPOOF_GL", "1")

            // LWJGL pointer checks - NoChecks=true needed for 1.16 null fog buffer
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

            // Debug/trace selectables (optional for users)
            selectable(
                key = "RENDERER_TRACE_GL",
                items = RendererConfig.EnvItems(
                    defaultValue = "0",
                    values = listOf("0", "1")
                )
            )
            selectable(
                key = "RENDERER_DEBUG",
                items = RendererConfig.EnvItems(
                    defaultValue = "0",
                    values = listOf("0", "1")
                )
            )
        },

        minMCVer = null,
        maxMCVer = null
    )
}

tasks.register<Copy>("packageRustGlApk") {
    dependsOn("assembleDebug")
    from(layout.buildDirectory.file("outputs/apk/debug/app-debug.apk"))
    into(layout.buildDirectory.dir("outputs/apk"))
    rename { "RustGL.apk" }
}

tasks.register<Copy>("packageRustGlAar") {
    dependsOn(":renderer:assembleRelease")
    from(rootProject.layout.projectDirectory.dir("android/renderer/build/outputs/aar/renderer-release.aar"))
    into(layout.buildDirectory.dir("outputs/aar"))
    rename { "RustGL.aar" }
}

// Fixture test tasks
tasks.register("runFixtures") {
    group = "verification"
    description = "Run fixture tests on connected device/emulator"
    doLast {
        println("Running fixture tests...")
    }
}

tasks.register("runFixturesArm64", Exec::class) {
    group = "verification"
    description = "Run fixture tests on arm64-v8a"
    commandLine("adb", "shell", "am", "instrument", "-w",
        "-e", "abi", "arm64-v8a",
        "dev.rustrenderer.plugin.test/androidx.test.runner.AndroidJUnitRunner")
}

tasks.register("runFixturesArm32", Exec::class) {
    group = "verification"
    description = "Run fixture tests on armeabi-v7a"
    commandLine("adb", "shell", "am", "instrument", "-w",
        "-e", "abi", "armeabi-v7a",
        "dev.rustrenderer.plugin.test/androidx.test.runner.AndroidJUnitRunner")
}

tasks.register("runFixturesX86", Exec::class) {
    group = "verification"
    description = "Run fixture tests on x86"
    commandLine("adb", "shell", "am", "instrument", "-w",
        "-e", "abi", "x86",
        "dev.rustrenderer.plugin.test/androidx.test.runner.AndroidJUnitRunner")
}

tasks.register("runFixturesX86_64", Exec::class) {
    group = "verification"
    description = "Run fixture tests on x86_64"
    commandLine("adb", "shell", "am", "instrument", "-w",
        "-e", "abi", "x86_64",
        "dev.rustrenderer.plugin.test/androidx.test.runner.AndroidJUnitRunner")
}

tasks.named("check") {
    dependsOn("runFixtures")
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
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64")
        }

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildFeatures {
        resValues = true
    }

    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }

    // Ensure all ABIs are built for test APK
    splits {
        abi {
            isEnable = false
        }
    }
}
