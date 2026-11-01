#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
BUILD="$ROOT/target/pipar-reference/zmk-via"
REV=9f8796671ae5843277e0daaaf711819499ad4a90
IMAGE=zmkfirmware/zmk-build-arm@sha256:edb1c953438c6f720ddb79c3762f3972013b7fbbaf4fff3592fc869983e7afc5
if [[ ! -d "$BUILD/.git" ]]; then
  git clone https://github.com/K4zoku/ZMK-VIA.git "$BUILD"
  git -C "$BUILD" checkout --detach "$REV"
fi
[[ "$(git -C "$BUILD" rev-parse HEAD)" == "$REV" ]] || { echo 'Unexpected VIA revision; use the pinned revision in an isolated build directory.' >&2; exit 1; }
docker run --rm -v "$BUILD:/workspace" -v "$ROOT:/project:ro" -w /workspace "$IMAGE" bash -lc '
  set -euo pipefail
  cp /project/firmware/keypad/west-lock.yml config/west.yml
  if [ ! -d .west ]; then west init -l config; fi
  if [ ! -f .cafe-dependencies-ready ]; then
    west update --narrow -o=--depth=1
    touch .cafe-dependencies-ready
  fi
  bash scripts/apply-patches.sh
  python3 /project/tools/patch_keypad_via.py /workspace/config/src/via_zmk.c
  python3 /project/tools/patch_keypad_input.py /workspace/zmk/app/src/hid_listener.c /workspace/zmk/app/src/ble.c
  west zephyr-export
  west build -p always -s zmk/app -b nice_nano_v2 -d build/cafe -- \
    -DSHIELD="pipar_flake raw_hid_adapter" \
    -DZMK_CONFIG=/project/firmware/keypad/config \
    -DZMK_EXTRA_MODULES="/workspace/config;/project/firmware/keypad/module"
'
