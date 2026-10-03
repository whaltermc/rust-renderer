#!/usr/bin/env bash
# Requires: rustup target aarch64-linux-android, cargo-ndk, Android NDK, Gradle (or wrapper).
set -euo pipefail
cd "$(dirname "$0")"

cargo test -p renderer-core
cargo ndk -t arm64-v8a build --release -p gl-compat

install -D target/aarch64-linux-android/release/librust_gl.so \
    android/app/src/main/jniLibs/arm64-v8a/librust_gl.so

cd android
${GRADLE:-gradle} :app:assembleDebug
echo "APK: android/app/build/outputs/apk/debug/app-debug.apk"
