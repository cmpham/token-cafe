#!/usr/bin/env bash
# Launch a previously built app against the boards in its provisioned semantic registry.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
export TC_BLE_REGISTRY="${TC_BLE_REGISTRY:-$ROOT/target/esp/v2-pairing/production.json}"
if [[ "$(uname -s)" == Darwin ]]; then
    APP="$(sh "$ROOT/tools/macos_app_path.sh")/Contents/MacOS/token-cafe"
else
    APP="$ROOT/target/release/token-cafe"
fi
if [[ ! -x "$APP" ]]; then
    echo 'Build first: sh tools/build_macos.sh (macOS), or cargo build --release (Linux).' >&2
    exit 1
fi
if [[ -f "$TC_BLE_REGISTRY" ]]; then
    exec "$APP" --bluetooth "$@"
else
    exec "$APP" "$@"
fi
