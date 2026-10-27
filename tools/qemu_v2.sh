#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
export IDF_TOOLS_PATH="$ROOT/target/esp/tools"
export IDF_PATH="$ROOT/target/esp/esp-idf"
export IDF_PYTHON_ENV_PATH="$IDF_TOOLS_PATH/python_env/idf5.4_py3.12_env"
export PATH="$IDF_PYTHON_ENV_PATH/bin:$PATH"
set +u
source "$IDF_PATH/export.sh" >/dev/null
set -u
case "${1:-build}" in
 build)
  bash "$ROOT/tools/build_engine.sh" --offline
  cd "$ROOT/firmware/v2"
  idf.py build
  cd build
  python -m esptool --chip esp32s3 merge_bin --fill-flash-size 16MB -o merged-binary.bin @flash_args
  ;;
 board-build|board-qa-build|board-flash|board-qa-flash)
  bash "$ROOT/tools/build_engine.sh" --offline
  cd "$ROOT/firmware/semantic"
  args=(build)
  if [[ "$1" == *-flash ]]; then [[ $# == 2 ]] || exit 2; args=(-p "$2" -b 460800 flash); fi
  if [[ "$1" == board-qa-* ]]; then
   idf.py -B build-qa -D SDKCONFIG=sdkconfig.qa -D 'SDKCONFIG_DEFAULTS=sdkconfig.defaults;sdkconfig.qa.defaults' "${args[@]}"
  else idf.py "${args[@]}"; fi
  ;;
 test)
  CARGO_HOME="${CARGO_HOME:-/tmp/token-cafe-cargo}" RUSTUP_HOME="${RUSTUP_HOME:-/tmp/token-cafe-rustup}" \
   "${CARGO_HOME:-/tmp/token-cafe-cargo}/bin/cargo" build --manifest-path "$ROOT/Cargo.toml" --offline --release --example codex_bridge_qa
  python "$ROOT/tools/test_connection_hint_qemu.py"
  python "$ROOT/tools/test_attention_qemu.py"
  python "$ROOT/tools/test_protocol_v2_qemu.py" "${@:2}"
  python "$ROOT/tools/test_transition_qemu.py"
  python "$ROOT/tools/test_codex_qemu.py"
  python "$ROOT/tools/test_desktop_resolution_qemu.py"
  exec python "$ROOT/tools/test_board_performance_qemu.py"
  ;;
 *) exit 2 ;;
esac
