# Minecraft Rust Renderer — ZalithLauncher 2 plugin

Rust GLES passthrough + desktop-GL compatibility shims, packaged as a ZalithLauncher 2
renderer plugin APK.

## Honest status

| Piece | State |
|---|---|
| Plugin APK (V2, MAIN activity) | ready — install and pick **Rust Renderer** |
| `renderer-core` | Backend trait, config, error state, unit tests |
| `backend::gles` | Full resource Backend over system GLES 3.0+ (dlopen) |
| `backend::vulkan` | **Device discovery only** — loads `libvulkan`, enumerates and picks a physical device, creates a device + graphics queue, reports real info/limits. Cannot render (see below) |
| `gl-compat` `librust_gl.so` | GLES 3.x backend + OpenGL 3.3 compatibility entry-point layer + legacy fixed-function shims |
| Shader translate | Version rewrite, precision, texture2D→texture, gl_FragColor, attribute/varying |
| Format translate | BGRA swizzle, depth internal formats, clamp-to-border, BGRA8→RGBA8 storage |
| OpenGL 3.3 core API surface | Complete against the GL 3.0–3.3 core function list; unsupported desktop-only features return real GL errors instead of lying |
| Vulkan path | Device discovery + reporting; **no rendering path** |

### The 0x0502 during depth-attachment creation: root cause

`IllegalStateException: OpenGL error 1282` at `GlBackend.createTexture` ->
`WindowFramebuffer.createDepthAttachment` is **reproduced and fixed**.

Minecraft creates the window's multisampled depth attachment as a multisample **texture**
(`glTexStorage2DMultisample(GL_TEXTURE_2D_MULTISAMPLE, samples, GL_DEPTH_COMPONENT24, w, h)`).
OpenGL ES cannot express that: `glTexImage2DMultisample` accepts only colour-renderable
internal formats, so a depth request raises `GL_INVALID_OPERATION` in the driver. The bridge
forwarded the request, so the driver's error reached the game unchanged.

Two bugs, both in the translation:

1. **No multisample depth representation.** Depth MSAA in ES is a multisample *renderbuffer*.
   `glTexStorage2DMultisample` now allocates one, records the substitution, and
   `glFramebufferTexture2D(GL_DEPTH_ATTACHMENT, GL_TEXTURE_2D_MULTISAMPLE, tex, 0)` is
   redirected to `glFramebufferRenderbuffer`. Colour requests still try a real texture first,
   so a driver that supports them gets one.
2. **`glCreateTextures` bound every name to the requested target.** The multisample *types*
   (`GL_TEXTURE_2D_MULTISAMPLE`, `GL_TEXTURE_3D_MULTISAMPLE`) are texture kinds, not binding
   points, so `glBindTexture` on one raises `GL_INVALID_ENUM`. Those are now recorded in the
   target table but never bound.

Both multisample paths are now covered by the harness (`minecraft depth attachment` group):
the non-DSA `createTexture` path and the DSA path, the latter asserting a **complete**
framebuffer.

### The `pc=0x0` crash that stubs could not prevent

Returning a no-op stub for unresolved `gl*` names fixed the *resolver* path, but 1.16.5 with
OptiFine still died at `SIGSEGV pc=0x0`, after "Reloading custom textures". Two more gaps
behind it:

- **OptiFine calls ARB/EXT-suffixed names** — `glGenTexturesARB`, `glBindTextureARB`,
  `glTexImage2DARB`, `glFramebufferTexture2DEXT`, `glRenderbufferStorageEXT`,
  `glGenerateMipmapEXT` and more. None were exported.
- **The fixed-function surface was never exported either.** `glBegin`, `glEnd`, `glVertex3f`,
  `glColor4f`, `glTexCoord2f` and friends existed only in the resolver's stub table.

  **Correction.** An earlier revision of this section said ES 3.x implements all of these and
  that they were now forwarded. That was wrong: OpenGL ES 2.0 and 3.x have **no immediate
  mode, no matrix stack and no fixed-function state** (that was ES 1.x). Forwarding them to
  the driver dropped every call after one log line. `immediate.rs` now implements
  `glBegin`/`glEnd`/`glVertex*`/`glColor*`/`glTexCoord*` for real: vertices are collected on
  the CPU and `glEnd` draws them through the same path as client-array draws
  (`QUAD_STRIP` -> `TRIANGLE_STRIP`, `POLYGON` -> `TRIANGLE_FAN`, `QUADS` expanded as before).

  Names ES genuinely lacks (`glTexImage1D`, `glSelectBuffer`, the evaluators, display lists,
  pixel maps) are exported as announcing stubs, so a client that depends on one is visible
  in the log. `glGenLists` used to be declared as a void function taking a pointer, so
  callers read garbage out of the return register; it now returns a real id range.
  Display lists are still **not** recorded or replayed (1.12 entity models use them).

### The follow-up `0x0502`: depth formats paired with a type ES rejects

With `RENDERER_TRACE_GL=1` the next device log named the call exactly:
`glGenTextures -> glBindTexture -> glTexParameteri ×3 -> glTexImage2D`, raising
`GL_INVALID_OPERATION`.

Minecraft allocates the window depth attachment as
`glTexImage2D(GL_TEXTURE_2D, 0, GL_DEPTH_COMPONENT24, w, h, 0, GL_DEPTH_COMPONENT, GL_FLOAT, NULL)`.
That desktop triple is **not valid in ES 3.0**: a sized depth internal format must be paired
with a matching type, and `GL_DEPTH_COMPONENT24` requires `GL_UNSIGNED_INT`. The bridge
mapped only the internal format and passed the type through, so the driver refused the
allocation and the error became "OpenGL error 1282" in
`WindowFramebuffer.createDepthAttachment`.

`format_translate::map_upload_format` now maps the whole triple — internal format, format and
type — so a sized depth format is paired with a type ES accepts. Both the multisample and the
single-sample depth paths are reproduced in the harness, so this specific regression is caught
without a device.

### What has actually been verified

Verified by running it, on this machine:

- `cargo check --workspace` / `cargo test --workspace` — 33 unit tests pass.
- `cargo build -p gl-compat --release` links; `nm -D` shows **352 exported `gl*` entry points**
  with **no duplicate symbols**.
- `cargo run -p backend --example probe` runs and reports a precise reason when no
  Vulkan loader is present.

**Not** verified, because this environment has no Android NDK, no device, no Vulkan driver
(no ICD, no `libvulkan`) and no Minecraft:

- The APK has never been built from these changes.
- No Minecraft version has been launched, and no frame has been rendered, with this code.
  Treat the version rows below as *prior* results from earlier device testing, not as
  results for the current tree.
- The Vulkan backend's **positive** path — instance, device and queue creation, device
  selection, reported limits — has never executed against a real driver. It compiles, and
  its absence path is tested, but only a device can confirm the FFI layouts. The struct
  offsets it decodes are pinned by a unit test, yet that arithmetic is exactly the kind of
  thing that must be re-checked on hardware before trusting a reported limit.

**Will Minecraft launch?** It *may* get past GL version checks and compile simple shaders.
Complex packs (Sodium, Iris, modern core-profile shaders, MRT, geometry shaders) will still
fail. Treat every successful frame as a bonus and file the log line that broke.

### The 1.12 → 26.3 range is not one GL version

A single "OpenGL 3.3 core layer" only addresses the modern half of that range. What each
band actually asks for:

| Band | Requirement | Handled by |
|---|---|---|
| 1.12–1.15 | GL 2.1 fixed function, GLSL 120 | `fixed_func.rs` + `ff_draw.rs` (matrix stack, client arrays, alpha test, quads→tris) |
| 1.16 | GL 2.1 plus early core-profile calls | both layers |
| 1.17 – 26.3 | **GL 3.2 core floor**, no forward-compatible fallback; GLSL 330/410 shaders | `gl33.rs` + shader translate |

So "launch 1.12 through 26.3" needs the fixed-function layer *and* the 3.3 layer to both
work; neither one alone covers the range. (Versions follow Mojang's year-based scheme from
2026 onward, so `26.x` is the current numbering.)

### Sodium: why "pass Sodium conformance" is not a goal that can be met here

There is no Sodium conformance suite, and three things make Sodium specifically out of reach
for a GLES translation layer:

1. **Sodium's own docs rule this architecture out.** From the Sodium README: *"Devices
   which need to use OpenGL translation layers (such as GL4ES, ANGLE, etc.) are not
   supported and will very likely not work with Sodium. These translation layers do not
   implement required functionality, and they suffer from underlying driver bugs which
   cannot be worked around."* This project **is** such a translation layer.
2. **Sodium needs more than a 3.3 layer.** Sodium officially supports drivers compatible
   with **OpenGL 4.5+** and uses those "new API features"; a 3.3 layer cannot provide
   SSBOs, real persistent mapping, debug output, or the rest of the 4.x surface by
   construction. Sodium is also porting to Vulkan, which is where its effort is going.
3. **Sodium is a mod, not a GL feature.** It needs Fabric/NeoForge plus Mixin working
   under the launcher's JVM. That is an independent problem from the GL surface, and no
   change to `librust_gl.so` can fix it. Sodium also publishes **no 1.12–1.15 builds**
   (its supported list starts at 1.16.3), so "1.12 + Sodium" has no target at all.

What is realistic: vanilla and lightly-modded launch on the modern band, with mods that
stay inside the 3.3 surface.

## Backend selection: gles / vulkan / hybrid / auto

The renderer options in the plugin (ZalithLauncher → renderer settings) expose four modes,
read through `RENDERER_BACKEND_SELECT`:

| Mode | Behaviour |
|---|---|
| `gles` | GLES 3.x passthrough. Serves every desktop GL entry point the game calls. Default. |
| `vulkan` | Try Vulkan for renderer-owned work; if it cannot render, log why and stay on GLES so the game still starts. |
| `hybrid` | GLES for the GL surface, Vulkan preferred for renderer-owned work when it can render. |
| `auto` | First backend that initializes *and can render*, in the order vulkan → gles. |

The plugin declares this as a **single** selectable option. The previous build shipped
`RENDERER_BACKEND` both as a fixed env var and as a selectable under a different key, which
is what made the backend show up twice in the launcher's renderer list; `RENDERER_BACKEND_SELECT`
is still accepted as a fallback so an already-installed launcher build keeps working. A junk
value is ignored rather than fatal.

`hybrid` reports what it actually did: GLES serves every GL entry point, so until Vulkan can
render, hybrid means "GLES draws the frame, Vulkan supplies device information". The log says
which of those happened instead of leaving the choice looking like it did nothing.

### What the Vulkan backend does and does not do

**Does:** loads `libvulkan.so` (Android sonames, plus `libvulkan.so.1` on desktop), creates an
instance at the highest version the loader reports, enumerates physical devices, picks one by
device class → graphics-queue count → device-local memory, creates a logical device with a
graphics queue, and reports real `DeviceInfo`/`Capabilities` (vendor, device name, driver
version, memory, `maxImageDimension2D`). Zero new dependencies — direct FFI.

**Does not:** render. There is no SPIR-V compilation, pipeline creation, descriptor sets,
command recording, or swapchain presentation. `can_render()` therefore returns `false`, and
selection skips Vulkan rather than reporting a renderer that cannot draw a triangle. All
resource methods return `BackendError::Unsupported` with a specific message rather than a
misleading GL error.

This is deliberate. A Vulkan backend only becomes the renderer once something owns the drawing:
either the game speaks Vulkan itself, or this renderer compiles for Vulkan and runs the
fixed-function emulation path on it. Both need the pipeline/present layer that does not exist
yet, so the honest state today is *detects the device, reports it, stays on GLES*.

### What "making the Vulkan backend functional" actually requires

Not attempted here, deliberately: it is several thousand lines and there is no way to test it
from here (no Vulkan driver, no NDK, no device). The pieces, in dependency order:

1. **GLSL → SPIR-V compilation.** The shader translator currently emits GLSL ES; Vulkan needs
   SPIR-V. Requires a compiler crate (e.g. `naga` with `spv-out`, or `shaderc`) — the
   existing `shader-translate` output would have to be fed through it, and translate results
   validated with `spirv-val`.
2. **Resources.** VkBuffer + VkDeviceMemory with a real allocator (suballocation, as
   `VMA`/naga do), VkImage + VkImageView, VkSampler, with the format mapping in
   `format-translate` re-expressed as Vulkan formats rather than GLES sized formats.
3. **Pipelines.** Shader modules, pipeline layout, descriptor set layout/pool, render pass
   and framebuffer for the `Framebuffer` trait methods, pipeline cache.
4. **Submission.** Command buffer pool, recording for the draw calls, queue submit, and a
   fence or semaphore for the `map_buffer`/readback paths.
5. **Presentation.** VkSwapchainKHR, surface (needs `VK_KHR_android_surface`), and the
   acquire/present synchronization against the existing `eglSwapBuffers` export — this is
   where a hybrid GL+Vulkan design genuinely conflicts, since a frame cannot be split across
   two APIs without an interop copy.

Until (1)–(5) exist, `can_render()` stays `false` and selection keeps using GLES. The
detection code in `raw.rs` is a usable foundation for it: device selection, queue families
and memory sizing are the parts any real Vulkan backend needs first.

To check what a device reports:

```bash
cargo run -p backend --example probe   # exits 1 when no loader is present
```

## Compatibility shims (gl-compat)

The 3.3 layer is a translation layer, not a fake desktop driver: GLES-compatible 3.3 calls are forwarded directly, desktop-only calls are emulated where practical, and features with no GLES 3.x equivalent fail explicitly.

Covered 3.3-era paths include VAO/VBO/UBO state, sampler objects, instanced and range draws, multi-draw fallback loops, indexed buffer bindings, sync objects, query objects, texture storage, layered FBOs, multisample renderbuffers, clear-buffer APIs, integer/64-bit queries, transform-feedback/UBO forwarding, and packed vertex attributes.

Added on top of that, to close the gaps found against the GL 3.0–3.3 core function list:

- Buffer read-back — `glGetBufferSubData`, `glGetBufferPointerv`
- Texture targets/queries — `glCompressedTexImage3D`, `glCompressedTexSubImage3D`,
  `glCopyTexSubImage3D`, `glGetCompressedTexImage`, `glGetTexLevelParameterfv`,
  `glGetTexImage` (BGRA/BGR read back through RGBA/RGB and swizzled)
- `glFramebufferTexture`, `glIsEnabledi`, `glGetBooleani_v`
- `glTexImage2DMultisample`, `glTexImage3DMultisample`, `glGetMultisamplefv`
- Generic integer vertex attributes — `glVertexAttribI2i`/`I2iv`/`I2ui`/`I2uiv`,
  `I3i`/`I3iv`/`I3ui`/`I3uiv`, `I4iv`/`I4uiv`
- Multi-bind `glBindTextureUnit` (restores the previously active unit)
- DSA texture getters — `glGetTextureImage`, `glGetTextureLevelParameteriv`,
  `glGetTextureParameteriv`/`fv` (bind, delegate, restore)
- Program interface queries — `glGetProgramInterfaceiv`, `glGetProgramStageiv`,
  `glGetProgramResourceIndex`/`iv`/`Name`
- `glDepthRange(double,double)` → `glDepthRangef`, `glTexParameteriv`/`fv` (same wrap
  translation as the scalar forms), `glRenderbufferStorage` (BGRA8 → RGBA8)
- `glBufferStorage` now honours its flags: ES 3.1 `glBufferStorage` when the driver has it,
  otherwise `glBufferData` with `GL_DYNAMIC_STORAGE_BIT` respected, and immutable buffers
  reject `glBufferData`/`glBufferSubData` with `GL_INVALID_OPERATION`

Still rejected on purpose, because GLES 3.x has no equivalent: `glTexStorage1D`,
`glTexSubImage1D`, `glTexBuffer`/`glTexBufferRange`, double-precision vertex attributes,
timer queries, geometry stages, and `GL_QUADS`/`LINE`/`POINT` polygon modes.

### Extension advertising is capability-probed, not assumed

`crates/gl-compat/src/caps.rs` measures the device once — ES version, the full extension set,
and the limits that actually change behaviour (`GL_MAX_TEXTURE_MAX_ANISOTROPY_EXT`,
`GL_MAX_TEXTURE_SIZE`, vertex attribs, draw buffers, samples, …). Decisions then read from
that instead of from a fixed guess:

- **Which aliases to advertise** is computed per device. `GL_ARB_instanced_arrays`,
  `GL_ARB_uniform_buffer_object`, `GL_ARB_map_buffer_range` and
  `GL_ARB_program_interface_query` need ES 3.1 (or the matching ES extension on 3.0);
  `GL_EXT_texture_filter_anisotropic` is claimed only when the driver has it *and* reports a
  usable limit; `GL_EXT_color_buffer_float`/`_half_float` follow ES 3.2 or the extension.
  Advertising an alias whose backing feature is missing is the bug this prevents — the
  client enables a fast path and gets a driver error instead of the behaviour it asked for.
- **`GL_TEXTURE_MAX_ANISOTROPY_EXT`** is dropped on devices without the extension instead of
  raising `GL_INVALID_ENUM`.
- **`GL_CLAMP_TO_BORDER`** survives only where `GL_EXT_texture_border_clamp` exists;
  otherwise it becomes clamp-to-edge, which is the behaviour the ES enum set allows.
- With no live context the probe is reported invalid and only the always-on alias set is
  offered, rather than guessing.

`glGetStringi` and `GL_NUM_EXTENSIONS` are served from one merged, de-duplicated list
(driver entries first, then the filtered aliases), so an iterator that reads `count` entries
never hits an early null — previously the count came from the driver while indices past the
driver's list resolved to null, which truncated enumeration and hid every alias.
`glGetString(GL_EXTENSIONS)` returns the alias list; GLES 3.0 defines the driver half of that
string as empty and directs clients to `glGetStringi`, which is where the driver-inclusive
list lives. The merged list is rebuilt until the driver has actually reported extensions, so
an early query made before the context exists no longer caches a truncated list for the rest
of the process.

`GL_KHR_debug` is **not** advertised: the layer exports no `glDebugMessageCallback`,
`glObjectLabel` or debug-group entry points, so a client that probed the extension and then
bound the callback would have resolved a null pointer. A unit test asserts every advertised
extension resolves a real entry point, and that the two cannot drift apart.

BGRA storage is real rather than aspirational: `GL_BGRA8_EXT` maps to `GL_RGBA8` for texture
*and* renderbuffer targets, matching the BGRA→RGBA upload swizzle, which is what makes the
`GL_EXT_texture_format_BGRA8888` advertisement consistent.

### Hot-path cost

The bridge used to run a `dlsym` — plus a `CString` allocation in the `eglGetProcAddress`
fallback — **on every forwarded GL call**, so a draw-heavy frame paid a symbol lookup per
draw. Entry points are now memoized by the address of the name literal, turning that into
one lookup per call, and the table is dropped when `eglMakeCurrent` observes a context change
(so context-specific pointers from `eglGetProcAddress` are never reused across contexts).

Two other per-call costs are gone:

- `glTexImage2D`/`glTexSubImage2D` issued **three `glGetIntegerv` round-trips each** to read
  `GL_UNPACK_ROW_LENGTH`/`SKIP_ROWS`/`SKIP_PIXELS`. Those enums can only change through
  `glPixelStorei`, which now maintains a local shadow, so the upload path reads them without
  touching the driver. A texture-atlas-heavy frame was paying 3× the driver calls per upload.
- `GL_ARRAY_BUFFER` is global context state, so it is shadowed locally instead of queried by
  every client-array pointer call on the fixed-function path.
- The sticky error flag moved off `SeqCst` to Release/Acquire; it is read-modify-written on
  every `glGetError`.

Regressions here are covered by unit tests: repeated lookups must not grow the cache, cached
misses stay misses, clearing the cache forgets everything, and the unpack shadow must be
visible to the upload path and resettable.

### Verified GL 3.3 completeness

The exported surface is checked against the GL 3.0–3.3 core function list. What is *not*
exported as a real entry point is deliberate: `glGetMap*`/`glGetTexEnv*`/`glGetTexGen*` are
GL 1.x–2.0 fixed-function getters handled by the legacy no-op table; `glGetTexParameteri_v`
(GL 4.4), `glGetTexSubImage` (GL 4.3) are past the 3.3 line; and `glUniformfv`,
`glUniformiv`, `glUniformuiv` are not real GL entry points (only the `glUniform1fv`-style
indexed forms exist).

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
| `RENDERER_BACKEND` | `gles` | `gles` / `hybrid` / `vulkan` / `auto` — one option in the launcher; `RENDERER_BACKEND_SELECT` is still read as a fallback for older launcher builds |
| `RENDERER_SPOOF_GL` | `1` | Advertise OpenGL 3.3 core / GLSL 3.30; `0` reports the real GLES strings. Selectable in the launcher. |
| `LIBGL_ES` | `3` | Hint for launcher / other libs |
| `RENDERER_DEBUG` | `0` | `1` enables extra logging. Selectable in the launcher. |

## Next work (spec order)

1. Shader translator: geometry/tessellation reject, MRT, more builtins
2. **Device verification of the new 3.3 entry points** — see checklist below
3. Full GLSL 410+ → ES 320 rewrite (MC 1.20+ ships desktop-style core shaders)
4. Vulkan backend (spec phase 5)
5. Standalone triangle / FBO test APK — the cheapest way to test a GL path
   without a full Minecraft launch

### Testing the build without a device

`tools/run-glsmoke.sh` renders a triangle **through the built `librust_gl.so`** on a headless
GLES context and checks the resulting pixels. It links only EGL and resolves every GL entry
point from the bridge at runtime, so no check can pass by quietly reaching the driver instead.
Needs Mesa's software rasteriser:

```bash
sudo apt-get install -y libegl1 libgles2 libegl1-mesa-dev libgles2-mesa-dev
./tools/run-glsmoke.sh
```

It covers what unit tests cannot: the spoofed version string, extension enumeration agreeing
with `GL_NUM_EXTENSIONS`, FBO completeness, desktop-GLSL-to-ES translation of a 120-era MRT
shader against a real compiler, and the DSA vertex-array path 1.20.5+ uses. A shader that
fails to compile, a state call that no-ops, or a mis-bound attribute shows up as wrong pixels
rather than as a black screen much later.

### Visual replay: a real Minecraft frame, compared to a golden reference

`tools/trace_replay/run_fixture.sh <1.17|26.3>` builds the replay tool, replays a bundled
Minecraft capture **through `librust_gl.so`**, and scores the rendered frame against a golden
PNG with SSIM. This is a real end-to-end visual test, not a smoke check.

One setting is essential and was found by hitting the failure: **`RENDERER_SPOOF_GL=0`**. The
replay must see the context the capture recorded, which is an ES context. With the spoof on,
`glGetString` reports a desktop "3.3 (Core Profile)" string, the tool takes a desktop
framebuffer-readback path against an ES context, and the snapshot comes back empty
(`failed to get snapshot`) even though every call replayed. The runner now sets it.

Current results on Mesa llvmpipe:

| Fixture | Scene | SSIM | Threshold | Verdict |
|---|---|---|---|---|
| 1.17 main menu | vanilla | 0.999962 | 0.99 | **PASS** |
| 26.3 improved-transparency | **shader pack** | 0.000006 | 0.995 | **FAIL** |

The 26.3 failure was traced to two real bugs, both fixed: with the spoof off a desktop Mesa build
reports `3.3 (Compatibility Profile) Mesa 25.2.8`, which `parse_es_version` did not recognise, so
the GLES backend refused to initialise and every backend-dependent call silently did nothing.
A failed initialisation was also retried on every GL call -- 51972 attempts, and 51972 log lines
that buried everything else. Both are fixed; the frame is still black, so a further cause remains.

The vanilla capture matching a golden frame is the strongest evidence so far that the GL
translation is faithful for ordinary rendering.

The shader-pack capture failing this badly — `mismatchPixels` is the whole frame, so it is not
a subtle colour difference but a completely different image — is a measured answer to "can this
run Complementary/Derivative": **not yet**, and it is now reproducible without a device. CI runs
both, gates on 1.17, and reports 26.3 as a warning until it passes.

### Replaying real Minecraft captures

`tools/trace-replay.sh` replays genuine Minecraft GL captures **through `librust_gl.so`**.
apitrace's `eglretrace` recreates the context the trace recorded and re-issues every call;
preloading the library puts this bridge in the path, because `eglretrace` resolves GL entry
points through `eglGetProcAddress`, which the library exports. The frames are therefore
rendered by the same code Android runs.

```bash
sudo apt-get install -y apitrace xvfb libgl1-mesa-dri
./tools/trace-replay.sh                                  # default fixtures
./tools/trace-replay.sh <fixture.tgz>:<recorded-gl-ver>   # specific ones
```

Two settings are load-bearing, and both were found by hitting the failure:

- **`RENDERER_SPOOF_GL=0`.** eglretrace checks the replay context reports the same version the
  trace recorded (ES 3.0 for the 1.17 capture). Spoofing 3.3 makes replay refuse to start.
- **`MESA_GL_VERSION_OVERRIDE` / `MESA_GLSL_VERSION_OVERRIDE`.** The 1.21.x captures were
  recorded against a 4.6 core context -- which is exactly why Zalith sets
  `MESA_GL_VERSION_OVERRIDE=4.6` -- so the version has to be overridden for the context request
  to be satisfiable on a software rasteriser.

Current status, on Mesa llvmpipe:

| Capture | Result |
|---|---|
| Minecraft 1.17 main menu | 2 frames rendered |
| Minecraft 1.21.1 NeoForge, world render | 2 frames rendered |

It runs in the `gl-smoke` workflow and fails the job if a capture does not replay. The 1.21.11
capture that shows a black screen on device is the obvious next one to add: replaying it here
would reproduce that failure without a phone.

### Trace coverage against real Minecraft captures

`tools/trace-coverage.sh` measures entry-point coverage against genuine Minecraft apitrace
captures, from the MobileGL fixture set (1.17, 1.21.1, and others). It fetches each fixture
through the Git LFS batch API -- the repository stores LFS pointers, so a plain raw download
returns a 133-byte text file -- parses it with `apitrace dump`, and diffs the `gl*` functions a
real run calls against what `librust_gl.so` exports.

```bash
sudo apt-get install -y apitrace
./tools/trace-coverage.sh                       # default fixtures
./tools/trace-coverage.sh <fixture.tgz> ...     # specific ones
```

This is not frame-accurate replay: MobileGL's `mobilegl_trace_replay` is hard-wired to
MobileGL's own renderer via `--mobilegl-library`, and a GLX/EGL display server is needed to
replay at all. What the traces *do* give is the call stream, and a name a real run calls that
the library does not export is a null function pointer for any client that resolves by dlsym --
exactly how 1.16.5 died. That is measurable without a display.

It found three real gaps that device logs had not yet surfaced: `glDebugMessageControl`,
`glBindImageTexture`, and `glMultiDrawElementsBaseVertex`. Coverage is now:

| Trace | Distinct `gl*` calls | Exported |
|---|---|---|
| Minecraft 1.17 (main menu) | 54 | 100% |
| Minecraft 1.21.1 (NeoForge, world) | 102 | 100% |

The step runs in the `gl-smoke` workflow, uploads `trace-coverage.log`, and fails the job on
any unexported entry point. It fails loudly rather than passing when no trace could be checked,
so a fetch or parse problem can never read as success.

### Testing Minecraft rendering

The harness has a group of scenarios that mirror what the vanilla renderer actually does,
because that is where a translation bug becomes a visibly wrong world instead of an error:

| Scenario | What it covers |
|---|---|
| Texture atlas sub-image | `glTexSubImage2D` with `UNPACK_ROW_LENGTH` set, as the 1.12–1.15 atlas uploader does — exercises the local unpack shadow |
| Chunk geometry | interleaved buffer with a byte stride, drawn with **32-bit indices** |
| Depth / fog | `glDepthRange(double)` translation and depth state |
| Alpha blending | `GL_BLEND` + `glBlendFunc` for GUI and translucent blocks |
| Scissor | clipping used by the GUI and chunk culling |
| MVP transform | `glUniformMatrix4fv`, where a bad upload moves geometry rather than failing |
| 1.12 client arrays | `glVertexPointer`/`glColorPointer` with `GL_QUADS`, the fixed-function route |
| DSA vertex arrays | `glCreateBuffers`/`glNamedBufferData`/`glVertexArrayAttribFormat`, the 1.20.5+ route |

### HTML report

The run writes a self-contained HTML report — environment (spoofed vs. real GL strings,
advertised extension count), every check grouped and colour-coded, and a detail column for
failures. No assets or network access needed to read it.

```bash
GLSMOKE_REPORT=target/reports/gl-smoke.html ./tools/run-glsmoke.sh
```

`.github/workflows/gl-smoke.yml` runs this in a separate workflow from the APK build, installs
Mesa's software rasteriser, fails the job on regressions, and uploads the report and log as
the `gl-smoke-report` artifact. Known issues are reported as `known` rather than failing, so a
documented limitation stays visible without blocking unrelated work.

### Screenshots

Each visual scene is rendered, read back, and written as a PNG, then embedded in the report as
base64 so a single uploaded HTML file is self-contained. The PNG writer is dependency-free
(stored deflate blocks plus CRC-32), so the harness needs no image library.

Scenes currently captured: an interpolated vertex-colour gradient, a procedural fragment
pattern (`length()` maths, no input texture), and a `gl_FragCoord` checkerboard. All three go
through the same desktop-GLSL translation the game relies on, so they exercise the shader path
as well as producing something a human can look at.

```bash
GLSMOKE_SHOT_DIR=target/reports/screenshots ./tools/run-glsmoke.sh
```

CI uploads the HTML, the log, and the PNGs as the `gl-smoke-report` artifact.

### On Minecraft version coverage

The harness cannot launch Minecraft — that needs a device and the game. What it does instead
is cover the GL surface each version band depends on, so a regression in one band is caught:

| Band | What it needs | Covered by |
|---|---|---|
| 1.12–1.15 | fixed function, GLSL 120, client arrays | `fixed function` group, GLSL 120 scenes |
| 1.16 | GLSL 150, VAOs/VBOs | `shader translation` group |
| 1.17–1.20.4 | GL 3.2 core, MRT, depth/blend | `minecraft rendering` group |
| 1.20.5–26.x | Direct State Access | `dsa` group |

### Open findings

Host-side, all reported as known issues rather than failures so the suite stays meaningful:

- **A freshly created vertex array draws nothing.** Attribute size, enabled flag and buffer
  binding all read back correctly and there is no GL error, yet nothing rasterises. Reproduces
  with indexed and non-indexed draws, and whether or not the objects are reused.
- **The DSA path renders nothing** on this host.
- **The 1.12 fixed-function quad path does not draw.**

**Retracted, and why it matters:** this file previously listed `glScissor` and "DSA stride
stays 0" as bridge bugs. Both were harness bugs — a sample point outside its own scissor
rectangle, and a texture left bound to unit 0 so later draws multiplied by black. More
importantly, the stride reading was a **measurement artifact**: `glGetVertexAttribiv(
GL_VERTEX_ATTRIB_ARRAY_STRIDE)` returns 0 even for an attribute that demonstrably renders
correctly, so stride cannot be used to diagnose anything here. That is why the harness no
longer reports stride as data.

### GLSL to SPIR-V (naga)

`crates/backend/src/spirv.rs` compiles GLSL to SPIR-V using `naga`, behind a cargo feature
that is **off by default**. Verified: a vertex shader compiles to a real SPIR-V module (the
test asserts the `0x07230203` magic word, so it is genuinely SPIR-V and not empty output).

Trunk is used rather than the published crate because `naga 30.0.1`'s `glsl-in` feature does
not compile — the front-end calls `apply_default_interpolation`, which no longer exists on the
interpolation enum. Trunk has that fixed and renamed the front-end API (`Options { stage,
defines }` replaces `Version`). The dependency is pinned to an exact revision for that reason.
It resolves from git, so it is optional: an offline or NDK-only build is not forced to fetch it.

This does **not** make Vulkan able to draw. Words in hand are not a renderer: there is still no
`VkShaderModule`, pipeline layout, descriptor sets, render pass, command buffer or swapchain,
which is why `can_render()` stays `false` and `compile_shader` still returns `Unsupported` —
now with an accurate reason instead of "no compiler linked". Geometry shaders are rejected
explicitly, since naga has no Geometry stage.

### Vendored native dependencies

`third_party/glslang` (Khronos, Apache-2.0, pinned at 15.0.0) is vendored as a submodule for
GLSL → SPIR-V. **It is inert**: nothing in the Rust build references it, so the APK build and
the test suite are unaffected. It is not yet buildable here — SPIRV-Tools has to be fetched at
build time and the arm64 cross-compile needs an NDK and CMake, which this environment does not
have. See `third_party/README.md` for the exact commands and what remains.

Consequently `backend::vulkan::can_render()` is still `false` and `compile_shader` still
returns the honest *"no SPIR-V compiler is linked"*. Nothing about rendering has changed yet.

### GL 1.x-4.x coverage

The surface was measured against the core function lists for each version. A measured pass
found roughly 90 entry points that were never wired up at all, concentrated in GL 1.2-1.3
(lighting/material getters, fog coordinates, the multi-texture and transpose-matrix families)
and GL 1.4 (secondary colour, window positioning). All of them are now implemented: **669
entry points exported, up from 591.**

The split is deliberate rather than uniform. Where ES 3.x implements a call it is forwarded --
lighting and material getters, `glFogCoord*`, `glMultiTexCoord*`, `glSecondaryColor*`,
`glWindowPos*`, `glPointParameter*`, the GL 3.0 vertex-attrib vector forms. Where ES has no
equivalent it is exported as a stub that announces itself once: 1D texture entry points, pixel
maps, rasterisation and window rectangles (all removed in ES 2.0), transpose matrices, and
fragment-depth writing. `glSwapBuffers` is exported and reports that presentation is EGL's
concern, not GL's.

Remaining gaps are all in GL 4.x, where ES has no equivalent at all: `Vk`-era draw and
descriptor APIs, compute, geometry and tessellation stages, and indirect draws. Those are
tracked rather than stubbed into misleading no-ops.

### Backends are one module with two implementations

`crates/backend/` holds both, as `gles` and `vulkan`. They shared the `Backend` trait and the
`libloading` dependency and isolated nothing from each other, so the split was costing a
dependency edge and two import paths:

- `backend::gles::GlesBackend` — the real ES driver underneath the translation layer
- `backend::vulkan::VulkanBackend` — device discovery and reporting; `can_render()` is `false`,
  so selection never chooses it to draw

### How the game identifies this renderer

`GL_VENDOR` and `GL_RENDERER` are built once from the capability probe, so they name both
what this is and what it is drawing on:

```
GL_VENDOR    OpenGL ES translation layer
GL_RENDERER  Rust Renderer GL translation (OpenGL ES 3.2 Mali-G77 MC9)
GL_VERSION   3.3 (Core Profile) RustRenderer GLES translation
```

Two reasons. It is accurate -- this is a desktop-GL-to-GLES translation layer, and calling it
a "passthrough" was not true. And several mods change behaviour when they detect a translation
layer, so naming it lets them adapt rather than crash; Sodium in particular documents that it
does not support them. Naming the device also means a log line or bug report identifies the GPU
that actually renders instead of pointing at the layer in front of it.

### Entry points are grouped by API family

`crates/gl-compat/src/` is organised the way the layer actually thinks, rather than as one
undifferentiated list:

| module | surface |
|---|---|
| `gl` | the desktop-GL compatibility surface, submodelled by introducing version |
| `gles3` | the direct OpenGL ES surface this translates onto, including the capability probe |
| `khr` | `GL_ARB_*` / `GL_EXT_*` / `GL_KHR_*` extension spellings |
| `egl` | context and symbol-resolution shims -- **still inside `lib.rs`, not yet extracted** |

The GL family is `gl/{v1_0, v1_1, v3_3}` plus `vertex_state.rs`, `named_objects.rs`,
`fixed_func.rs`, `fixed_draw.rs` and `immediate.rs`.

| module | what it is |
|---|---|
| `vertex_state` | vertex array / attribute format / buffer association, and the MSAA depth substitution |
| `named_objects` | the GL 4.5 named-object entry points (`glCreateTextures`, `glNamedFramebuffer*`, …) |
| `fixed_func` / `fixed_draw` | fixed-function state, and the quad-to-triangle draw emulation |
| `immediate` | immediate-mode state |

Each module publishes an `EXPORTS` manifest and tests hold it honest: every claimed name must
resolve to something that is **not** the shared legacy no-op, and a name must belong to exactly
one version. That is the invariant whose absence let `glLightModeliv` sit in the resolver's
stub table looking handled while every call was silently discarded.

### DSA / named-object entry points (`dsa_named.rs`)

Minecraft 1.20.5+ and every mod on top of it use the DSA spellings: the object is named rather
than bound. The 1.21.11 log asked for 62 entry points this layer did not resolve —
`glTextureParameteriv`/`fv`, `glNamedFramebuffer*`, `glNamedRenderbuffer*`, `glMapNamedBuffer*`,
`glCreateFramebuffers`/`Samplers`/`Queries`, `glVertexArrayVertexBuffers`, the
`ARB`-suffixed instancing aliases, and the bulk `glBindTextures`/`glBindBuffers*` family.

GLES has no named objects, so each one binds the object and delegates to the classic entry
point that already works. Two bugs surfaced while writing them:

- `glTextureParameteri`/`f` bound the texture to a hardcoded `GL_TEXTURE_2D` regardless of its
  real target, which is exactly the kind of thing that raises `GL_INVALID_OPERATION` on a
  depth or array attachment during framebuffer setup.
- A texture name used before this layer saw it created was treated as an error. It now
  defaults to 2D and logs once, because a missing detail there cost a framebuffer.

GL error attribution: the bridge now logs which entry point last set an error from its own
code (`[dsa] GL error site: ...`). Minecraft only reports the numeric code, so without this
the log could not say which of several dozen sites raised `1282`.

### The `glFogfv` crash

A null entry point is the worst failure mode in this layer: LWJGL resolves a function pointer
and calls it, so a missing symbol is `SIGSEGV` at address 0 with no GL error and nothing in
`glGetError` to explain it. Two things came out of that crash:

- **An audit now runs on every invocation.** `legacy dispatch audit` resolves the whole
  fixed-function surface LWJGL enumerates — fog, lighting, material, texture env, matrix
  stack, client arrays, immediate mode, display lists, multisample — and fails if any returns
  null. It currently resolves all 82 of them.
- **The fog path is real rather than a hole.** `glFogf`/`glFogi`/`glFogfv` recorded nothing
  before, `glFogColor` (GL 1.4, LWJGL's `GL11.glFogColor`) was not exported at all, and
  `glGetFloatv`/`glGetIntegerv` forwarded the `GL_FOG_*` pnames straight to a driver that has
  no fog, which raises `GL_INVALID_ENUM`. Fog parameters are now recorded and those queries
  are answered from that record.

Note the crash was almost certainly from an APK built before the reachability fix: that fix
restored `glBindBuffer` and `glPixelStorei`, which were exported but unreachable through
`eglGetProcAddress` — the same null-pointer class of bug, on functions MC calls constantly.

**Rebuild the APK before retesting.** If it still crashes, the audit names the null symbol
rather than leaving a bare `si_addr = 0`, and the log line to send is:

```bash
adb logcat -c
adb logcat -s RustRenderer RendererV2Plugin > mc.log
# launch the version under test from ZalithLauncher, wait for it to fail, then stop it
grep -E "Missing entry point|GLCompat|GLBridge|Renderer|Vulkan" mc.log | head -80
```

### How to actually verify a GL change

The 3.3 layer can only be trusted against a real device. Loop for each entry-point change:

```bash
./build.sh
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb logcat -c && adb logcat -s RustRenderer RendererV2Plugin
# launch the version under test from ZalithLauncher, then:
```

Watch for `[GLBridge] Missing entry point: <name>` (a symbol the game asked for that we do
not resolve), `[GLCompat]` lines (shims taking an unsupported path), and any
`GL_INVALID_*` raised during a frame. Each of those is a concrete bug report; the fix is to
make that one call behave, not to widen the version claim.

## License / honesty

Do not claim Minecraft compatibility until a concrete version has been tested and logged.

## Support status

| Target | Status |
|--------|--------|
| Vanilla MC 1.16 | **Works** on an earlier build — world render verified on device |
| Vanilla MC 1.12–1.15 | Likely works (fixed-function path) |
| Vanilla MC 1.17–1.20 | Partial — modern shaders via GLES3 passthrough + GLSL rewrite; test per version |
| Vanilla MC 1.21 / 26.x | Experimental — needs more GL 4.x / DSA coverage |
| Sodium | **Out of scope for a 3.3 layer** — see above: unsupported architecture per Sodium's own docs, needs 4.5-class drivers, and is a mod requiring a working Fabric/NeoForge loader |
| Iris / shader packs | GLSL translated (OptiFine/Iris-era `#version 120`, MRT, `gl_FragData`, `texture2DGrad`, `gl_FragDepthEXT`); **unverified** — MRT needs `glDrawBuffers` paths that are untested here, and Iris/OptiFine are mods needing Fabric/Forge + Mixin |
| Performance | GLES driver does the heavy lifting; FF path is only used when no program is bound |

These rows describe earlier device testing and are **not** re-verified for the current tree
(see *What has actually been verified*).

### Enabling debug logs
```
RENDERER_DEBUG=1
```

### Roadmap toward modern MC
1. Extension string + `glGetStringi` advertising — **done** (merged, de-duplicated, self-consistent)
2. `glBufferStorage` / multi-draw shims — **done** (flags honoured, immutability enforced when supported)
3. Full GLSL 410+ → ES 320 rewrite
4. Device verification per version, oldest band first (1.12 fixed-function → 1.17 core)
5. Vulkan rendering path: SPIR-V compilation (GLSL → SPIR-V), pipelines, descriptor sets,
   command submission, swapchain. Until this exists, `vulkan`/`hybrid` only detect the device.

The previous "toward Sodium" goal is deliberately dropped rather than left aspirational —
see *Sodium: why "pass Sodium conformance" is not a goal that can be met here*.


### Minecraft 1.16 compatibility
The Android plugin sets `JAVA_TOOL_OPTIONS=-Dorg.lwjgl.util.NoChecks=true`. Minecraft 1.16 can pass a null fog buffer through its deprecated `RenderSystem.fog` path; LWJGL 3.3.3 normally rejects that at `Checks.check()` before the native compatibility shim is reached. The renderer's `glFogfv` shim safely ignores a null parameter, so disabling the Java-side LWJGL pointer check lets the compatibility layer handle the call instead of crashing.


## Fixed-function emulation: what is and is not covered (1.12-1.16)

Covered by `ff_draw.rs` / `fixed_func.rs` / `immediate.rs`:
matrix stacks, client arrays, `glBegin`/`glEnd`, colour, texture unit 0, alpha test, **fog**
(linear/exp/exp2, from the recorded `glFog*` state), the **lightmap** (unit 1 on 1.12-1.14,
unit 2 on 1.15/1.16, with that unit's texture matrix applied) and the unit-0 texture matrix.
Per-unit `GL_TEXTURE_2D` enable and `glActiveTexture` are now tracked, so enabling texturing
on the lightmap unit no longer switches it on for unit 0.

The extended program is compiled first; if it fails, the previous program is used and the log
says `extended program failed`. Look for `fixed-function emulation program ready (extended...)`.

**Not covered** (visible as flat or missing effects, not crashes):
- lighting (`GL_LIGHTING`, `glLight*`, `glMaterial*`, normals) -- entities are not shaded
- texture environment modes (`glTexEnv`), including the 1.15/1.16 entity overlay (unit 1,
  `GL_COMBINE`) -- the hurt-flash tint is missing
- display lists, `glPushAttrib`/`glPopAttrib` (announced, not applied)
- compat-profile shader built-ins (`gl_Vertex`, `gl_ModelViewMatrix`, `ftransform()`,
  `gl_Color`, ...) used by `#version 120` OptiFine shader packs. `shader-translate` rewrites the
  matrices to `mat4(1.0)` and leaves `gl_Vertex` undeclared, so those packs either fail to
  compile or draw wrong geometry. Supporting them means declaring `rust_*` attributes and
  uniforms in the translator and feeding them from the FF state at draw time.

**None of this has been compiled or run by the author of this change** (no Rust toolchain was
available). Run `cargo test --workspace` and `tools/run-glsmoke.sh` first.
