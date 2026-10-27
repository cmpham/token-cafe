#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

command -v cargo >/dev/null || { echo 'Install Rust with rustup first.' >&2; exit 1; }
command -v python3 >/dev/null || { echo 'Python 3 is needed for APK verification.' >&2; exit 1; }
command -v rustup >/dev/null || { echo 'rustup is needed to install Android targets.' >&2; exit 1; }
cargo ndk --version >/dev/null 2>&1 || { echo 'Install cargo-ndk: cargo install cargo-ndk --locked' >&2; exit 1; }
: "${ANDROID_HOME:?Set ANDROID_HOME to your Android SDK directory}"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.0.13004108"
test -d "$ANDROID_NDK_HOME" || { echo 'Install NDK 28.0.13004108 with Android Studio SDK Manager.' >&2; exit 1; }
rustup target add aarch64-linux-android x86_64-linux-android
# Align the Rust shared object as well as the JNI library for 16 KiB devices.
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-soname,libtoken_cafe_android.so"
cargo ndk -t arm64-v8a -t x86_64 --platform 26 -o android/native build --release --locked -p token-cafe-android
if [[ "${1:-}" == '--native-only' ]]; then exit 0; fi
cd android
if [[ -x ./gradlew ]]; then
    ./gradlew assembleDebug lintDebug
elif command -v gradle >/dev/null; then
    gradle assembleDebug lintDebug
else
    echo 'Native libraries built. Install Gradle 8.11.1, or generate its wrapper in android/.' >&2
    exit 1
fi
cd ..
python3 tools/verify_android_apk.py android/app/build/outputs/apk/debug/app-debug.apk
echo 'APK: android/app/build/outputs/apk/debug/app-debug.apk'
