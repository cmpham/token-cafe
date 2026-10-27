# Token Café for Android

Native Android companion for Android 8.0+ (API 26), ARM64 phones/tablets and
x86-64 emulators. The APK embeds the same Rust 480×272 café renderer as the
desktop and ESP32 versions. Animations, selection and task scrolling run on the
phone. Demo mode works without a desktop or network connection.

Live mode uses a USB cable and a read-only desktop bridge. Android cannot inspect
Claude/Codex processes or transcripts on your computer itself. This initial
version supports session states, names, task excerpts, branches, subagent counts,
and monthly usage. It does not dispatch approvals or open desktop chats. Wi-Fi,
Bluetooth, background notifications and Play Store publication are not included.

## Build

Install Rust 1.88+ via rustup, `cargo install cargo-ndk --version 4.1.2 --locked`, Python 3, JDK 17,
and use the included Gradle **8.11.1** wrapper. Through Android Studio's SDK Manager install:

- Android SDK Platform 35 and Build Tools 35.0.0
- NDK (Side by side) 28.0.13004108
- CMake 3.22.1 and Android SDK Platform Tools

Set `ANDROID_HOME` to the SDK directory and put its `platform-tools` on `PATH`.
Use JDK 17 for Gradle (`JAVA_HOME`); newer JDK versions may be incompatible with
this pinned Gradle version.

```sh
./tools/build_android.sh
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n cafe.token.android/.MainActivity
```

The build script first compiles Rust for both ABIs, then builds, lints and checks
native packaging and 16 KiB alignment in the APK.
Native libraries are ignored generated artifacts. To develop in Android Studio,
run `./tools/build_android.sh --native-only`, then open `android/` and use the
included Gradle wrapper.

The C JNI shim links the Rust library using CMake. Both libraries must support
16 KiB pages; the Rust build explicitly sets ELF alignment and NDK 28 handles the
JNI library. Runtime testing on a 16 KiB emulator/device is still pending.

## Connect live sessions

Enable USB debugging on your Android device and authorize your desktop.

```sh
cargo run --release --no-default-features --example android_bridge
```

In another terminal:

```sh
adb reverse tcp:38417 tcp:38417
```

Open the app, tap **Connect USB**, and paste the bridge's 64-character pairing
token. The bridge reads the same live collectors as the desktop app, polls
sessions once per second, and updates usage once per minute. Keep it running;
the desktop window itself need not be open. The token changes on every bridge
restart. With multiple devices, use `adb -s SERIAL reverse ...` for each one.

The bridge binds only to `127.0.0.1:38417`. It exports session names, branch names,
task excerpts and usage only to requests bearing that token. The app permits
cleartext HTTP only to `127.0.0.1`; ADB carries that connection over the authorized
USB link. Do not expose the bridge through a public or LAN port forward. The app
keeps the credential in memory, preserves it across rotation, and does not back
it up. Stop the bridge with Ctrl-C and remove forwarding with
`adb reverse --remove tcp:38417` when finished.

## Interaction and lifecycle

Tap a patron to select/clear their task, tap the footer to cycle patrons, and
drag the right-hand task scrollbar for long text. Rotate the phone for a larger
view; pixels use nearest-neighbor whole-number scaling whenever they fit.
Touch input ignores the letterbox and canceled/multiple-pointer gestures.
The app pauses rendering and polling in the background, then reconnects on
return. Failed reads clear the scene and display recovery instructions while
retrying. **Demo** disconnects live mode and restores the sample café.

## Validation

```sh
cargo test -p token-cafe-android --locked
cargo test --no-default-features --example android_bridge --locked
./tools/build_android.sh
```

For a repeatable end-to-end run, boot an Android emulator, install the debug APK,
build the bridge, and install Pillow in your Python environment. Then run:

```sh
cargo build --locked --no-default-features --example android_bridge
python3 tools/test_android_e2e.py --serial emulator-5554
```

Use the actual emulator serial shown by `adb devices`. The runner is restricted
to emulator serials, starts the real bridge on port 38417, creates temporary
Claude session/transcript files tied to a live test process, and exercises the
Android UI through ADB. That port must be free. It makes no model calls and
does not change your agent configuration. Evidence goes to `target/android-e2e`;
only synthetic demo screens and the controlled session's footer are saved.

An API 35 ARM64 emulator passed demo launch, synthetic live-session updates,
no-session snapshots, selection and task scrolling, portrait/landscape layout,
background/foreground, credential preservation across rotation, incompatible
snapshots, wrong credentials, forwarding loss/recovery and return to demo. See
[validation notes and screenshots](../docs/qa/android/README.md).

Physical USB cable behavior, real desktop-agent sessions on a phone, x86-64 runtime, Android 8 and 16 KiB runtime are
still unverified. Both ABIs compile and their APK/ELF alignment checks pass.
The [real-bridge simulator E2E run](../docs/qa/android/e2e-2026-10-22.md) passed all 16 checks, including bridge restart and token replacement.

Build configuration follows the [AGP 8.9 compatibility matrix](https://developer.android.com/build/releases/agp-8-9-0-release-notes),
[NDK/CMake guidance](https://developer.android.com/ndk/guides/cmake), and
[loopback network security configuration](https://developer.android.com/privacy-and-security/security-config).
