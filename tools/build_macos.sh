#!/bin/sh
# Build a locally ad-hoc-signed app bundle for Finder / Dock launching.
set -eu
if [ "$(uname -s)" != Darwin ]; then
    echo 'This script must run on macOS.' >&2
    exit 1
fi
cd "$(dirname "$0")/.."
cargo build --locked --release
build_dir="${CARGO_TARGET_DIR:-target}/release"
# Keep one app location across worktrees and Cargo clean/build operations.
app_dir=$(sh tools/macos_app_path.sh)
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources"
swiftc -O native/desktop/*.swift -o "$app_dir/Contents/MacOS/keypad-desktop.new"
mv "$app_dir/Contents/MacOS/keypad-desktop.new" "$app_dir/Contents/MacOS/keypad-desktop"
swiftc -O native/keypad/*.swift -o "$app_dir/Contents/MacOS/keypad-hid.new"
mv "$app_dir/Contents/MacOS/keypad-hid.new" "$app_dir/Contents/MacOS/keypad-hid"
cp assets/TokenCafe.icns "$app_dir/Contents/Resources/TokenCafe.icns"
# Replace the executable inode so macOS does not reuse cached code-signing pages.
cp "$build_dir/token-cafe" "$app_dir/Contents/MacOS/token-cafe.new"
mv "$app_dir/Contents/MacOS/token-cafe.new" "$app_dir/Contents/MacOS/token-cafe"
cat > "$app_dir/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
    <key>CFBundleExecutable</key><string>token-cafe</string>
    <key>CFBundleIdentifier</key><string>local.token-cafe.desktop</string>
    <key>CFBundleName</key><string>Token Café</string>
    <key>CFBundleDisplayName</key><string>Token Café</string>
    <key>CFBundleIconFile</key><string>TokenCafe.icns</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>NSBluetoothAlwaysUsageDescription</key><string>Connect your Token Café keypad and optional companion display.</string>
    <key>NSBluetoothPeripheralUsageDescription</key><string>Connect your Token Café keypad and optional companion display.</string>
    <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
# Bind the bundle identifier and Info.plist, instead of retaining Cargo's
# linker-generated executable identity. Sign nested helpers before the app.
codesign --force --sign - --identifier local.token-cafe.desktop.keypad-desktop "$app_dir/Contents/MacOS/keypad-desktop"
codesign --force --sign - --identifier local.token-cafe.desktop.keypad-hid "$app_dir/Contents/MacOS/keypad-hid"
codesign --force --sign - --identifier local.token-cafe.desktop "$app_dir"
codesign --verify --deep --strict "$app_dir"
printf 'Built %s\n' "$app_dir"
