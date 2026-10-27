# Board hardware integration QA report

**Date:** 2026-10-13 (Asia/Saigon). **Verdict:** automated software/board integration
PASS after fixes; full physical display/sensor qualification remains incomplete.

## Executive results

- **42/42 final automated hardware cases passed**, zero failures.
- **44 framebuffer captures** read from the actual board over USB and checked
  byte-for-byte against the host frame and an independently action-driven reference
  scene: 261120 bytes per capture, 480×272 RGB565. PNGs were also visually inspected.
- Eleven isolated response samples: **median 700 ms**,
  **p95 820 ms**, **maximum 820 ms** from
  injected release processing to the matching LCD-frame completion log. The
  predeclared p95 ≤1500 ms / maximum ≤2000 ms criteria passed.
- No CRC mismatch, unexpected reconnect, crash, or final-state loss in the verified
  suite. Twelve rapid taps and ten repeated toggles reached the expected final state.
- A 60-second stability interval completed with the same captured pixels and a
  healthy BLE connection. This is a short soak, not endurance or leak qualification.
- One reproduced product defect was fixed: an unfinished touch survived reconnect
  and caused an unintended selection. Out-of-panel scroll handling was also hardened.
- An existing font limitation remains: accented/non-ASCII characters and emoji
  render as `?`. UTF-8 wrapping and board/host consistency pass; glyph fidelity does not.

## Environment and tested path

Board: JC4827W543, ESP32-S3, 8 MB PSRAM, 4 MB flash, NV3041A QSPI LCD and GT911 touch.
BLE identity: TokenCafe-6356. Serial: `/dev/cu.usbmodem2101`. Host: this macOS machine.
Repository branch `cmpham`, base HEAD `8e6e19f08fb6264d536c1ef2bfa80588ac4228c0`, with uncommitted changes.
The [build manifest](../../target/board-e2e-verified/build-manifest.json) records
SHA-256 hashes of the diagnostic firmware, production firmware, host and harness.
Pairing keys and factory flash backups are not included in the report.

```
USB scripted controller sample
  → real board gesture recognizer and input queue
  → authenticated BLE input / production Mirror
  → production Input.apply and Scene on the host
  → RGB565/compression/BLE frame protocol
  → board staging, checksum verification, LCD write, committed buffer
  → USB coherent framebuffer snapshot → exact pixel comparison and PNG
```

This is hardware-in-the-loop component integration. The deterministic host uses
the same production renderer, input handler and BLE worker as the native app, but
freezes animation/time and supplies synthetic session fixtures. The oracle is a
separate Scene driven by explicit expected actions rather than received events.
It therefore tests gesture interpretation, coordinate mapping, state routing,
transport and pixel integrity; it is not an independent implementation of the
renderer. Visual review and the existing scene tests supplement the oracle.
Native window/compositor behavior and live session collection are outside this
42-case harness; normal-app restart/connection is checked separately at cleanup.

The firmware diagnostics are a **separate build profile**, disabled by default.
Snapshots are copied under a mutex after presentation; offsets, byte count and
CRC are validated. The harness requires two fresh consecutive acknowledgments
and the expected input count before capturing, avoiding stale checksum matches.

## Execution history and findings

| Finding | Evidence | Resolution |
|---|---|---|
| P2: held touch leaked across reconnect | Before fix: press, terminate host, reconnect, release produced one input and selected patron 0; [record](../../target/board-e2e-stale-before/result.json), [capture](../../target/board-e2e-stale-before/unexpected-selection.png) | Fixed with connection-generation tags on gestures and queue entries. Final case 30 had zero leaked inputs and correct pixels. |
| P2: scroll could move outside panel | Inspection found continuing rail movement lacked coordinate bounds checks. Such samples could reach host validation with invalid coordinates. | Out-of-panel movement cancels the gesture. Cases 09 and 28 pass; no reconnect or unintended final page. The before-state was identified statically, not reproduced as a hardware failure. |
| Test harness: historical ACK accepted as current | Expanded pre-final run was 41/42: scroll-clamp capture differed by 7422 bytes; [original results](../../target/board-e2e-final/results.json) and expected/actual PNGs retained. | Fixed synchronization to require fresh consecutive ACKs and all expected inputs. Two Python regression tests pass; strict rerun 42/42. |
| P2/open: unsupported font glyphs | [Unicode capture](../../target/board-e2e-verified/17-unicode.png); `src/gfx.rs` explicitly maps non-ASCII to `?`. | Existing renderer limitation, unchanged. Do not interpret Unicode case PASS as full glyph support. |

The first exploratory 32-case run passed. A focused adversarial reconnect test then
reproduced the stale-touch defect. The expanded run exposed the harness race.
Only the final `board-e2e-verified` results are used for the release verdict.
Previous runs and failures remain available for audit.

## Final hardware case results

Times below are release-to-LCD-completion milliseconds; a dash means the case
was functional/visual rather than a latency sample. Capture links show actual
board framebuffer pixels, not host-generated substitutes.

| Case | Result | Latency ms | Board CRC / capture |
|---|---|---:|---|
| 01-initial-full-sync | PASS | — | [acebf771](../../target/board-e2e-verified/01-initial-full-sync.png) |
| 02-select-seat-0 | PASS | 640 | [8cc16a04](../../target/board-e2e-verified/select-0.png) |
| 02-select-seat-1 | PASS | 670 | [0b300b8c](../../target/board-e2e-verified/select-1.png) |
| 02-select-seat-2 | PASS | 780 | [32bea2ec](../../target/board-e2e-verified/select-2.png) |
| 02-select-seat-3 | PASS | 660 | [50c51813](../../target/board-e2e-verified/select-3.png) |
| 02-select-seat-4 | PASS | 710 | [af88d7f8](../../target/board-e2e-verified/select-4.png) |
| 02-select-seat-5 | PASS | 740 | [7a485564](../../target/board-e2e-verified/select-5.png) |
| 02-select-seat-6 | PASS | 820 | [6bd2f3f4](../../target/board-e2e-verified/select-6.png) |
| 02-select-seat-7 | PASS | 670 | [b031b16b](../../target/board-e2e-verified/select-7.png) |
| 03-toggle-off | PASS | — | [acebf771](../../target/board-e2e-verified/03-toggle-off.png) |
| 04-long-hold-release | PASS | 700 | [8cc16a04](../../target/board-e2e-verified/04-long-hold-release.png) |
| 05-nonrail-drag | PASS | — | [acebf771](../../target/board-e2e-verified/05-nonrail-drag.png) |
| 06-multitouch | PASS | — | [acebf771](../../target/board-e2e-verified/06-multitouch.png) |
| 07-out-of-range | PASS | — | [acebf771](../../target/board-e2e-verified/07-out-of-range.png) |
| 08-release-alone | PASS | — | [acebf771](../../target/board-e2e-verified/08-release-alone.png) |
| 09-move-outside | PASS | — | [acebf771](../../target/board-e2e-verified/09-move-outside.png) |
| 10-footer-next | PASS | 510 | [0b300b8c](../../target/board-e2e-verified/10-footer-next.png) |
| 11-scroll-endpoints | PASS | — | [8cc16a04](../../target/board-e2e-verified/11-scroll-first.png) |
| 12-scroll-opens-focus | PASS | — | [62ea2513](../../target/board-e2e-verified/12-scroll-opens-focus.png) |
| 13-empty | PASS | — | [aa0619d8](../../target/board-e2e-verified/13-empty.png) |
| 14-one-long-name | PASS | — | [5827b6ba](../../target/board-e2e-verified/14-one-long-name.png) |
| 15-overflow | PASS | — | [5474e4bf](../../target/board-e2e-verified/15-overflow.png) |
| 16-missing-task | PASS | — | [32bea2ec](../../target/board-e2e-verified/16-missing-task.png) |
| 17-unicode | PASS | — | [50c51813](../../target/board-e2e-verified/17-unicode.png) |
| 18-attention | PASS | — | [9de44681](../../target/board-e2e-verified/18-attention.png) |
| 19-manual-with-attention | PASS | — | [5bc25d34](../../target/board-e2e-verified/19-manual-with-attention.png) |
| 20-task-replaced | PASS | — | [1dfc03fd](../../target/board-e2e-verified/20-task-replaced.png) |
| 21-selected-removed | PASS | — | [b81c8a87](../../target/board-e2e-verified/21-selected-removed.png) |
| 22-rapid-12-taps | PASS | — | [50c51813](../../target/board-e2e-verified/22-rapid-12-taps.png) |
| 23-repeat-toggle-10 | PASS | — | [acebf771](../../target/board-e2e-verified/23-repeat-toggle-10.png) |
| 24-host-reconnect | PASS | — | [acebf771](../../target/board-e2e-verified/24-host-reconnect.png) |
| 25-touch-after-reconnect | PASS | 760 | [8cc16a04](../../target/board-e2e-verified/25-touch-after-reconnect.png) |
| 26-four-corners | PASS | — | [62ea2513](../../target/board-e2e-verified/26-four-corners.png) |
| 27-scroll-clamped | PASS | — | [62ea2513](../../target/board-e2e-verified/27-scroll-clamped.png) |
| 28-scroll-leaves-panel | PASS | — | [9f424e5b](../../target/board-e2e-verified/28-scroll-leaves-panel.png) |
| 29-invalid-usb-commands | PASS | — | [acebf771](../../target/board-e2e-verified/29-invalid-usb-commands.png) |
| 30-held-across-reconnect | PASS | — | [acebf771](../../target/board-e2e-verified/30-held-across-reconnect.png) |
| 31-footer-overflow | PASS | — | [e79647cd](../../target/board-e2e-verified/31-footer-overflow.png) |
| 32-removal-animation-complete | PASS | — | [d191fd57](../../target/board-e2e-verified/32-removal-animation-complete.png) |
| 33-unicode-last-page | PASS | — | [689bc9cf](../../target/board-e2e-verified/33-unicode-last-page.png) |
| 34-attention-rotation | PASS | — | [e1d49630](../../target/board-e2e-verified/34-attention-rotation.png) |
| 35-idle-60s | PASS | — | [acebf771](../../target/board-e2e-verified/35-idle-60s.png) |

## Supporting checks

- Rust: 43 library + 8 native-app tests passed; BLE-disabled build: 38 + 8 passed.
- Clippy passed with warnings treated as errors.
- Two Python synchronization-barrier regression tests passed.
- 51 Rust encoder → actual C firmware decoder full/delta fixtures passed under
  ASan/UBSan, including random/raw fallback. C tests also cover CRC known vector,
  all 65536 packet-length header values, malformed RLE/LZ4, overlapping LZ4 copies,
  truncated blocks and no partial mutation on failure.
- Display production and headless firmware profiles built successfully. This is
  cross-compilation for those ESP targets, not a physical run on the old board.
- Physical protocol fault test: see [hardware test log](../../target/board-e2e-verified/protocol-hardware.log).
  Covers wrong-key rejection, full/delta CRCs, invalid RLE atomicity, interruption
  restoring the committed framebuffer, and reconnect/full resynchronization.

## Visual review

Reviewed the selected hand/portrait/title relationship, two-line task area,
far-right scrollbar, first/last-page text, empty state, overflow indicator,
attention/manual focus, task replacement and removal transition. No unexpected
crop, padding, corrupted block, or mismatch was seen in captured framebuffers.
The removal test captures both the departure transition and fully cleared seats.
Unicode fallback is visible and recorded above.

[Contact sheet 1](../../target/board-e2e-verified/contact-1.png) ·
[2](../../target/board-e2e-verified/contact-2.png) ·
[3](../../target/board-e2e-verified/contact-3.png) ·
[4](../../target/board-e2e-verified/contact-4.png) ·
[5](../../target/board-e2e-verified/contact-5.png)

## Measurement limits and coverage gaps

- **NOT RUN:** optical panel inspection by camera, true finger-contact-to-photon
  timing, gamma/inversion/backlight/tearing/viewing-angle qualification. A correct
  framebuffer cannot establish any of these.
- **NOT RUN:** real GT911 finger/stylus accuracy, noisy/wet/multiple physical
  contacts, electrical signal integrity. Injection starts after the driver.
- **NOT RUN:** hard power cuts/brownouts, RF range/interference, battery/thermal
  endurance, queue saturation beyond tested bursts, multi-board ambiguity.
- **NOT RUN:** Linux desktop/Bluetooth runtime and a Windows host. Shared Rust
  code and BLE-disabled tests run on macOS; no cross-platform runtime claim.
- **NOT RUN:** an Ask-specific attention message in this deterministic fixture;
  permission waiting, multiple waiting patrons, manual focus and rotation are covered.
- The 11 timed samples are a small, deterministic workload with diagnostic
  firmware. They do not establish production p95 under arbitrary live animations,
  RF load or prolonged use. Prior manual live-app timings are a different workload.
- The LCD completion log is not an optical sensor. Injection also bypasses the
  controller's scan time; press/release timing represents firmware processing.

## Reproduce and artifacts

Follow [the detailed plan](board-test-plan.md) and
[the firmware suite instructions](../../firmware/ble/README.md#automated-real-board-gesture-and-capture-suite).
Runner: `tools/board_e2e.py`; host: `examples/board_e2e.rs`.

[Machine-readable summary](../../target/board-e2e-verified/summary.json) ·
[Per-case results](../../target/board-e2e-verified/results.json) ·
[USB/board log](../../target/board-e2e-verified/serial.log) ·
[Host/BLE log](../../target/board-e2e-verified/host.log).
Large evidence files are local under ignored `target/`, not committed or uploaded.

## Cleanup status

Complete. Physical protocol fault checks passed. Production firmware was flashed
successfully with digest verification; its build has no `CONFIG_TC_QA_USB` define,
and a USB diagnostic command produced no QA acknowledgment. The regular native
app was rebuilt and restarted, its UI title reports `TokenCafe-6356 mirrored`,
and new checksum-verified live frames are arriving. The native window was visually
inspected. Diagnostic serial ownership was released; no pairing keys were uploaded.
Changes remain uncommitted.

[Production diagnostic-disable check](../../target/board-e2e-verified/production-diagnostics-check.txt) ·
[Restore log](../../target/board-e2e-verified/production-restore.log) ·
[Live app smoke log excerpt](../../target/board-e2e-verified/live-smoke.log).
