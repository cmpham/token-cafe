# External approval LED and button

Target: JC4827W543, semantic v2 firmware. P4 UART1 is used as GPIO rather
than UART. GPIO17 drives a small LED through a 330 ohm series resistor to GND.
GPIO18 connects to a normally open momentary button to GND, with the internal
pull-up enabled. P4 3V3 stays disconnected. Never identify cable pins by colour.
Check switch terminal pairs and disconnect USB/battery before changing wiring.

## Production behaviour

The LED is on while an approve-once-capable pending request exists. Requests
already submitted by this board, desktop-only requests, and disconnected or
unsynchronized state do not light it. A lit LED is an attention signal; it does
not imply the currently selected screen can approve.

Open the intended request on the board's touchscreen and wait until its approval
controls appear. A short press and release of the external button approves that
exact review once. The button never selects an arbitrary waiting request or a
choice answer. Deny and multiple-choice answers continue to use the screen.
Presses during loading, before the displayed frame is armed, during another
screen gesture, or while asleep cannot approve. Waking requires another fresh
press. Long holds over 800 ms are ignored. Input uses 30 ms debounce, requires a
stable release after boot, and never repeats while held. Request replacement,
resolution, resynchronization, or disconnect invalidates the captured press.
The existing durable receipt and authenticated decision transport are reused.

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
The external LED blinks while released and stays on while held. The LCD changes
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
produced six debounced press/release pairs in the diagnostic serial log. The
user reports that the external LED does not light. LED polarity, breadboard
continuity and GPIO17 wiring still need physical diagnosis. Production deployment
and live approval acceptance remain pending; the board is left in the diagnostic
image with approvals disabled. No GPIO17 voltage measurement has been made.
