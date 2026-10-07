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
        ;;
    26.3)
        FIXTURE="improved-transparency-minecraft-26.3"
        GOLDEN="$TOOL_DIR/fixtures/$FIXTURE.0002667619.png"
        TARGET_CALL=2667619
        SSIM_THRESHOLD=0.995
        ;;
    *)
        echo "Usage: $0 {1.17|26.3}" >&2
        exit 2
        ;;
esac

for tool in cargo cmake tar; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "Required command not found: $tool" >&2
        exit 1
    }
done

TRACE_DIR="$BUILD_DIR/fixtures/$1"
OUTPUT_DIR="$BUILD_DIR/results/$1"
mkdir -p "$TRACE_DIR" "$OUTPUT_DIR"
if [[ ! -f "$TOOL_DIR/vendor/apitrace/thirdparty/snappy/COPYING" ]]; then
    echo "Initialize the replay dependency first:" >&2
    echo "  git submodule update --init --recursive tools/trace_replay/vendor/apitrace" >&2
    exit 1
fi

cargo build --release -p gl-compat
cmake -S "$TOOL_DIR" -B "$BUILD_DIR"
cmake --build "$BUILD_DIR" --target rust_renderer_trace_replay --parallel

if [[ ! -f "$TRACE_DIR/trace.trace" ]]; then
    tar -xzf "$TOOL_DIR/fixtures/$FIXTURE.tgz" -C "$TRACE_DIR"
fi

export EGL_PLATFORM="${EGL_PLATFORM:-surfaceless}"
export LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-1}"
export MESA_GL_VERSION_OVERRIDE="${MESA_GL_VERSION_OVERRIDE:-3.3}"
export MESA_GLSL_VERSION_OVERRIDE="${MESA_GLSL_VERSION_OVERRIDE:-330}"

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