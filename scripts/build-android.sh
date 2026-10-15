#!/usr/bin/env bash
# Build the Rust core for Android and regenerate Kotlin bindings, then the APK.
# Usage: scripts/build-android.sh [debug|release]
set -euo pipefail
cd "$(dirname "$0")/.."
MODE=${1:-debug}
: "${ANDROID_HOME:?set ANDROID_HOME to your Android SDK}"
export ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)}
ABIS=${ABIS:-"arm64-v8a armeabi-v7a x86_64"}
args=(); for a in $ABIS; do args+=(-t "$a"); done
cargo ndk "${args[@]}" -P 29 -o android/app/src/main/jniLibs build --release -p dropbridge-ffi
LIB=$(ls target/*-linux-android*/release/libdropbridge_ffi.so | head -1)
cargo run -q -p dropbridge-ffi --bin uniffi-bindgen -- generate --library "$LIB" --language kotlin --out-dir android/app/src/main/java --no-format
cd android
if [ "$MODE" = release ]; then ./gradlew assembleRelease; else ./gradlew assembleDebug; fi
ls -1 app/build/outputs/apk/*/*.apk
