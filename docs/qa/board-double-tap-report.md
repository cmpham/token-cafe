# Board double-tap navigation verification — 2026-10-13

Implemented and tested locally on the JC4827W543 ESP32-S3, GT911 touch controller,
NV3041A 480×272 panel, and macOS desktop app. Changes are uncommitted on `cmpham`.

## Driver research and behavior

The installed Espressif GT911 1.1.3 implementation and the
[official driver source](https://github.com/espressif/esp-bsp/blob/master/components/lcd_touch/esp_lcd_touch_gt911/esp_lcd_touch_gt911.c)
provide contact coordinates/state, with no double-tap callback. Application-level
gesture recognition was added to the board using its own clock, avoiding BLE
arrival-time distortion.

Two short taps at the same position (within 8 panel pixels per axis), released
within 450 ms, emit one immediate tap followed by one double-tap event. Each
contact must last at most 350 ms. Double taps focus the patron and resolve its
existing desktop session URL. The same resolver and macOS launcher serve mouse
double-clicks. Slower taps still select/deselect. Missing targets stay selected;
CLI-only terminal sessions are not mapped. Linux does not run the Mac launcher.

## Executed verification

| Check | Result |
| --- | --- |
| Full board-injected gesture/framebuffer suite | **49/49 PASS**, no transport errors |
| Claude and Codex fixture double taps | Exactly one expected provider URL each; correct focus |
| Missing target, slow taps, different patrons | No navigation |
| Footer double taps | Normal patron cycling preserved |
| Multi-touch between taps | Cancels the pair; no navigation |
| Existing selection, scrolling, bounds, reconnect, held touch and idle cases | PASS |
| Portable C detector under ASan/UBSan | PASS: timing/radius boundaries, anchor, triple tap, holds, epochs, cancellation, clock rollover |
| Rust default tests | 44 library + 8 app tests PASS; all-target run PASS |
| Rust BLE-disabled tests | 39 library + 8 app tests PASS |
| Clippy all targets, formatting, diff whitespace | PASS |
| Python capture-barrier regression tests | 2/2 PASS |
| Diagnostic, production LCD and headless firmware builds | PASS |
| macOS bundle build | PASS |

The existing single-tap latency samples measured release to completed LCD frame:
**680 ms median, 790 ms p95/max, n=11**. These are deterministic diagnostic samples,
not measurements of destination-app opening latency.

The deterministic host runs the production BLE transport, input decoder, action
resolver and scene renderer. It records URLs instead of launching synthetic
sessions. Framebuffer captures are compared byte-for-byte against the host and
reference action model. They do not verify optical output or physical sensing.

## Live desktop check

After the fixture suite, the rebuilt native Mac app was connected to the board.
Firmware-injected double taps reached the app as `Tap` then `DoubleTap` and
successfully dispatched real Codex and Claude Desktop session URLs through
`/usr/bin/open`. Claude's accessibility tree showed the exact matching session ID
and session title, confirming the destination. Computer-use policy blocked
inspection of Codex itself; its destination UI is **not visually verified**.
No messages were sent to either session.

Physical finger testing, Linux runtime navigation, and optical panel inspection
were not performed. The C detector is now included in both Linux and macOS CI
jobs, but remote CI was not run in this task. Both app and firmware must be
updated together because older desktop decoders do not understand input kind 3.

## Evidence

- [Summary](../../target/board-double-tap/summary.json)
- [Per-case results](../../target/board-double-tap/results.json)
- [Source and build hashes](../../target/board-double-tap/manifest.json)
- [Board serial log](../../target/board-double-tap/serial.log)
- [BLE host log](../../target/board-double-tap/host.log)
- [Live navigation dispatch](../../target/board-double-tap/live-navigation.log)
- [Claude destination observation](../../target/board-double-tap/claude-destination.txt)
- [Claude fixture focus](../../target/board-double-tap/36-double-0.png)
- [Codex fixture focus](../../target/board-double-tap/36-double-1.png)
- [Live board after Codex activation](../../target/board-double-tap/live-after-double.png)

Artifacts remain local in ignored `target/board-double-tap/`. Provisioned firmware
contains the board's pairing key and must not be published.

## Cleanup

Production LCD firmware was restored and flash hashes verified. Its generated
configuration excludes `CONFIG_TC_QA_USB`; a USB test command received no QA
acknowledgment ([verification](../../target/board-double-tap/production-check.txt)).
The rebuilt native app was restarted in normal live BLE mode.
