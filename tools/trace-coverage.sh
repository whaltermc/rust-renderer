#!/usr/bin/env bash
# Measure GL entry-point coverage against real Minecraft traces.
#
# MobileGL ships apitrace captures of Minecraft (1.17, 1.21.1, 1.21.4, 1.21.11, 26.3, some with
# Sodium/Iris) plus golden frames. Their own `mobilegl_trace_replay` cannot be pointed at this
# library -- it is hard-wired to MobileGL's renderer via `--mobilegl-library` -- and this
# container's apitrace is GLX-only, so frame-accurate replay is not available here.
#
# What *is* available, and what matters for this layer, is the call stream: the set of GL
# entry points a real Minecraft run actually asks for. A missing one is not a cosmetic gap --
# it is a null function pointer and a SIGSEGV, which is how 1.16.5 died. So this compares the
# functions each trace calls against what librust_gl.so exports.
#
# Usage:  tools/trace-coverage.sh [fixture ...]
# With no arguments it walks the default fixture list.

set -uo pipefail
cd "$(dirname "$0")/.."

SO="${SO:-target/release/librust_gl.so}"
FIXTURE_REPO="MobileGL-Dev/MobileGL"
FIXTURE_PATH="tools/trace_replay/fixtures"
RAW="https://raw.githubusercontent.com/$FIXTURE_REPO/dev/$FIXTURE_PATH"

DEFAULT_FIXTURES=(
    "minecraft-1.17-main-menu-854.tgz"
    "minecraft-1.21.1-neoforge-create-indirect-in-world-align1024.tgz"
)

command -v apitrace >/dev/null || {
    echo "apitrace is required: sudo apt-get install -y apitrace" >&2
    exit 2
}
[ -f "$SO" ] || { echo "$SO not built; run cargo build --release -p gl-compat" >&2; exit 2; }

EXPORTS="$(mktemp)"
trap 'rm -f "$EXPORTS"; rm -rf "$WORK"' EXIT
nm -D --defined-only "$SO" | awk '$2 == "T" { print $3 }' | sort -u > "$EXPORTS"

WORK="$(mktemp -d)"

# Fetches a fixture through the Git LFS batch API: the repository stores Git LFS pointers, so
# a plain raw download yields a 133-byte text file rather than the trace.
fetch_fixture() {
    local name="$1" out="$WORK/$1"
    [ -s "$out" ] && { echo "$out"; return 0; }
    local oid size href
    oid=$(curl -sSfL "$RAW/$name" | sed -n 's/^oid sha256://p')
    size=$(curl -sSfL "$RAW/$name" | sed -n 's/^size //p')
    [ -n "$oid" ] && [ -n "$size" ] || return 1
    href=$(curl -sS -X POST "https://github.com/$FIXTURE_REPO.git/info/lfs/objects/batch" \
        -H "Accept: application/vnd.git-lfs+json" \
        -H "Content-Type: application/vnd.git-lfs+json" \
        -d "{\"operation\":\"download\",\"transfers\":[\"basic\"],\"objects\":[{\"oid\":\"$oid\",\"size\":$size}]}" \
        | sed -n 's/.*"href" *: *"\([^"]*\)".*/\1/p' | head -1)
    [ -n "$href" ] || return 1
    curl -sSfL "$href" -o "$out" || return 1
    echo "$out"
}

total_missing=0
checked=0
fixtures=("${@:-${DEFAULT_FIXTURES[@]}}")

for fixture in "${fixtures[@]}"; do
    echo "== $fixture"
    archive="$(fetch_fixture "$fixture")" || { echo "  could not fetch (skipping)"; continue; }

    inner="$WORK/$(basename "$fixture" .tgz).trace"
    tar xzf "$archive" -C "$WORK" 2>/dev/null || { echo "  could not unpack (skipping)"; continue; }
    # The archive may contain any *.trace name; take the largest one unpacked.
    inner="$(find "$WORK" -name '*.trace' -size +1k -printf '%s %p\n' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2-)"
    [ -n "$inner" ] && [ -s "$inner" ] || { echo "  no trace inside (skipping)"; continue; }
    checked=$((checked + 1))

    called="$WORK/$(basename "$fixture").called"
    # Lines look like: "42 glBindTexture(target = 0xDE1, texture = 3)"
    apitrace dump "$inner" 2>/dev/null \
        | grep -oE '^[0-9]+ gl[A-Za-z0-9_]+' \
        | awk '{ print $2 }' | sort -u > "$called"

    total=$(wc -l < "$called")
    if [ "$total" -eq 0 ]; then
        echo "  no gl* calls parsed (skipping)"
        continue
    fi
    missing="$WORK/$(basename "$fixture").missing"
    while read -r fn; do
        grep -qx "$fn" "$EXPORTS" || echo "$fn"
    done < "$called" > "$missing"
    count=$(grep -c . "$missing" || true)

    printf '  %d distinct gl* entry points called, %d not exported (%.1f%% covered)\n' \
        "$total" "$count" "$(awk -v t="$total" -v m="$count" 'BEGIN { printf (t-m)*100/t }')"
    if [ "$count" -gt 0 ]; then
        sed 's/^/    missing: /' "$missing"
        total_missing=$((total_missing + count))
    fi
done

# Skipping every fixture must not look like success.
if [ "$checked" -eq 0 ]; then
    echo
    echo "no trace could be checked: this is a failure, not a pass." >&2
    exit 1
fi

if [ "$total_missing" -gt 0 ]; then
    echo
    echo "$total_missing entry point(s) that a real Minecraft run calls are not exported."
    echo "Each one is a NULL function pointer for a client that resolves by dlsym."
    exit 1
fi
echo
echo "every gl* entry point called by the checked traces is exported."