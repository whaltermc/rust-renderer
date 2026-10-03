# Minecraft Rust Renderer — ZalithLauncher plugin (Phase 1 scaffold)

Implements the first slice of `minecraft-rust-android-renderer-spec.md`, packaged as a
ZalithLauncher 2 renderer plugin APK.

## Status (honest)

| Piece | State |
|---|---|
| `renderer-core` (Backend trait, config, GL error state) | written, unit tests included |
| `gles-backend` (dlopen system GLES, 7 entry points) | written, **not compiled or run** |
| `vulkan-backend` | **not implemented** — probe always fails, `auto` falls back to GLES |
| `gl-compat` cdylib `librust_gl.so` (7 GL functions + `glXGetProcAddress`) | written, **not compiled or run** |
| Plugin APK manifest | written from public plugin docs, **unverified in the launcher** |
| Desktop-GL translation, shader translation, textures, buffers, FBOs, JNI, EGL | **not started** |

**Minecraft will not launch with this yet.** It needs desktop OpenGL 3.2+ (buffers, VAOs,
shaders, textures, FBOs...), which is spec phases 2-4. Today the plugin only proves the
plugin plumbing: install, appear in Zalith's renderer list, load, log the GPU.

## Build

```bash
rustup target add aarch64-linux-android
cargo install cargo-ndk
export ANDROID_NDK_HOME=/path/to/ndk
./build.sh
```

Output: `android/app/build/outputs/apk/debug/app-debug.apk`. Install it, then pick
"Rust Renderer" in ZalithLauncher's renderer list. Logs: `adb logcat -s RustRenderer`.

## Things to verify first (I could not)

1. `pojavEnv` value and whether `libEGL.so` is accepted as the EGL field — compare with
   https://github.com/ZalithLauncher/RendererPlugin.
2. Whether the launcher resolves GL symbols via `glXGetProcAddress` from your `.so`.
3. `cargo ndk build` — the code was written without a Rust toolchain available.

## Next steps (spec order)

Phase 2 buffers/textures/shaders in core -> Phase 3 GL compat (state, VAO, draws) ->
Phase 4 shader translation (naga) -> EGL shim -> Vulkan.

## Building the plugin APK (V2 format)

The plugin now uses ZalithLauncher's RendererPlugin-v2 Gradle DSL (same as MobileGL's
plugin). Gradle/AGP does not run well on Termux, so build it either:

- on GitHub: push this folder to a repo; `.github/workflows/build.yml` builds the APK
  (download it from the Actions run's artifacts), or
- on a PC with the Android SDK/NDK: `./build.sh`.

The old Termux `aapt2` script produces a legacy-only APK and may not be detected.
