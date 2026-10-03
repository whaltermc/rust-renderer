#!/data/data/com.termux/files/usr/bin/bash
set -e
cd "$(dirname "$0")"

# 1. Rust library
cargo build --release -p gl-compat

# 2. Fresh working dir
rm -rf build && mkdir -p build/lib/arm64-v8a && cd build
cp ../target/release/librust_gl.so lib/arm64-v8a/

# 3. Tools: android.jar and a signing key (created once, reused)
[ -f ../android.jar ] || curl -L -o ../android.jar \
  https://github.com/Sable/android-platforms/raw/master/android-33/android.jar
[ -f ../debug.keystore ] || keytool -genkeypair -keystore ../debug.keystore \
  -storepass android -alias key -keypass android -keyalg RSA -keysize 2048 \
  -validity 10000 -dname "CN=debug"

# 4. Manifest
cat > AndroidManifest.xml <<'XML'
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="dev.rustrenderer.plugin">
    <application android:label="Rust Renderer"
        android:hasCode="false"
        android:extractNativeLibs="true">
        <meta-data android:name="fclPlugin" android:value="true"/>
        <meta-data android:name="zalithRendererPlugin" android:value="true"/>
        <meta-data android:name="renderer" android:value="Rust Renderer:librust_gl.so:libEGL.so"/>
        <meta-data android:name="pojavEnv" android:value="POJAV_RENDERER=opengles3:RENDERER_BACKEND=auto:RENDERER_DEBUG=0"/>
    </application>
</manifest>
XML

# 5. Package, sign, copy to shared storage
aapt2 link -o app.apk -I ../android.jar --manifest AndroidManifest.xml \
  --min-sdk-version 26 --target-sdk-version 29 \
  --version-code 3 --version-name 0.1.2
zip app.apk lib/arm64-v8a/librust_gl.so
apksigner sign --ks ../debug.keystore --ks-pass pass:android app.apk
apksigner verify app.apk
cp app.apk ~/storage/downloads/rust-renderer.apk
echo "Done: ~/storage/downloads/rust-renderer.apk"
