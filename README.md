# RustGL — ZalithLauncher 2 Minecraft Renderer

A desktop-GL-to-GLES translation layer packaged as a ZalithLauncher 2 renderer plugin for Minecraft.

## Project Status

| Component | Status |
|-----------|--------|
| Plugin APK (ZalithLauncher V2) | ✅ Builds and installs |
| `renderer-core` | ✅ Backend trait, config, error state, unit tests |
| `backend::gles` | ✅ Full GLES 3.0+ backend with dlopen |
| `backend::vulkan` | Device discovery only — no rendering path yet |
| `gl-compat` (`librust_gl.so`) | ✅ GLES 3.x + OpenGL 3.3 compat layer |
| Shader translator | ✅ Desktop GLSL → GLSL ES 3.00 |
| Format translate | ✅ BGRA↔RGBA, depth formats, clamp-to-border |
| OpenGL 3.3 core API | 669 entry points (complete vs 3.0–3.3 core) |
| Vulkan path | Device discovery only — no rendering path |

**Verified (host, Mesa llvmpipe):**
- 130 unit tests pass
- `glsmoke` — 47 checks pass (52 with contract mode)
- 1.17 main menu retrace: SSIM 0.999962 ✅
- 1.21.4-shaders (Iris/BSL): SSIM 0.999934, 0 shader failures ✅
- 1.21.4-world retrace: SSIM 0.999976
- 26.3 shader pack: FAIL (known shader translation gaps)

## Quick Start

```bash
# Prerequisites
rustup target add aarch64-linux-android
cargo install cargo-ndk
export ANDROID_NDK_HOME=/path/to/ndk

# Build
./build.sh

# Output: android/app/build/outputs/apk/debug/app-debug.apk
```

Or download from GitHub Actions artifacts (`.github/workflows/build.yml`).

## Install

1. Install APK on device
2. Open ZalithLauncher 2 → Renderer list → **RustGL**
3. Set env vars in ZalithLauncher renderer settings:
   - `RENDERER_SPOOF_GL=1` (default on) — advertises OpenGL 3.3 Core
   - `RENDERER_DEBUG=1` for verbose logging
   - `RENDERER_TRACE_GL=1` for GL call tracing

Logs: `adb logcat -s RustRenderer RendererV2Plugin`

## Supported Minecraft Versions

| Version | Status |
|---------|--------|
| Vanilla 1.16 | ✅ Works (earlier build) |
| Vanilla 1.12–1.15 | Likely (fixed-function path) |
| Vanilla 1.17–1.20 | Partial — modern shaders via GLSL rewrite |
| Vanilla 1.21.4 | GUI + world retrace pass; device launch unverified |
| Sodium | **Out of scope** — requires GL 4.5+ |
| Iris / shader packs | ❌ Not working — 13 shader compile failures |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Minecraft (LWJGL)                        │
├─────────────────────────────────────────────────────────────┤
│  glGetString / glGetProcAddress                             │
├─────────────────────────────────────────────────────────────┤
│  librust_gl.so (gl-compat) — OpenGL 3.3 → GLES 3.x shim    │
├─────────────────────────────────────────────────────────────┤
│         shader-translate (GLSL 330 → ES 300)                │
├─────────────────────────────────────────────────────────────┤
│              backend::gles (GLES 3.0+ dlopen)               │
├─────────────────────────────────────────────────────────────┤
│              System GLES driver (Mali/Adreno/etc.)          │
└─────────────────────────────────────────────────────────────┘
```

**Key crates:**
- `renderer-core` — Backend trait, config, error handling
- `backend::gles` — GLES 3.0+ backend (dlopen)
- `backend::vulkan` — Device discovery only (no render path)
- `gl-compat` — OpenGL 3.3 → GLES 3.x translation layer
- `shader-translate` — GLSL 330 → GLSL ES 300
- `format-translate` — BGRA↔RGBA, depth formats, swizzles
- `shader-translate` — Desktop GLSL → GLSL ES 300
- `format-translate` — Format/format swizzles, depth formats

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RENDERER_BACKEND` | `gles` | `gles` \| `hybrid` \| `vulkan` \| `auto` |
| `RENDERER_SPOOF_GL` | `1` | Advertise OpenGL 3.3 Core (always on) |
| `RENDERER_DEBUG` | `0` | Enable verbose logging |
| `RENDERER_TRACE_GL` | `0` | Trace GL calls (1=ring, all=full) |
| `RENDERER_TRACE_GL=all` | — | Full call sequence for debugging |

## Debugging

```bash
# Enable debug logs
adb logcat -c
adb logcat -s RustRenderer RendererV2Plugin > mc.log

# Watch for:
# [GLBridge] Missing entry point: <name>   → missing export
# [GLCompat]                               → shim taking unsupported path
# GL_INVALID_*                              → driver rejected call
```

## Credits

- **WhalterMC** — Project maintainer
- **MobileGL** — Reference GLES implementation (inspiration for dispatch)
- **naga** — GLSL → SPIR-V compilation (Vulkan backend, optional)
- **glslang** — GLSL → SPIR-V (vendored, Apache-2.0)
- **apitrace** — Trace capture/replay for testing
- **Mesa/llvmpipe** — Software rasteriser for CI testing
- **Mesa/ANGLE** — Translation layer testing (lavapipe)
- **apitrace** — Trace capture/replay for regression testing
- **naga** — GLSL → SPIR-V compilation (Vulkan backend)
- **libloading** — Dynamic library loading
- **ash** — Vulkan bindings (unused in current render path)
- **log / android_logger** — Logging

## License

MIT License — see [LICENSE](LICENSE) for details.

> **Honesty policy:** Do not claim Minecraft compatibility until a concrete version has been tested and logged. See `SUPPORT.md` for current support matrix.