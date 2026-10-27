# External approval LED and button

Target: JC4827W543, semantic v2 firmware. P4 UART1 is used as GPIO rather
than UART. GPIO17 drives a small LED through a 330 ohm series resistor to GND.
GPIO18 connects to a normally open momentary button to GND, with the internal
pull-up enabled. P4 3V3 stays disconnected. Never identify cable pins by colour.
Check switch terminal pairs and disconnect USB/battery before changing wiring.

## Production behaviour

The LED blinks at 1 Hz (500 ms on, 500 ms off) while an approve-once-capable
pending request exists. Submitted, desktop-only, disconnected and unsynchronized
requests do not light it.

The happy path requires no touchscreen:

1. Briefly press and release to open the waiting approval. Prefer the request
   shown in the attention area; otherwise open the next pending approve-once request.
2. Read the review and wait for approval controls (at least 500 ms after presentation).
3. Briefly press and release again to approve that exact request once.
4. After completion, another click returns to the cafe; the next click opens the
   next waiting request. A review opened from the desktop requires one click to
   enable its controls before a separate approval click.

Deny and multiple-choice answers continue to use the screen. Each click captures
one step on press and performs it on release; holding cannot skip steps. Presses
while asleep only wake the display. Loading or unarmed presses cannot approve,
even if controls become ready before release. Long holds over 800 ms are ignored.
Input uses 30 ms debounce and requires a stable release after boot. Request
replacement, resolution, resynchronization, disconnect, or simultaneous touch
invalidates an approval press. Durable receipts and authenticated transport remain
in use; unavailable text or delivery errors require desktop attention.

## Wiring diagnostic (no Bluetooth or approval dispatch)

Use the existing provisioned production pairing header; this mode does not
connect to the desktop or read/answer requests. Preserve a private full-flash
backup before replacing firmware. Follow the normal simulation gate in
`protocol/operations.md` before physical deployment.

```sh
bash tools/qemu_v2.sh board-io-build
bash tools/qemu_v2.sh board-io-flash /dev/cu.YOUR_PORT
```

The LCD must say `LED / BUTTON WIRING TEST` and `APPROVALS DISABLED`.
The external LED is off while released and on while held. The LCD changes
between `BUTTON: RELEASED` and `BUTTON: PRESSED`. Serial at 115200 baud logs
`EXT_IO: BUTTON PRESSED` and `EXT_IO: BUTTON RELEASED` after debouncing.
A switch held at startup is visible in the diagnostic, but cannot generate a
production approval. If the display says PRESSED while untouched, check the
button's permanent terminal pairs; do not fix wiring with power connected.

After the user confirms LED and button operation, install normal firmware:

```sh
bash tools/qemu_v2.sh board-build
bash tools/qemu_v2.sh board-flash /dev/cu.YOUR_PORT
bash tools/run_companion.sh
```

Diagnostic and production builds have separate build directories and sdkconfig
files. Do not deploy the diagnostic image for everyday use. Do not use the
legacy framebuffer `display-flash` target on a semantic companion.

## Verification

Native tests exercise physical press/release, duplicate/held input, missing
presentation, concurrent touchscreen input, sleep/wake, withdrawal, expiry,
desktop resolution, disconnect, receipt persistence failure and unrelated model
updates. A portable ASan/UBSan C test exercises switch bounce and startup release:

```sh
cc -std=c11 -Wall -Wextra -Werror -fsanitize=address,undefined \
  -Ifirmware/semantic/main firmware/test_external_io.c -o /tmp/test-external-io
/tmp/test-external-io
```

Automated checks do not establish LED polarity, resistor value, physical switch
terminal pairing or a human press reaching a real provider. Verify diagnostic
behaviour physically before production, then use a deliberately harmless live
request for acceptance.


## Local verification — 2026-10-27

Implemented on `codex/approval-led-button`, based on local `master` at `ee952b7`.
The original mixed checkout was preserved. Validation completed:

- 166 workspace tests passed, including six physical-button regression tests.
- Workspace/all-target Clippy passed with warnings denied.
- Portable switch debounce test passed with AddressSanitizer and UBSan.
- The complete `tools/qemu_v2.sh build` / `test` gate passed, including protocol
  capacities 182/244/509, attention, transitions, provider resolution, and large
  text performance. QEMU does not verify the physical GPIO circuit.
- Diagnostic and production ESP-IDF builds passed; the matching macOS app built.
- The connected board's full 4 MiB flash was backed up privately before flashing.
  Diagnostic flashing completed with esptool hash verification. Serial boot
  confirmed wiring-test mode with approvals disabled.

Local logs are under ignored `target/external-io-qa/`. The physical switch
produced six debounced press/release pairs in the diagnostic serial log. The LED
initially stayed dark because it was reversed. The user reversed it and confirmed
that it works in the button-controlled diagnostic. No GPIO17 voltage measurement
was needed.

Production firmware was then built and flashed on 2026-10-27 using
`tools/qemu_v2.sh board-flash /dev/cu.usbmodem1101`, with esptool hash verification.
Both `CONFIG_TC_EXTERNAL_IO_DIAGNOSTIC` and `CONFIG_TC_QA_USB` are disabled in the
production config. Serial startup confirmed GPIO17/GPIO18 initialization,
NV3041A initialization and production BLE advertising. The existing compatible
Pokémon desktop companion reconnected and its window reported
`BLE: TokenCafe-6336 synced v2`; its display settings and other desktop windows
were preserved. A real provider approval using the physical button has not yet
been exercised; no real pending request was dispatched by automated testing.

## Button-only flow update — 2026-10-27

The LED now blinks at 1 Hz. Short clicks open the waiting review, acknowledge a
desktop-opened review, approve once after presentation, and leave a completed
review. Each click performs only its captured step.

Validation: 170 workspace tests passed, Clippy passed with warnings denied,
portable LED/debounce tests passed under ASan/UBSan, and the complete QEMU gate
passed. Production firmware was flashed to `/dev/cu.usbmodem1101` with hash
verification and the existing companion reconnected as `synced v2`. This verifies
installation and synchronization; the updated physical flow awaits user testing.
