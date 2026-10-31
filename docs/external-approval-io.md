# External approval LED and button

For the bill of materials, editable electrical schematic, assembly checks
and rebuild instructions, see the
[recreation guide](hardware/recreate-approval-controller.md).

Target: JC4827W543, semantic v2 firmware. P4 UART1 is used as GPIO rather
than UART. GPIO17 drives a small LED through a 330 ohm series resistor to GND.
GPIO18 connects to a normally open momentary button to GND, with the internal
pull-up enabled. P4 3V3 stays disconnected. Never identify cable pins by colour.
Check switch terminal pairs and disconnect USB/battery before changing wiring.

## Production behaviour

The LED blinks at 1 Hz (500 ms on, 500 ms off) whenever a session needs
attention: approvals, questions, and desktop-only waiting items. A completed
physical click or on-screen selection acknowledges the current alert and stops
the LED, even while an item remains unresolved. A newly observed waiting request
rearms it; a refresh of the same item does not. Submitted board decisions,
disconnected and unsynchronized states do not light it.

For questions and desktop-only requests, briefly press and release the button
to open the associated desktop app and complete the action there. This works
from the cafe or the item's review screen. If no desktop link is available,
the button opens details on the board and acknowledges the alert.

The happy path requires no touchscreen:

1. Briefly press and release to open the waiting approval. Prefer the request
   shown in the attention area; otherwise open the next pending approve-once request.
2. Read the review and wait for approval controls (at least 500 ms after presentation).
3. Briefly press and release again to approve that exact request once.
4. After completion, another click returns to the cafe; the next click opens the
   next waiting request. A review opened from the desktop requires one click to
   enable its controls before a separate approval click.

Deny continues to use the screen. The physical button routes multiple-choice
requests to the desktop rather than choosing an answer. Each click captures
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

## Desktop reconnect recovery — 2026-10-27

After physical button acceptance, the user reported the disconnected-board
instruction while the companion was still running. Desktop logs showed repeated
device disconnects followed by setup timeouts. Restarting the companion restored
an applied-sync report. The initial radio drop cause remains unconfirmed; serial
monitoring did not capture a usable disconnect reason.

Fixed a confirmed reconnect cleanup gap: the old outer connection timeout could
cancel setup without explicitly disconnecting the peripheral. Discovery and
setup now have separate deadlines, with bounded disconnect cleanup on setup
failure or timeout. Retry status resets to connecting, then synchronizing before
the board acknowledges its model; errors include timestamps.

All 172 workspace tests, Clippy and the macOS release build passed. Regression
tests cover stalled setup cleanup, failed authentication cleanup and successful
setup retaining its link. This desktop-only change does not alter board firmware
or approval policy. Physical long-duration stability still requires observation.

## General attention and desktop handoff — 2026-10-30

The LED now covers unlinked waiting questions and permissions, desktop-only
requests, and multiple-choice requests. A short physical click opens the linked
desktop app for items the button cannot approve. It works from both the cafe
and an unlinked review. Opening the app keeps the attention LED blinking until
the waiting item resolves. Approve-once requests retain their separate review
and approval clicks. A waiting item resolved during a press cancels that click.

All 175 workspace tests and Clippy passed. The production firmware and the
complete QEMU gate passed, including protocol capacities, reconnect, navigation,
provider results, and large-text rendering. Physical LED and button handoff
confirmation is still needed after deployment.

Before deployment, a fresh private 4 MiB flash/NVS backup was saved under
`target/esp/backups/attention-desktop-2026-10-30/`. Production firmware was flashed
to `/dev/cu.usbmodem101` with esptool hash verification; QA injection and wiring
diagnostic modes are disabled. The existing companion reconnected and reported
`TokenCafe-6336 synced v2`. The actual question's LED and physical desktop
handoff remain for the user to confirm.

## Attention acknowledgement — 2026-10-30

A completed physical click or on-screen selection now silences the external
LED for the current alert batch, including when opening the desktop app. This
acknowledges attention only: waiting items remain unresolved and no decision is
implied. Refreshing the same item does not rearm the LED. A new waiting identity
(including a new request in the same session) rearms it. Cancelled or long
presses do not acknowledge the alert.

All 178 workspace tests, Clippy, the production build, and the full QEMU gate
passed. Regression tests cover screen and physical acknowledgement, refreshes,
new requests, multiple waiting items, and cancelled inputs.

The acknowledgement firmware was installed on `/dev/cu.usbmodem101` with
esptool hash verification, retaining the pairing and NVS partitions. The private
backup from the preceding attention deployment remains available.
