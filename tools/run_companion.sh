#!/usr/bin/env bash
# Launch a previously built app with its locally provisioned companion key.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
KEY="$ROOT/target/esp/ble-pairing-key"
if [[ ! -f "$KEY" ]]; then
    echo 'Provision the board first: bash tools/firmware.sh ble-flash PORT' >&2
    exit 1
fi
if [[ "$(uname -s)" == Darwin ]]; then
    APP="$ROOT/target/release/Token Café.app/Contents/MacOS/token-cafe"
else
    APP="$ROOT/target/release/token-cafe"
fi
if [[ ! -x "$APP" ]]; then
    echo 'Build first: sh tools/build_macos.sh (macOS), or cargo build --release (Linux).' >&2
    exit 1
fi
exec "$APP" --bluetooth --bluetooth-key "$KEY" "$@"
