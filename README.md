# Minecraft Rust Renderer — ZalithLauncher 2 plugin

Rust GLES passthrough + desktop-GL compatibility shims, packaged as a ZalithLauncher 2
renderer plugin APK.

## Honest status

| Piece | State |
|---|---|
| Plugin APK (V2, MAIN activity) | ready — install and pick **Rust Renderer** |
| `renderer-core` | Backend trait, config, error state, unit tests |
| `gles-backend` | Full resource Backend over system GLES 3.0+ (dlopen) |
| `vulkan-backend` | **Device discovery only** — loads `libvulkan`, enumerates and picks a physical device, creates a device + graphics queue, reports real info/limits. Cannot render (see below) |
| `gl-compat` `librust_gl.so` | GLES 3.x backend + OpenGL 3.3 compatibility entry-point layer + legacy fixed-function shims |
| Shader translate | Version rewrite, precision, texture2D→texture, gl_FragColor, attribute/varying |
| Format translate | BGRA swizzle, depth internal formats, clamp-to-border, BGRA8→RGBA8 storage |
| OpenGL 3.3 core API surface | Complete against the GL 3.0–3.3 core function list; unsupported desktop-only features return real GL errors instead of lying |
| Vulkan path | Device discovery + reporting; **no rendering path** |

### Does it run on a device? No.

**Tested on hardware.** Two device reports so far, and both moved the failure later rather
than persisting:

1. 1.16.5 + OptiFine: `SIGSEGV` at `si_addr = NULL` in `GL11.glFogfv`. A missing entry point
   resolved to null and LWJGL called it. Fixed by never returning null for a `gl*` name.
2. 1.21.11 + Sodium/Iris: the game now boots, initialises, reports
   `Mali-G77 MC9 / OpenGL ES 3.2 / 104 extensions`, and dies with a readable error:
   `IllegalStateException: OpenGL error 1282` at `GlBackend.createTexture` →
   `WindowFramebuffer.createDepthAttachment`. The log also named **62 unresolved entry
   points**, nearly all named-object/DSA spellings.

After the fix, report 3 (same device, same mod set) resolved **54 of those 62** entry points and
failed at the *same* place with the same `1282` — and crucially **no `GL error site` line
appeared**, which proves the error is raised by the **Mali driver**, not by this layer. Since
Minecraft only ever reports the numeric code, there was no way to tell which call provoked it,
so the bridge now keeps a ring buffer of the last 16 forwarded GL calls and dumps them when
`glGetError` returns non-zero (`RENDERER_TRACE_GL=1`).

Still open after report 3, and the next things to do:

- 8 entry points remain unresolved (`glCopyTextureSubImage2D/3D`,
  `glCompressedTextureSubImage2D/3D`, `glBlitNamedFramebuffer`, `glBindImageTextures`,
  `glTransformFeedbackBufferBase/Range`).
- **`glTexStorage2DMultisample` was missing entirely** — the call 1.20.5+ uses for the
  multisampled depth attachment in `WindowFramebuffer.createDepthAttachment`, which is exactly
  where this crash is. Now added, mapped onto `glTexImage2DMultisample` with null data, since
  GLES has no `*TexStorage*Multisample`.
- The `1282` itself is still unexplained. Turn on `RENDERER_TRACE_GL` and the next log will
  name the call instead of the code.

**None of this has been retested on a device.** Treat it as "the missing surface is filled in
and the next failure will be diagnosable", not as "it runs". Treat the host harness results as necessary but *not* sufficient — they did
not predict the device outcome, and they should not be read as evidence that the bridge works.

The host suite runs against Mesa's llvmpipe software rasteriser. That catches translation and
state bugs, but it cannot catch anything where a real driver differs from Mesa — and the paths
that are broken are exactly those: DSA vertex arrays, freshly created vertex arrays, the
fixed-function quad route, and scissored draws. Those all render or behave differently once a
real vendor driver is involved, so a green host run is not evidence about a phone.

### What has actually been verified

Verified by running it, on this machine:

- `cargo check --workspace` / `cargo test --workspace` — 33 unit tests pass.
- `cargo build -p gl-compat --release` links; `nm -D` shows **352 exported `gl*` entry points**
  with **no duplicate symbols**.
- `cargo run -p vulkan-backend --example probe` runs and reports a precise reason when no
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
cargo run -p vulkan-backend --example probe   # exits 1 when no loader is present
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
