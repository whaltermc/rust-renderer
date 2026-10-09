# RustGL — ZalithLauncher 2 Minecraft Renderer

A desktop-GL-to-GLES translation layer packaged as a ZalithLauncher 2 renderer plugin for Minecraft.

## Project Status

| Component | Status |
|-----------|--------|
| Plugin APK (ZalithLauncher V2) | ✅ Builds and installs |
| `renderer-core` | ✅ Backend trait, config, error state, unit tests |
| `backend::gles` | ✅ Full GLES 3.0+ backend with dlopen |
| `backend::directes` | ✅ Direct ES 3.x backend (bypasses GL compat layer) |
| `backend::vulkan` | ✅ Discovery + pipeline path (triangle PoC) |
| `backend::directvk` | ✅ DirectVK rendering backend with Vulkan pipeline |
| `backend::angel` | ✅ ANGLE driver detection and configuration |
| `gl-compat` (`librust_gl.so`) | Partial — GLES 3.x + OpenGL 4.5 compatibility path |
| Shader translator | ✅ Desktop GLSL → GLSL ES 3.00 / GL 4.5 core |
| `shader-manager` | ✅ Parallel shader translation, caching, pack management (1.21.4-26.4) |
| Format translate | ✅ BGRA↔RGBA, depth formats, clamp-to-border |
| OpenGL 3.3 core API | 669 entry points (complete vs 3.0–3.3 core) |
| Multi-threaded pipeline | ✅ Rayon-based parallel shader translation |

**Verified (host, Mesa llvmpipe):**
- 130 unit tests pass
- `glsmoke` — 47 checks pass (52 with contract mode)
- 1.17 main menu retrace: SSIM 0.999962 ✅
- 1.21.4-shaders (Iris/BSL): SSIM 0.999934, 0 shader failures ✅
- 1.21.4-world retrace: SSIM 0.999976
- 26.3 shader pack: SSIM 0.858 (286K mismatch pixels) — modern shader translation gaps remain

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
   - `RENDERER_SPOOF_GL=1` (default on) — advertises OpenGL 4.5 Core
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
| Vanilla 1.21.5–26.2 | Translation path in place; device launch unverified |
| 26.3 improved transparency | Partial — SSIM 0.869875, translation gaps remain |
| Sodium | **Out of scope** — requires GL 4.5+ |
| Iris / shader packs | ✅ 1.21.4 Iris/BSL passes; 26.3 needs more translation work |

## Completed Tasks

- [x] Separate `shader-manager` module (pack discovery, caching, parallel pipeline)
- [x] Separate `backend` module (DirectES, DirectVK, ANGEL, GLES, Vulkan)
- [x] Separate `shader-translate` module (desktop GLSL → GLSL ES / GL 4.5 core)
- [x] Separate `gl-compat` module (desktop GL compatibility layer)
- [x] Separate `format-translate` module (BGRA↔RGBA, depth formats)
- [x] Modern shader pack support (Complementary, Derivative, Bliss, BSL, SEUS)
- [x] Multi-threaded shader translation pipeline (rayon-based)
- [x] 1.21.4-26.4 shader token rewrites (PBR, volumetric, shadow, SSR, TAA, biome)
- [x] DirectES backend (bypasses GL compat layer)
- [x] DirectVK backend (Vulkan pipeline path)
- [x] ANGLE driver support (Vulkan/D3D11/D3D12/GL/SwiftShader)
- [x] 26.3 fixture replay: SSIM 0.858 (286K mismatch pixels) — **known gaps remain**

```
┌─────────────────────────────────────────────────────────────┐
│                    Minecraft (LWJGL)                        │
├─────────────────────────────────────────────────────────────┤
│  glGetString / glGetProcAddress                             │
├─────────────────────────────────────────────────────────────┤
│  librust_gl.so (gl-compat) — OpenGL 4.5 → GLES 3.x shim    │
├─────────────────────────────────────────────────────────────┤
│         shader-translate (GLSL 330 → ES 300)                │
│         shader-manager (1.21.4-26.4 parallel pipeline)      │
├─────────────────────────────────────────────────────────────┤
│  backend::directes | backend::gles | backend::directvk      │
├─────────────────────────────────────────────────────────────┤
│  backend::angel (ANGLE) | System GLES driver (Mali/Adreno) │
└─────────────────────────────────────────────────────────────┘
```

**Key crates:**
- `renderer-core` — Backend trait, config, error handling
- `backend::gles` — GLES 3.0+ backend (dlopen)
- `backend::directes` — Direct ES 3.x backend (no GL compat layer)
- `backend::vulkan` — Vulkan device discovery
- `backend::directvk` — DirectVK Vulkan rendering backend
- `backend::angel` — ANGLE driver support (Vulkan/D3D11/D3D12/GL/SwiftShader)
- `gl-compat` — OpenGL 4.5 → GLES 3.x translation layer (GLES driver) / desktop GL 4.5 core
- `shader-translate` — Desktop GLSL → GLSL ES 3.00 (GLES) / GL 4.5 core (desktop)
- `shader-manager` — Modern shader pack manager (Complementary, Derivative, Bliss, BSL, SEUS)
- `format-translate` — BGRA↔RGBA, depth formats, clamp-to-border

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RENDERER_BACKEND` | `gles` | `gles` \| `hybrid` \| `vulkan` \| `auto` |
| `RENDERER_SPOOF_GL` | `1` | Advertise OpenGL 4.5 Core (always on) |
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

## Remaining Work

- 26.3 shader pack translation gaps (SSIM 0.858, 286K mismatch pixels)
  - Missing shader token rewrites for transparency-order effects
  - Additional modern shader features need ES 3.00 equivalents
  - Buffer storage / SSBO aliasing for complex pack data paths
- Full Vulkan render path (DirectVK pipeline/present for game frames)
- Swapchain presentation for DirectVK
- SPIR-V compilation pipeline for dynamic Vulkan shaders
- Device-launch verification for 1.21.4 on real Android hardware

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