#!/usr/bin/env bash
# Requires: rustup target aarch64-linux-android, cargo-ndk, Android NDK, Gradle (or wrapper).
set -euo pipefail
cd "$(dirname "$0")"

cargo test -p renderer-core
cargo ndk -t arm64-v8a build --release -p gl-compat

install -D target/aarch64-linux-android/release/librust_gl.so \
    android/app/src/main/jniLibs/arm64-v8a/librust_gl.so

install -D target/aarch64-linux-android/release/librust_gl.so \
    android/renderer/src/main/jniLibs/arm64-v8a/librust_gl.so

cd android
# Prefer the pinned wrapper: AGP 8.5.2 targets Gradle 8.x, so a machine with Gradle 9
# installed can otherwise fail in ways that have nothing to do with this project.
if [ -x ./gradlew ]; then
    GRADLE_CMD=./gradlew
else
    GRADLE_CMD="${GRADLE:-gradle}"
fi
"$GRADLE_CMD" :app:packageRustGlApk
echo "APK: android/app/build/outputs/apk/RustGL.apk"

# Build the renderer library AAR (requires Android SDK / local.properties).
if [ -f local.properties ] || [ -n "${ANDROID_HOME:-}" ] || [ -n "${ANDROID_SDK_ROOT:-}" ]; then
    "$GRADLE_CMD" :renderer:assembleRelease :app:packageRustGlAar
    echo "AAR: android/app/build/outputs/aar/RustGL.aar"
else
    echo "SKIP: AAR build needs Android SDK (ANDROID_HOME / local.properties)"
fi
