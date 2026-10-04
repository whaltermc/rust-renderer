# Vendored native dependencies

## glslang (pinned: 15.0.0)

GLSL → SPIR-V compiler, Khronos. Vendored as a submodule, **Apache-2.0** — see
`glslang/LICENSE.txt`. It is inert today: no part of the Rust build references it, so
`cargo build` and `cargo test` are unaffected.

### Why it is not simply ready to build

Two things are missing, and neither can be resolved without an Android NDK:

1. **SPIRV-Tools is not tracked.** glslang's SPIR-V backend needs
   `External/spirv-tools` at the commit its `known_good.json` pins
   (`6dcc7e350a0b9871a825414d42329e44b0eb8109`). glslang has no `.gitmodules` for it —
   it is fetched at build time by `update_deps.py`, and glslang's `.gitignore` already
   covers `External/`, so it stays invisible to git. Fetch it with:

   ```sh
   git clone --depth 1 https://github.com/KhronosGroup/SPIRV-Tools.git \
       third_party/glslang/External/spirv-tools
   git -C third_party/glslang/External/spirv-tools checkout 6dcc7e350a0b9871a825414d42329e44b0eb8109
   ```

2. **Cross-compiling to arm64 needs CMake and the NDK toolchain**, neither of which exists
   in the container this was set up from. The invocation it needs is roughly:

   ```sh
   cmake -S third_party/glslang -B build/glslang \
       -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
       -DANDROID_ABI=arm64-v8a -DANDROID_PLATFORM=android-24 \
       -DCMAKE_BUILD_TYPE=Release \
       -DENABLE_GLSLANG_BINARIES=OFF \
       -DENABLE_HLSL=OFF \
       -DBUILD_SHARED_LIBS=OFF \
       -DSPIRV_SKIP_TESTS=ON -DSPIRV_SKIP_EXECUTABLES=ON
   cmake --build build/glslang --target SPIRV-Tools-opt
   ```

   That has never been run here, so it is unverified.

### What still needs doing

- A Cargo `build.rs` in `crates/backend` that compiles the above and links it, gated behind
  a cargo feature that is **off by default**, so the Android APK build is unaffected until
  someone can verify it.
- FFI bindings for `glslang::Compiler` / `SpvTools` and a `compile_shader` path that
  consumes `shader-translate`'s output.
- Until all of that exists, `backend::vulkan::VulkanBackend::can_render()` correctly stays
  `false` and `compile_shader` returns *"Vulkan needs GLSL compiled to SPIR-V; no SPIR-V
  compiler is linked"*. Nothing about the renderer's behaviour has changed.

The reason for taking the pure-Rust route (`naga`) instead was that it needs no C++
toolchain and could be verified here — but its `glsl-in` feature does not compile at the
current release (30.0.1), failing with `method apply_default_interpolation not found`. That is
an upstream regression, so glslang remains the reference fallback.

## tools/trace_replay/vendor/apitrace

Pre-existing, unrelated to this directory: MobileGL's replay tool, LGPL-3.0, pinned as a
submodule.