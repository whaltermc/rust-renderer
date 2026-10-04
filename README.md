# Minecraft Rust Renderer — ZalithLauncher 2 plugin

Rust GLES passthrough + desktop-GL compatibility shims, packaged as a ZalithLauncher 2
renderer plugin APK.

## Honest status

| Piece | State |
|---|---|
| Plugin APK (V2, MAIN activity) | ready — install and pick **Rust Renderer** |
| `renderer-core` | Backend trait, config, error state, unit tests |
| `gles-backend` | Full resource Backend over system GLES 3.0+ (dlopen) |
| `vulkan-backend` | **not implemented** — `auto` falls back to GLES |
| `gl-compat` `librust_gl.so` | GLES 3.x backend + OpenGL 3.3 compatibility entry-point layer + legacy fixed-function shims |
| Shader translate | Version rewrite, precision, texture2D→texture, gl_FragColor, attribute/varying |
| Format translate | BGRA swizzle, depth internal formats, clamp-to-border |
| OpenGL 3.3 core API surface | Broad entry-point coverage; unsupported desktop-only features return real GL errors instead of lying |
| Vulkan path | not started |

**Will Minecraft launch?** It *may* get past GL version checks and compile simple shaders.
Complex packs (Sodium, Iris, modern core-profile shaders, MRT, geometry shaders) will still
fail. Treat every successful frame as a bonus and file the log line that broke.

## Compatibility shims (gl-compat)

The 3.3 layer is a translation layer, not a fake desktop driver: GLES-compatible 3.3 calls are forwarded directly, desktop-only calls are emulated where practical, and features with no GLES 3.x equivalent fail explicitly.

Covered 3.3-era paths include VAO/VBO/UBO state, sampler objects, instanced and range draws, multi-draw fallback loops, indexed buffer bindings, sync objects, query objects, texture storage, layered FBOs, multisample renderbuffers, clear-buffer APIs, integer/64-bit queries, transform-feedback/UBO forwarding, and packed vertex attributes.

The shader translator also rewrites common GLSL 3.30 desktop constructs to GLSL ES 3.00, including desktop version headers, precision, `attribute`/`varying`, texture functions, explicit layout cleanup, `noperspective`, and double-precision type fallbacks.

- `glShaderSource` — desktop GLSL → GLSL ES (header, precision, texture2D, FragColor, attr/varying)
- `glTexImage2D` / `glTexSubImage2D` — BGRA → RGBA swizzle when unpack state is default
- `glTexParameteri` — `CLAMP` / `CLAMP_TO_BORDER` → `CLAMP_TO_EDGE`
- `glMapBuffer` → `glMapBufferRange` over the whole buffer
- `glDrawBuffer` → `glDrawBuffers(1, …)`
- `glClearDepth(double)` → `glClearDepthf(float)`
- `glPolygonMode` — FILL no-op; LINE/POINT → `GL_INVALID_OPERATION`
- `RENDERER_SPOOF_GL` (default **on** via plugin env) — reports GL 3.2 / GLSL 1.50
- `glXGetProcAddress` / `eglGetProcAddress` / `glGetProcAddress` — resolve our exports, then driver

## Build

```bash
rustup target add aarch64-linux-android
cargo install cargo-ndk
export ANDROID_NDK_HOME=/path/to/ndk
./build.sh
```

Output: `android/app/build/outputs/apk/debug/app-debug.apk`.

Or push to GitHub and download the Actions artifact (`.github/workflows/build.yml`).

Install the APK, open ZalithLauncher 2 → renderer list → **Rust Renderer**.

Logs: `adb logcat -s RustRenderer RendererV2Plugin`

## Env (set by the plugin)

| Variable | Default | Meaning |
|---|---|---|
| `RENDERER_BACKEND` | `gles` | `gles` / `vulkan` / `auto` |
| `RENDERER_SPOOF_GL` | `1` | Advertise OpenGL 3.2 (set `0` to report real GLES strings) |
| `LIBGL_ES` | `3` | Hint for launcher / other libs |
| `RENDERER_DEBUG` | unset | `1` enables extra logging later |

## Next work (spec order)

1. Shader translator: geometry/tessellation reject, MRT, more builtins  
2. Missing desktop entry points that show up in logcat  
3. `glGetTexImage` / PBO readback path  
4. Vulkan backend (spec phase 5)  
5. Standalone triangle / FBO test APK  

## License / honesty

Do not claim Minecraft compatibility until a concrete version has been tested and logged.

## Support status

| Target | Status |
|--------|--------|
| Vanilla MC 1.16 | **Works** — world render verified |
| Vanilla MC 1.12–1.15 | Likely works (fixed-function path) |
| Vanilla MC 1.17–1.20 | Partial — modern shaders via GLES3 passthrough + GLSL rewrite; test per version |
| Vanilla MC 1.21+ | Experimental — needs more GL 4.x / DSA coverage |
| Sodium | **Not yet** — needs multi-draw, buffer storage semantics, full extension set; shims started |
| Iris / shader packs | **Not yet** — needs broader GLSL + extension surface (shadow, compute later) |
| Performance | GLES driver does the heavy lifting; FF path is only used when no program is bound |

### Enabling debug logs
```
RENDERER_DEBUG=1
```

### Roadmap toward Sodium / modern MC
1. Extension string + `glGetStringi` advertising (started)
2. `glBufferStorage` / multi-draw shims (started)
3. Persistent mapped buffers / fence-heavy paths Sodium uses
4. Full GLSL 410+ → ES 320 rewrite
5. Optional Desktop-GL-on-Vulkan path long-term

