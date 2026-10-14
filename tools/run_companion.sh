#!/usr/bin/env bash
# Launch a previously built app with its locally provisioned companion key.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
export TC_BLE_REGISTRY="${TC_BLE_REGISTRY:-$ROOT/target/esp/v2-pairing/production.json}"
if [[ ! -f "$TC_BLE_REGISTRY" ]]; then
    echo 'Provision semantic firmware and its board registry first; see docs/protocol/companion-v2.md.' >&2
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
exec "$APP" --bluetooth "$@"
