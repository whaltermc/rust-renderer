#!/usr/bin/env bash
# Host-side smoke test for the built GL bridge.
#
# Renders a triangle through librust_gl.so on a headless GLES context and checks the pixels,
# so GL changes can be validated without a device or a Minecraft launch. Needs Mesa's
# software rasteriser:
#
#   sudo apt-get install -y libegl1 libgles2 libegl1-mesa-dev libgles2-mesa-dev
#
# Only EGL is linked; every GL entry point is resolved from the bridge at runtime, so no
# check can accidentally pass by reaching the driver directly.
set -euo pipefail
cd "$(dirname "$0")/.."

SO="${1:-target/release/librust_gl.so}"
OUT="${TMPDIR:-/tmp}/glsmoke"

echo "building $SO"
cargo build --release -p gl-compat

command -v cc >/dev/null || { echo "cc is required"; exit 1; }
cc -O1 -o "$OUT" tools/glsmoke.c -lEGL -ldl -lm

# llvmpipe: there is no GPU here, and a real one is not needed for correctness checks.
LIBGL_ALWAYS_SOFTWARE=1 "$OUT" "$SO"
status=$?

if [ $status -eq 77 ]; then
    echo "no GL driver available; nothing was tested"
fi
exit $status
