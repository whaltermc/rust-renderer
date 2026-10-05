#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL_DIR="$ROOT/tools/trace_replay"
BUILD_DIR="$ROOT/target/trace-replay"

case "${1:-}" in
    1.17)
        FIXTURE="minecraft-1.17-main-menu-854"
        GOLDEN="$TOOL_DIR/fixtures/$FIXTURE.0000117757.png"
        TARGET_CALL=117757
        SSIM_THRESHOLD=0.99
        TRACE_ARCHIVE="$TOOL_DIR/fixtures/$FIXTURE.tgz"
        ;;
    1.21.4)
        FIXTURE="minecraft-1.21.4-main-menu"
        CACHE_DIR="$BUILD_DIR/downloads"
        GOLDEN_NAME="$FIXTURE.0000481787.png"
        TARGET_CALL=481787
        SSIM_THRESHOLD=0.99
        TRACE_SHA256="d7d9d05e0b9542907e7c2259bccb3074b03f9a18e11c6d675397d8252893005c"
        GOLDEN_SHA256="0d3e85401b8763f6a78a59fd146f1d7deddf7c83db1450e2c1c25c5c888eae2e"
        REMOTE_FIXTURE=1
        ;;
    1.21.4-world)
        FIXTURE="minecraft-1.21.4-in-world"
        CACHE_DIR="$BUILD_DIR/downloads"
        GOLDEN_NAME="$FIXTURE.0000280000.png"
        TARGET_CALL=280000
        SSIM_THRESHOLD=0.99
        TRACE_SHA256="bb3af0453fa45f1fc4edbf757387616d2c7e4bd24e7dc9d91b7936c9c7ca92c8"
        GOLDEN_SHA256="e6bc1f2673b58ac4bcc80ab732098d548a43bb5f22dabe2f5cb3e0fb31d44979"
        REMOTE_FIXTURE=1
        ;;
    1.21.4-shaders)
        FIXTURE="minecraft-1.21.4-fabric-iris-bsl-in-world"
        CACHE_DIR="$BUILD_DIR/downloads"
        GOLDEN_NAME="$FIXTURE.0000110725.png"
        TARGET_CALL=110725
        SSIM_THRESHOLD=0.99
        TRACE_SHA256="5ff3414f8fa7a310add6b78c3bc6d5b11df85fee255b7d3d54f6a014447f1c03"
        GOLDEN_SHA256="5dcc2d4fd32778222615ac4220bbdaee48c5351c244eaa5373dea13f7c577173"
        REMOTE_FIXTURE=1
        ;;
    26.3)
        FIXTURE="improved-transparency-minecraft-26.3"
        GOLDEN="$TOOL_DIR/fixtures/$FIXTURE.0002667619.png"
        TARGET_CALL=2667619
        SSIM_THRESHOLD=0.995
        TRACE_ARCHIVE="$TOOL_DIR/fixtures/$FIXTURE.tgz"
        ;;
    *)
        echo "Usage: $0 {1.17|1.21.4|1.21.4-world|1.21.4-shaders|26.3}" >&2
        exit 2
        ;;
esac

for tool in cargo cmake tar sha256sum; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "Required command not found: $tool" >&2
        exit 1
    }
done

TRACE_DIR="$BUILD_DIR/fixtures/$1"
OUTPUT_DIR="$BUILD_DIR/results/$1"
mkdir -p "$TRACE_DIR" "$OUTPUT_DIR"
if [[ "${REMOTE_FIXTURE:-0}" == "1" ]]; then
    command -v curl >/dev/null 2>&1 || {
        echo "Required command not found: curl" >&2
        exit 1
    }
    mkdir -p "$CACHE_DIR"
    GOLDEN="$CACHE_DIR/$GOLDEN_NAME"
    TRACE_ARCHIVE="$CACHE_DIR/$FIXTURE.tgz"
    TRACE_URL="https://media.githubusercontent.com/media/MobileGL-Dev/MobileGL/dev/tools/trace_replay/fixtures/$FIXTURE.tgz"
    GOLDEN_URL="https://media.githubusercontent.com/media/MobileGL-Dev/MobileGL/dev/tools/trace_replay/fixtures/$GOLDEN_NAME"
    if [[ ! -f "$TRACE_ARCHIVE" ]]; then
        curl -fLsS --retry 3 "$TRACE_URL" -o "$TRACE_ARCHIVE.tmp"
        mv "$TRACE_ARCHIVE.tmp" "$TRACE_ARCHIVE"
    fi
    if [[ ! -f "$GOLDEN" ]]; then
        curl -fLsS --retry 3 "$GOLDEN_URL" -o "$GOLDEN.tmp"
        mv "$GOLDEN.tmp" "$GOLDEN"
    fi
    echo "$TRACE_SHA256  $TRACE_ARCHIVE" | sha256sum --check --status || {
        echo "Minecraft 1.21.4 trace checksum mismatch: $TRACE_ARCHIVE" >&2
        exit 1
    }
    echo "$GOLDEN_SHA256  $GOLDEN" | sha256sum --check --status || {
        echo "Minecraft 1.21.4 golden checksum mismatch: $GOLDEN" >&2
        exit 1
    }
fi
if [[ ! -f "$TOOL_DIR/vendor/apitrace/thirdparty/snappy/COPYING" ]]; then
    echo "Initialize the replay dependency first:" >&2
    echo "  git submodule update --init --recursive tools/trace_replay/vendor/apitrace" >&2
    exit 1
fi

cargo build --release -p gl-compat

# The retrace has to see the context the capture recorded, which is an ES context. With the
# spoof on, glGetString reports a desktop "3.3 (Core Profile)" string, the replay tool takes a
# desktop framebuffer readback path against an ES context, and the snapshot comes back empty
# ("failed to get snapshot") even though every call replayed. Spoofing is for the game, not
# for a recorded capture.
export RENDERER_SPOOF_GL=0
cmake -S "$TOOL_DIR" -B "$BUILD_DIR"
cmake --build "$BUILD_DIR" --target rust_renderer_trace_replay --parallel

if [[ ! -f "$TRACE_DIR/trace.trace" ]]; then
    tar -xzf "$TRACE_ARCHIVE" -C "$TRACE_DIR"
fi

export EGL_PLATFORM="${EGL_PLATFORM:-surfaceless}"
export LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-1}"
export MESA_GL_VERSION_OVERRIDE="${MESA_GL_VERSION_OVERRIDE:-4.6}"
export MESA_GLSL_VERSION_OVERRIDE="${MESA_GLSL_VERSION_OVERRIDE:-460}"

"$BUILD_DIR/rust_renderer_trace_replay" \
    --trace "$TRACE_DIR/trace.trace" \
    --golden "$GOLDEN" \
    --target-call "$TARGET_CALL" \
    --width 854 \
    --height 480 \
    --ssim-threshold "$SSIM_THRESHOLD" \
    --renderer-library "$ROOT/target/release/librust_gl.so" \
    --backend DirectGLES \
    --output "$OUTPUT_DIR"