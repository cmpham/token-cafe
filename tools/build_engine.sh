#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
export CARGO_HOME="${CARGO_HOME:-/tmp/token-cafe-cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-/tmp/token-cafe-rustup}"
export PATH="$CARGO_HOME/bin:$PATH"
source "$ROOT/target/esp/export-rust.sh"
export CC_xtensa_esp32s3_none_elf=xtensa-esp32s3-elf-gcc
export AR_xtensa_esp32s3_none_elf=xtensa-esp32s3-elf-ar
export CFLAGS_xtensa_esp32s3_none_elf=-mlongcalls
cargo +cafe-esp build --manifest-path "$ROOT/firmware/engine/Cargo.toml" --target xtensa-esp32s3-none-elf -Zbuild-std=core,alloc --release "$@"
