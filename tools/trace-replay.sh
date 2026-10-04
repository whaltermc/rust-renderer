#!/usr/bin/env bash
# Replay real Minecraft GL captures through librust_gl.so.
#
# This is the replay harness: apitrace's eglretrace recreates the recorded context and
# re-issues every recorded call. Preloading librust_gl.so puts this bridge in the path
# (eglretrace resolves GL entry points through eglGetProcAddress, which the library exports),
# so the frames are rendered by the same code Android runs.
#
# Two settings are load-bearing and were found the hard way:
#
#   RENDERER_SPOOF_GL=0
#     The trace recorded whatever context the device created (ES 3.0 for 1.17, a 4.6 core
#     context for 1.21.x, which is why Zalith sets MESA_GL_VERSION_OVERRIDE=4.6). eglretrace
#     checks that the replay context reports the same version, so spoofing 3.3 makes replay
#     refuse to start on an ES 3.0 capture.
#
#   MESA_GL_VERSION_OVERRIDE / MESA_GLSL_VERSION_OVERRIDE
#     Needed for the 1.21.x captures, which were recorded against a 4.6 core context. The
#     software rasteriser tops out lower, so the version has to be overridden for the context
#     request to be satisfiable at all.
#
# Usage: tools/trace-replay.sh [fixture.tgz ...]

set -uo pipefail
cd "$(dirname "$0")/.."

SO="${SO:-target/release/librust_gl.so}"
REPO="MobileGL-Dev/MobileGL"
FIXTURES_PATH="tools/trace_replay/fixtures"
RAW="https://raw.githubusercontent.com/$REPO/dev/$FIXTURES_PATH"

# name:context-gl-version, chosen per capture based on the context it recorded.
DEFAULT_FIXTURES=(
    "minecraft-1.17-main-menu-854.tgz:3.0"
    "minecraft-1.21.1-neoforge-create-indirect-in-world-align1024.tgz:4.6"
)

for tool in apitrace Xvfb curl; do
    command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 2; }
done
[ -f "$SO" ] || { echo "$SO not built; run cargo build --release -p gl-compat" >&2; exit 2; }
SO="$(readlink -f "$SO")"

started_xvfb=0
if [ -z "${DISPLAY:-}" ]; then
    DISPLAY=:99
    Xvfb "$DISPLAY" -screen 0 1280x720x24 >/dev/null 2>&1 &
    started_xvfb=$!
    export DISPLAY
    sleep 2
fi
cleanup() { [ "$started_xvfb" -ne 0 ] && kill "$started_xvfb" 2>/dev/null; rm -rf "$WORK"; }
WORK="$(mktemp -d)"
trap cleanup EXIT

# Git LFS stores a pointer file; the real bytes come from the batch API.
fetch() {
    local name=$1 out="$WORK/$1"
    [ -s "$out" ] && { echo "$out"; return 0; }
    local oid size href
    oid=$(curl -sSfL "$RAW/$name" | sed -n 's/^oid sha256://p')
    size=$(curl -sSfL "$RAW/$name" | sed -n 's/^size //p')
    [ -n "$oid" ] && [ -n "$size" ] || return 1
    href=$(curl -sS -X POST "https://github.com/$REPO.git/info/lfs/objects/batch" \
        -H "Accept: application/vnd.git-lfs+json" \
        -H "Content-Type: application/vnd.git-lfs+json" \
        -d "{\"operation\":\"download\",\"transfers\":[\"basic\"],\"objects\":[{\"oid\":\"$oid\",\"size\":$size}]}" \
        | sed -n 's/.*"href" *: *"\([^"]*\)".*/\1/p' | head -1)
    [ -n "$href" ] || return 1
    curl -sSfL "$href" -o "$out" || return 1
    echo "$out"
}

pass=0
fail=0
replayed=0

for entry in "${@:-${DEFAULT_FIXTURES[@]}}"; do
    name="${entry%%:*}"
    want="${entry##*:}"
    [ "$want" = "$entry" ] && want=3.0
    echo "== replaying $name (recorded context: GL $want)"

    archive="$(fetch "$name")" || { echo "  fetch failed"; fail=$((fail + 1)); continue; }
    mkdir -p "$WORK/$name.d" && tar xzf "$archive" -C "$WORK/$name.d" 2>/dev/null \
        || { echo "  unpack failed"; fail=$((fail + 1)); continue; }
    trace="$(find "$WORK/$name.d" -name '*.trace' -size +1k | head -1)"
    [ -n "$trace" ] || { echo "  no trace inside"; fail=$((fail + 1)); continue; }

    if [ "$want" = "4.6" ]; then
        gl_ver=4.6; glsl_ver=460
    else
        gl_ver=3.0; glsl_ver=300
    fi

    log="$WORK/$(basename "$name").log"
    LIBGL_ALWAYS_SOFTWARE=1 \
    RENDERER_SPOOF_GL=0 \
    MESA_GL_VERSION_OVERRIDE="$gl_ver" \
    MESA_GLSL_VERSION_OVERRIDE="$glsl_ver" \
    LD_PRELOAD="$SO" \
        apitrace replay --benchmark "$trace" >"$log" 2>&1
    status=$?

    if grep -q "^Rendered .* frames" "$log"; then
        summary="$(grep -o 'Rendered.*' "$log" | head -1)"
        echo "  PASS  $summary"
        grep -E "^\[RustRenderer\] \[(GLBridge\] (Missing|unresolved)|GLCompat\] (DSA|dsa))" "$log" \
            | sed 's/^/    /' | sort -u | head -5
        pass=$((pass + 1))
        replayed=$((replayed + 1))
    else
        echo "  FAIL  (exit $status)"
        tail -4 "$log" | sed 's/^/    /'
        fail=$((fail + 1))
    fi
    cp "$log" "${LOG_DEST:-/dev/null}" 2>/dev/null || true
done

echo
if [ "$replayed" -eq 0 ]; then
    echo "no trace was replayed: that is a failure, not a pass." >&2
    exit 1
fi
echo "$pass replayed, $fail failed"
[ "$fail" -eq 0 ]