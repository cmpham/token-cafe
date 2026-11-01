#!/usr/bin/env bash
# Pure protocol/route tests: no device access or desktop actions.
set -euo pipefail
cd "$(dirname "$0")/.."
out=$(mktemp -d "${TMPDIR:-/tmp}/token-cafe-keypad-tests.XXXXXX")
trap 'rm -rf "$out"' EXIT
cc -std=c11 -Wall -Wextra -Werror -fsanitize=address,undefined \
  firmware/keypad/test_display.c -o "$out/display-test"
"$out/display-test"
if [[ $(uname -s) == Darwin ]]; then
  swiftc -O native/keypad/Protocol.swift native/tests/keypad/main.swift -o "$out/keypad-test"
  "$out/keypad-test"
  swiftc -O native/desktop/Controls.swift native/tests/desktop/main.swift -o "$out/desktop-test"
  "$out/desktop-test"
fi
