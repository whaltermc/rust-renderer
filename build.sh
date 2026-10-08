#!/usr/bin/env bash
# Requires: rustup targets for Android, cargo-ndk, Android NDK, Gradle (or wrapper).
set -euo pipefail
cd "$(dirname "$0")"

cargo test -p renderer-core

# Build for all Android ABIs
ABIS=("arm64-v8a" "armeabi-v7a" "x86" "x86_64")
RUST_TARGETS=("aarch64-linux-android" "armv7-linux-androideabi" "i686-linux-android" "x86_64-linux-android")

for i in "${!ABIS[@]}"; do
    ABI="${ABIS[$i]}"
    RUST_TARGET="${RUST_TARGETS[$i]}"
    echo "Building for $ABI ($RUST_TARGET)..."
    cargo ndk -t "$ABI" build --release -p gl-compat

    install -D "target/$RUST_TARGET/release/librust_gl.so" \
        "android/app/src/main/jniLibs/$ABI/librust_gl.so"
    install -D "target/$RUST_TARGET/release/librust_gl.so" \
        "android/renderer/src/main/jniLibs/$ABI/librust_gl.so"
done

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
