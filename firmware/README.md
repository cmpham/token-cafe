# Token Café ESP32-S3 prototype

The default build is **QEMU-only** and must not be flashed to a physical board.
A separate **screenless hardware build** supports ESP32-S3 N16R8 bring-up (below).
It runs ESP-IDF on an emulated ESP32-S3 with 16 MB flash and 8 MB octal PSRAM.
The host renders the existing café scene; firmware receives RGB565 rectangles into
PSRAM and presents them on a 480×320 virtual panel. The existing 320×240 scene is
centered at (80, 40), preserving its pixels without stretching.

## Install on macOS

From the repository root, with Homebrew and Python 3.12 available:

```sh
HOMEBREW_NO_INSTALL_CLEANUP=1 brew install cmake ninja libgcrypt glib pixman sdl2 libslirp python@3.12
mkdir -p target/esp
git clone --branch v5.4.3 --depth 1 --shallow-submodules --recursive \
  https://github.com/espressif/esp-idf.git target/esp/esp-idf
export IDF_TOOLS_PATH="$PWD/target/esp/tools"
python3.12 target/esp/esp-idf/tools/idf_tools.py install --targets esp32s3
python3.12 target/esp/esp-idf/tools/idf_tools.py install qemu-xtensa
python3.12 target/esp/esp-idf/tools/idf_tools.py install-python-env
```

Use **Espressif's QEMU fork**, not Homebrew's generic `qemu` package. The pinned
ESP-IDF v5.4.3 tools manifest installs QEMU 9.2.2 (`esp_develop_9.2.2_20250817`).
Dependencies, SDK, and compiler live under ignored `target/esp`; no shell startup
files are modified. The panel dependency is pinned in `dependencies.lock`.

Linux uses the same firmware and Python host client; install the distribution's
QEMU runtime/build dependencies first using [Espressif's guide](https://docs.espressif.com/projects/esp-idf/en/v5.4.1/esp32s3/api-guides/tools/qemu.html).
Linux execution is not yet locally verified. `IDF_PATH`, `IDF_TOOLS_PATH`, and
`IDF_PYTHON_ENV_PATH` can override the default local installation paths.

## Build, test, and show the café

```sh
cargo run --example firmware_frame -- target/cafe.rgb565
bash tools/firmware.sh build
bash tools/firmware.sh test
bash tools/firmware.sh run
```

`run` opens an SDL window with a static demo rendered by the real scene code.
Close the window or press Ctrl-C to stop. The normal desktop app is unaffected.
This initial prototype does not stream live sessions or handle mouse clicks in
the QEMU window. To preview a selected patron, export another frame with scene
coordinates, then run with that file:

```sh
cargo run --example firmware_frame -- target/cafe-selected.rgb565 120 210
bash tools/firmware.sh run --frame "$PWD/target/cafe-selected.rgb565"
```

`test` boots a fresh emulator, checks the PSRAM configuration (firmware asserts
8 MB), uploads the frame, and compares host and firmware CRCs. It exercises bad
CRC/version/length/opcode, out-of-bounds and empty rectangles, fragmented input,
noise, a disconnect mid-packet, simulated tap mapping, and repeated updates.
Finally it compares every displayed pixel in a QEMU screenshot to the uploaded
RGB565 frame. Artifacts: `target/esp/cafe-qemu.ppm` and `target/esp/qemu.log`.
Tests use made-up sessions; no local session information is sent anywhere.

The optional native parser stress test requires Clang and runs without QEMU:

```sh
mkdir -p target/esp
clang -std=c11 -Wall -Wextra -Werror -fsanitize=address,undefined -g \
  firmware/test_protocol.c firmware/main/protocol.c -o target/esp/test-protocol
target/esp/test-protocol
```

It checks the standard CRC32 test vector and all 65,536 payload-length values
with randomized trailing bytes under AddressSanitizer and UndefinedBehaviorSanitizer.
QEMU tests use SDL's dummy backend: `-display none` does not service the virtual
panel refresh and hangs the panel driver. PSRAM pixels are copied into QEMU's
dedicated display buffer because its virtual panel cannot read PSRAM addresses
directly. UART boot diagnostics are saved to `target/esp/boot.log`.
Transfers are intentionally uncompressed over an emulated 115200-baud UART;
allow a few minutes for the full test, and wait for the initial frame in `run`.
This does not measure the eventual native USB or physical screen frame rate.

## Transport and protocol v1

QEMU exposes UART0 through a private Unix socket in a temporary directory. No
TCP listener is opened. USB CDC is a future transport for the same parser.
One request at a time; wait for a reply before sending another. Sequence numbers
correlate replies, not deduplication. A partial packet is discarded after one
second without input. After reconnect, wait for that timeout, send HELLO, CLEAR,
upload a full frame, then PRESENT to replace any partially staged updates.

All integers are little endian. Header (16 bytes): magic `TCF1` (4), opcode (1),
version `1` (1), payload length (u16), sequence (u32), CRC32 (u32). CRC uses the
IEEE/zlib algorithm over header bytes 0–11 followed by the payload. Maximum
payload: 4096 bytes. Replies use `opcode | 0x80`, echo sequence, and prefix their
payload with status: 0 success, 1 invalid argument/header, 2 bad CRC, 3 unknown
opcode. Invalid requests must not mutate the framebuffer.

| Opcode | Request | Successful reply after status byte |
|---|---|---|
| 1 HELLO | Empty | Width u16, height u16, max payload u16, pixel format u8 (`1` = RGB565 LE) |
| 2 RECT | x/y/width/height as u16, then packed RGB565 LE pixels | Empty |
| 3 PRESENT | Empty | CRC32 of the entire staged framebuffer; also displays it |
| 4 INJECT_TAP | Panel x/y as u16 | Scene x/y as u16; rejects letterbox coordinates |
| 5 CLEAR | Empty | Empty; zeros staging buffer, requires PRESENT to display |

`INJECT_TAP` is a simulator test hook, not a real touch driver or unsolicited
input event. Display and transport callbacks separate the protocol from the
QEMU-specific code. RGB565 is our **host protocol** format; the ILI9488 SPI driver
will need conversion to its supported wire pixel format.

## Still requires hardware work

- Native USB CDC transport, Mac/Linux port discovery and reconnect behavior.
- ILI9488 initialization, SPI transfer/conversion, backlight/power verification.
- Confirmed touch controller driver, calibration, and asynchronous touch events.
- Continuous live scene streaming, changed-region updates, and desktop navigation.
- Physical performance, cable/power behavior, and long-running device tests.

Simulation validates the firmware logic and virtual framebuffer, not those
hardware-dependent details.

## Physical ESP32-S3 N16R8: first connection

1. Leave the screen and all jumper wires disconnected. Connect a USB data cable
   from the Mac to the board's **COM** port. Allow the accessory if macOS asks.
2. Discover the port with the installed environment's
   `python -m serial.tools.list_ports -v`. Use the actual `/dev/cu.*` port below;
   Linux generally uses `/dev/ttyACM*` or `/dev/ttyUSB*` instead.
3. Read identity before flashing; confirm ESP32-S3, 16 MB flash, 8 MB PSRAM.
4. Build, flash, and test:

```sh
bash tools/firmware.sh hardware-info PORT
bash tools/firmware.sh hardware-build
bash tools/firmware.sh hardware-flash PORT
bash tools/firmware.sh hardware-test PORT
```

The hardware image uses UART0 at 115200 baud through COM, allocates the frame in
8 MB octal PSRAM, and implements the same CRC-protected frame protocol without
accessing the QEMU display or any LCD/touch GPIOs. Flashing replaces the factory
application. Flash-tool hash verification checks the uploaded image; the host
hardware test separately checks execution, full-frame PSRAM transfer, invalid
messages, and timeout recovery. It does not test an attached display or touch.

If automatic programming cannot connect: hold BOOT, press and release RESET,
release BOOT, then retry flashing. Press RESET after flashing if necessary.
Close other serial monitors before testing. Use a data cable, not a charge-only
cable. The first hardware stage does not stream live sessions or drive ILI9488;
those drivers still need implementation and physical screen pin verification.

## Bluetooth companion

For wireless app mirroring on physical hardware, use the separate
[BLE target](ble/README.md). It authenticates the board and accepts compressed
updates into PSRAM; it does not require the LCD or native USB-OTG power jumper.
