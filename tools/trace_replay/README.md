# Rust Renderer trace replay

This is a Linux desktop port of the [MobileGL trace replay tool](https://github.com/MobileGL-Dev/MobileGL/tree/dev/tools/trace_replay).
It loads this project's `librust_gl.so`, replays an apitrace capture, saves the rendered
image, and compares it with the capture's reference image.

The bundled fixtures cover Minecraft 1.17's main menu and Minecraft 26.3's
improved-transparency scene. They come from the upstream MobileGL repository; the upstream
fixture corpus is much larger, so only these two version-boundary cases are included here.

## Run a fixture

The first run needs CMake, a C/C++ compiler, Python 3, Cargo, and the desktop Mesa EGL/GLES
runtime. The apitrace source is a pinned submodule; initialize it once:

```sh
git submodule update --init --recursive tools/trace_replay/vendor/apitrace
```

Then run either replay:

```sh
tools/trace_replay/run_fixture.sh 1.17
tools/trace_replay/run_fixture.sh 26.3
```

The script builds the host `gl-compat` library and replay executable, extracts the selected
trace under `target/trace-replay/`, and writes the actual image and comparison result there.
To invoke the runner directly:

```sh
target/trace-replay/rust_renderer_trace_replay \
  --trace target/trace-replay/fixtures/1.17/trace.trace \
  --golden tools/trace_replay/fixtures/minecraft-1.17-main-menu-854.0000117757.png \
  --target-call 117757 \
  --width 854 --height 480 \
  --renderer-library target/release/librust_gl.so \
  --output target/trace-replay/manual-1.17
```

The 26.3 fixture uses target call `2667619` and the matching PNG in `fixtures/`.
Set `RENDERER_TRACE_GL=1` before running to include the renderer's recent GL calls when it
reports an OpenGL error.

## What a passing result means

A passing trace confirms the captured GL call sequence rendered a sufficiently similar frame
on the host's EGL/GLES driver. It is a regression check, not proof that every Minecraft build
from 1.17 through 26.3 launches on Android: launcher/JVM behavior, Android GPU drivers, and
unrepresented game paths still need device tests. The renderer's Vulkan backend remains
device discovery only; this runner tests its GLES path.

## Attribution and license

The replay implementation and selected captures are copied from MobileGL-Dev/MobileGL
(`dev`, including its pinned apitrace dependency). MobileGL is LGPL-3.0-or-later; see
`COPYING` and `COPYING.LESSER` here. The apitrace source is a separate pinned submodule with
its own license files.