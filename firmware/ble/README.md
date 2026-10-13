# Token Café BLE companion (ESP32-S3 N16R8)

This hardware target replaces the USB audio probe. No screen or solder changes
are needed. Power the unmodified board through COM; frame data travels through
Bluetooth Low Energy, not serial. The native USB-OTG port is unused.

## Build and provision

```sh
bash tools/firmware.sh ble-build
bash tools/firmware.sh ble-flash /dev/cu.YOUR_COM_PORT
```

The wrapper creates/reuses a private 32-byte key in `target/esp/ble-pairing-key`
and generates ignored `main/pairing_key.h`. Never commit either key file or share
the provisioned firmware image. Use a distinct key for each physical board.
Deleting the key requires reflashing the board with its replacement.

## Desktop app

```sh
# macOS: build bundle with the OS Bluetooth permission descriptions
sh tools/build_macos.sh
"target/release/Token Café.app/Contents/MacOS/token-cafe" \
  --bluetooth --bluetooth-key "$PWD/target/esp/ble-pairing-key"

# Linux: install BlueZ, libdbus-1-dev and pkg-config plus normal window dependencies
cargo run --release -- --bluetooth --bluetooth-key "$PWD/target/esp/ble-pairing-key"
```

After building, `bash tools/run_companion.sh` launches with the local key; add
`--always-on-top` to keep the window pinned where supported.

Allow the macOS Bluetooth permission if asked. No Bluetooth Settings pairing is
required: the app discovers its service and authenticates with the provisioned
key. Discovery refuses multiple matching boards. After connecting, it restricts
retries to that board's advertised name and checks its key again every time.
The title reports searching, syncing, mirrored, or retrying. Bluetooth is opt-in;
normal desktop launch is unchanged. `cargo build --no-default-features` omits BLE.

## Screenless acceptance test

```sh
cargo run --example firmware_frame -- target/cafe.rgb565
"target/release/Token Café.app/Contents/MacOS/token-cafe" \
  --bluetooth-test "$PWD/target/cafe.rgb565" \
  --bluetooth-key "$PWD/target/esp/ble-pairing-key"
```

This rejects a wrong key, checks a full synthetic café frame, a delta update,
malformed RLE rejection, disconnect/reconnect and a full resynchronization. Host
CRC32 is independent of firmware CRC32. The physical board logs committed frame
CRCs through COM, allowing separate evidence of execution. USB is only used for
flashing, power and optional diagnostics; the test does not transfer frames over
serial. LCD pixels, touch hardware and power-bank operation are not tested here.

## Transport and limits

- Same desktop 320x240 renderer, RGB565, centered at (80,40) on a 480x320 buffer.
- Compressed four-row rectangles (opcode 6), XOR pixel deltas (opcode 8),
  raw fallback (opcode 2), and only
  stripes changed since the last verified frame. Failed transfers force a full
  resync. The firmware validates complete RLE runs before changing staging memory.
- Separate staging and committed buffers in 8 MB PSRAM. PRESENT copies the frame
  to the committed buffer, ready for a future ILI9488 display driver.
- GATT service `7e57cafe-0001-4000-8000-000000000001`; RX/TX/auth suffixes 2/3/4.
  ATT writes use the negotiated MTU; notifications support fragmentation.
- Mutual HMAC-SHA256 challenge-response, with fresh host and board nonces and
  128-bit tags, verifies the destination before frame data is sent. RX rejects
  unauthenticated writers. This prototype does **not** encrypt frame traffic;
  authentication is not confidentiality. Use in a trusted local environment.
- Latest-frame slot, sampled at most twice per second; the radio worker never
  blocks desktop rendering. It discards stale queued frames instead of building
  a backlog. BLE cannot mirror 30 fps: initial frame and busy scenes can take
  seconds; small changes transfer faster. This is a companion snapshot display.
- No touch input or LCD driver yet. Patron focus/text are controlled in the app
  and reflected in the outgoing frame. No separate companion UI pattern.

## Local validation (2026-10-13)

- Physical N16R8 board: wrong-key rejection; full synthetic frame CRC; delta CRC;
  malformed RLE rejection; disconnect/reconnect and full resync passed.
- Live desktop frames: full initial sync about 18 seconds, subsequent animated
  scene updates about 1.6–2.3 seconds after XOR compression (observed, not a guarantee).
- macOS: 33 library + 8 app tests; Clippy; firmware protocol ASan/UBSan passed.
- Linux Rust 1.88 container: 30 library + 8 app tests passed with BlueZ build
  dependencies. A physical Linux Bluetooth adapter has not been tested.
