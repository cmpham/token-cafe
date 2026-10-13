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
serial. That diagnostic command does not verify physical LCD pixels or touch alignment.

## Transport and limits

- Same desktop 320x240 renderer, RGB565, nearest-neighbor scaled to fill the 480x272 panel.
- Adaptive compressed rectangles (opcode 6), XOR pixel deltas (opcode 8),
  and raw fallback (opcode 2). Changed four-row regions merge up to the 4096-byte
  payload limit, reducing round trips for sparse updates. Failed transfers force a full
  resync. The firmware validates complete RLE runs before changing staging memory.
- Fast-path capability (opcode 12) negotiates LZ4 rectangles (10), compressed XOR
  rectangles (13), and abort/restore (11). The compressed replies piggyback one
  queued input event; raw/RLE equivalents (16/14/15) do the same, avoiding
  separate input polls during frame transfers. The decoder validates the full block before modifying
  staging pixels; XOR uses a dedicated PSRAM scratch buffer. Older firmware uses
  the existing RLE transport.
- New touch generations preempt background transfers between packets. Abort
  restores the last displayed framebuffer, preserving the verified delta baseline;
  partially received frames are never presented. Touch application publishes a new
  frame immediately instead of waiting for the normal 100 ms publication timer.
- Separate staging and committed buffers in 8 MB PSRAM. PRESENT copies the frame
  to the committed buffer and sends it to the NV3041A on the JC4827 profile.
  The LCD uses 32-row DMA batches instead of four-row batches, and ESP32 builds
  use the ROM CRC routine. The original headless profile keeps the committed buffer in memory.
- GATT service `7e57cafe-0001-4000-8000-000000000001`; RX/TX/auth suffixes 2/3/4.
  ATT writes use the negotiated MTU; notifications support fragmentation.
  When supported, write-without-response fragments use a write-response barrier
  every eight fragments; the checked protocol reply is the final packet barrier. Every packet still receives a
  sequence/CRC-checked protocol reply, and PRESENT verifies the whole frame.
- The board requests a 15–30 ms connection interval (the central decides).
  Desktop frame publication is capped at 10 Hz with a single latest-frame slot;
  input polls are spaced at least 75 ms apart, between complete packet transfers.
- Mutual HMAC-SHA256 challenge-response, with fresh host and board nonces and
  128-bit tags, verifies the destination before frame data is sent. RX rejects
  unauthenticated writers. This prototype does **not** encrypt frame traffic;
  authentication is not confidentiality. Use in a trusted local environment.
- Latest-frame slot, sampled at most ten times per second (immediately on touch); the radio worker never
  blocks desktop rendering. It discards stale queued frames instead of building
  a backlog. BLE cannot mirror 30 fps: initial frame and busy scenes can take
  seconds; small changes transfer faster. This is a companion snapshot display.
- The JC4827 profile returns touch input to the desktop and displays the shared
  scene; the headless profile returns no input.

## Interruptible protocol validation (2026-10-13)

Physical JC4827W543 with the live Mac app, eight final taps: all eight produced
separately acknowledged display responses, with no reconnect or CRC errors.
Release-to-LCD completion: median **1155 ms**, range **700–1370 ms**. The earlier
protocol measured median 2900 ms, range 2240–3320 ms across 13 displayed responses.
These are observed trials, not a guaranteed latency bound or identical tap replay.
Touch-down-to-LCD: median 1555 ms, range 1020–1730 ms; taps activate on release.

Synthetic hardware selection/page transfers: 270–540 ms; full sync 2.70 s.
These transfer-only timings exclude input delivery and scheduling.
Checks: 43 library + 8 app tests, 51 Rust-to-C full/delta fixtures under ASan/UBSan,
Clippy, BLE-disabled tests, both display/headless firmware builds, and physical
wrong-key rejection, frame CRC, malformed packet rejection, cancellation rollback,
and disconnect/reconnect resynchronization.

## Earlier baseline validation (2026-10-13, before the interruptible protocol)

- Physical N16R8 board: wrong-key rejection; full synthetic frame CRC; delta CRC;
  malformed RLE rejection; disconnect/reconnect and full resync passed.
- Live desktop frames: full initial sync about 18 seconds, subsequent animated
  scene updates about 1.6–2.3 seconds after XOR compression (observed, not a guarantee).
- macOS: 33 library + 8 app tests; Clippy; firmware protocol ASan/UBSan passed.
- Linux Rust 1.88 container: 30 library + 8 app tests passed with BlueZ build
  dependencies. A physical Linux Bluetooth adapter has not been tested.

The desktop and firmware must both use the 480x272 protocol dimensions. Rebuild
and flash the companion when updating from the previous 480x320 version; the
desktop rejects a mismatched handshake. This changes framebuffer geometry only,
not the board-specific LCD driver or flash/PSRAM configuration.

For repeatable visual QA, run `cargo run --example companion_qa` from the repo
root. It writes 24 named 480x272 PNG/RGB565 scenarios and four contact sheets to
`target/companion-qa/`, covering selection, paging, waiting alerts, missing tasks,
Unicode fallback, overflow patrons, idle/closed sessions, and long names.
Equivalent navigation paths are checked for identical rendered pixels. A chosen
RGB565 scenario can be passed to `tools/firmware.sh test --frame ABSOLUTE_PATH`.
This exercises the renderer and simulator; it does not replace native window
interaction or physical display/touch/Bluetooth testing.

## JC4827W543 display board (verified N4R8)

The tested board has 4 MB flash and 8 MB octal PSRAM, despite the seller's N8R4
listing. Use its separate build profile; do not flash the headless N16R8 image.

```sh
bash tools/firmware.sh display-build
bash tools/firmware.sh display-flash /dev/cu.usbmodem2101
sh tools/build_macos.sh
bash tools/run_companion.sh --always-on-top
```

The serial port can change after reconnecting. The USB connection supplies power
and flashing/logging; mirroring and input use authenticated BLE. The desktop app
must stay running. The firmware shows a colored boot bar until a frame arrives.
First-frame transfer can take about 20 seconds; subsequent updates depend on the
changed area and radio link (roughly 2–5 seconds in local tests).

LCD: NV3041A QSPI at 20 MHz, CS45, clock47, D0/D1/D2/D3=21/48/40/39,
backlight1, with display inversion disabled (this panel otherwise shows negative colors). The initialization table and QSPI framing follow Arduino_GFX, with
its BSD license retained in `licenses/Arduino_GFX.txt`. Touch uses the pinned
Espressif GT911 component on SDA8/SCL4. The driver probes 0x5d then 0x14;
reset/interrupt pins are not driven because the supplied pin sheet does not
establish a capacitive reset connection. Touch is polled every 20 ms.

Single-finger taps select/toggle patrons or cycle the banner. Starting on the
right rail drags the task scrollbar. Multi-touch and non-rail drags are ignored. Opcode 9 polls a queued event: empty response, or five bytes
`kind:u8, panel_x:u16le, panel_y:u16le` (1=tap, 2=scroll). The desktop checks bounds,
maps panel coordinates back to the 320x240 scene, and applies input on its UI thread. Input queues are
bounded and cleared on authentication/reconnect. No desktop app-launch commands
are exposed over this touch path.

Factory flash was saved locally under `target/esp/backups/` before replacement
and verified using esptool's flash digest. Keep that ignored backup outside Git.
Hardware testing confirmed BLE authentication, completed LCD transfers, repeated
matching host/board frame CRCs and physical GT911 tap reports. Screen orientation,
color accuracy, and touch alignment additionally require visual confirmation.

### Measuring physical touch latency

Capture serial output at 115200 baud and desktop stderr during the same connection.
The firmware logs `TOUCH: DOWN`, `TOUCH: TAP` (on release), and the CRC of each
completed LCD frame. Desktop logs tag verified frames with the latest applied
input generation. Tap distinct patrons with at least four seconds between taps,
then run:

```sh
python3 tools/measure_touch_latency.py /tmp/tc-touch-board.log /tmp/tc-touch-app.log
```

The report correlates each tap with its first acknowledged response frame using
CRC and input generation. Both endpoint timestamps come from the board clock;
USB log delivery and host clock skew do not enter the duration. It separates
finger hold time from release-to-LCD latency. Incomplete or superseded responses
are excluded. This measures completion of the LCD write, not the panel's optical
response; touch detection itself is polled every 20 ms. Use high-speed video for
physical finger-contact-to-photon timing.

Codec interoperability checks (Rust encoder → actual C firmware decoder):

```sh
TC_CODEC_FIXTURES=target/codec-fixtures cargo test fast_protocol_cross_language_fixtures
clang -std=c11 -Wall -Wextra -Werror -fsanitize=address,undefined -g \
  firmware/test_protocol.c firmware/main/protocol.c -o target/esp/test-protocol
for packet in target/codec-fixtures/*.packets; do
  target/esp/test-protocol "$packet" "${packet%.packets}.rgb565" || exit 1
done
```

Set `TC_BLE_BENCH_DIR=target/companion-qa` when running `--bluetooth-test`
to time representative task-page and selection transitions on hardware. These
transfer benchmarks do not replace the physical touch-to-LCD measurement above.

## Automated real-board gesture and capture suite

The normal firmware **does not expose** USB injection/capture. A separate profile
sets `CONFIG_TC_QA_USB=y`; it still uses the same production gesture recognizer,
BLE transport, renderer, and display driver. Read [the QA plan](../../docs/qa/board-test-plan.md)
for scope, criteria, and optical/sensor limitations.

```sh
bash tools/firmware.sh qa-flash /dev/cu.usbmodem2101
cargo build --release --example board_e2e
target/esp/tools/python_env/idf5.4_py3.12_env/bin/python tools/board_e2e.py \
  --port /dev/cu.usbmodem2101 --out target/board-e2e-final
# Restore the normal firmware and application after the test releases serial/BLE:
bash tools/firmware.sh display-flash /dev/cu.usbmodem2101
bash tools/run_companion.sh --always-on-top
```

Stop this checkout's regular app before running the deterministic test host.
Results, raw RGB565, PNGs and both logs are saved under the output directory.
Failures, a missed latency target, or transport errors return a nonzero exit code.
Reference actions drive a separate Scene; only firmware-generated BLE inputs drive
the actual Scene. Framebuffer snapshots are taken from committed board memory
under a mutex, include frame/time/CRC metadata, and are checked byte-for-byte.

Diagnostic serial commands (ASCII, newline terminated):

- `QA MODE <id> 1` / `0`: enable/disable synthetic samples; clear prior gestures.
- `QA IN <id> <count:0..2> <x:0..65535> <y:0..65535>`: queue a controller sample;
  count zero releases, one presses/moves, two tests multitouch cancellation.
  Out-of-panel coordinates are deliberately permitted for negative testing.
- `QA SNAP <id>`: coherent RGB565 snapshot with `@QA SNAP`, ordered 128-byte
  hexadecimal `@QA DATA` lines, and `@QA END`. Validate length/offset/CRC.

`@QA INPUT` acknowledges processing on the touch task, not just USB receipt.
A held/queued touch is tagged with the input connection generation and discarded
across disconnect, authentication or reset, preventing stale selections.
Captures inspect intended LCD pixels; they are not photographs of the panel.

## Companion double-tap navigation

The pinned Espressif `esp_lcd_touch_gt911` 1.1.3 driver exposes contact coordinates,
not double-tap events. Its [official implementation](https://github.com/espressif/esp-bsp/blob/master/components/lcd_touch/esp_lcd_touch_gt911/esp_lcd_touch_gt911.c)
was checked along with the installed source. The board therefore recognizes
pairs using its own monotonic millisecond clock, before BLE transmission.

Two releases within 450 ms, each held no longer than 350 ms, and within 8 panel
pixels per axis form a double tap. The first tap is sent immediately. The second
sends input kind **3** (`kind, x_lo, x_hi, y_lo, y_hi`) using the first tap's
coordinates, so movement across a hit-box boundary cannot open a different
patron. Scrolls, cancelled drags/multi-touch, diagnostic mode changes and
connection/authentication epochs break the pair. A third tap starts a new pair.
Update the desktop app and board together; older desktop decoders do not support
input kind 3. New desktops continue accepting older firmware's tap/scroll events.

The Mac focuses the patron and opens its validated `codex://threads/...` or
`claude://code/continue?session=local_...` URL using LaunchServices. Both mouse
and companion activation use the same resolver and launcher. No commands or URLs
arrive from the board, only bounded coordinates. Missing desktop mappings stay
selected without launching anything. Linux keeps selection without invoking the
macOS launcher. CLI-only Claude sessions are not automatically mapped to terminal
windows.

The QA host records resolved launch URLs instead of opening synthetic sessions.
Run `tools/board_e2e.py` with QA firmware for both provider routes, unlinked
patrons, slow taps, different patrons, footer double taps and cancelled pairs,
in addition to the existing screen/gesture/reconnect suite. The portable detector
can also be checked with:

```sh
clang -Wall -Wextra -Werror -fsanitize=address,undefined \
  firmware/test_double_tap.c -o /tmp/tc-test-double-tap
/tmp/tc-test-double-tap
```

### Verify live native-app touch areas

With diagnostic firmware loaded, launch the native app with `TC_TOUCH_QA=1` and
redirect its output to a local log. This opt-in trace records the current seat
IDs, validated desktop URLs and resolved hit for every board input. It is off in
normal launches. Run:

```sh
TC_TOUCH_QA=1 bash tools/run_companion.sh > /tmp/cafe-touch.log 2>&1
# In a second terminal, using the firmware Python environment:
python tools/board_hit_areas.py --port /dev/cu.YOUR_PORT \
  --log /tmp/cafe-touch.log --out target/board-hit-areas
```

This test opens the real mapped desktop sessions. It tests five points per seat,
one double tap per occupied seat, and ten header/footer points. It stops if the
live seat map changes. Keep its logs/captures local: they contain real session
identities and task text. Afterwards restore production firmware and relaunch
without `TC_TOUCH_QA`. Injected samples do not verify physical sensor calibration.
