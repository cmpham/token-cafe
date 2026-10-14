# ESP32-S3 companion board test plan

Date: 2026-10-13. Target: JC4827W543, 480×272 NV3041A QSPI LCD, GT911 touch,
ESP32-S3 with 8 MB PSRAM, macOS desktop host, authenticated BLE mirroring.

## Objectives and release criteria

Verify that touchscreen gestures cause the correct selection/task state and that
exactly those pixels reach the board's committed framebuffer. Verify full-screen
RGB565 encoding, atomic updates, cancellation, recovery, bounded input behavior,
and measured latency. Never substitute a transfer benchmark for touch response.

Critical functional cases must have exact pixel equality between (1) an expected
scene driven by explicit test actions, (2) the actual scene driven only by board
BLE input, and (3) a USB snapshot of the board's committed framebuffer. Negative
gestures must leave the scene unchanged. No CRC error, crash, reconnect loop, or
partial commit is acceptable. Latency target: release-to-LCD p95 ≤1500 ms for
isolated selections, maximum ≤2000 ms; report failures rather than relax limits.

## Automation architecture and controls

A diagnostic firmware build enables a local USB command interface, disabled by
default in normal firmware. Samples enter the same gesture recognizer as GT911
samples, before tap/drag/multitouch interpretation. Test mode suppresses physical
samples to make the run deterministic. Screen captures copy the last completed
frame under a mutex; USB transports captures and test commands only. Touches and
screen updates still traverse BLE in both directions.

A deterministic host harness uses production Scene, input application, Mirror,
compression, authentication and reconnection code. Its reference scene is driven
by explicit expected actions, never by received input. Animation/time are frozen
for exact comparison. This is a hardware integration suite; native window events,
live process/session collection, and real GT911 sensor detection are separate
checks. A final normal-app smoke test restores real operation.

## Case matrix

| Area | Required cases and assertions |
|---|---|
| Boot/connection | Cold/reset boot, BLE auth, negotiated fast protocol, initial full sync, stable frame dimensions and CRC |
| Basic selection | Each of eight seats, same-seat toggle-off, long hold (no activation before release), footer next patron, selection and footer coherence |
| Gesture rejection | Non-rail drag, multitouch, out-of-range sample, empty-space taps, release without press; no unintended action |
| Scroll | Rail opens focus, drag to first/last page, clamping, repeated endpoint, scroll after selecting another patron |
| Screen/content | Empty, one/eight/ten sessions, missing task, Unicode/long word, long title, large token/cost values, replaced task, selected session removed |
| Attention | Waiting/permission/ask state, auto attention, manual selection while another patron waits |
| Transport | Full and delta updates, CRC equality, fragmented writes, malformed RLE/LZ4, wrong key, abort/restore, reconnect and full resync |
| Stress | Rapid taps, repeated selection/clear cycles, queued gestures, burst while transfer active, sustained stable-frame integrity |
| Performance | Timestamp injected press/release processing on board; correlate input generation with completed CRC; median/p95/max, excluded samples and superseded inputs |
| Recovery | Restart host and board, verify resync and fresh input after reconnection; no stale queued touches |
| Compatibility | Rust tests with/without BLE, Clippy, display and headless firmware builds, portable C ASan/UBSan fixtures |

## Evidence and execution

Record exact git HEAD/dirty state, toolchain/build, board identity, scenario inputs,
expected/actual frame CRCs, PNG captures, result JSON, serial and host logs. Save
failures with diffs; continue independent cases. Re-run affected cases after fixes
and distinguish original failures from final results. Inspect a contact sheet and
representative full-size captures rather than relying only on checksums.

## Explicit manual / unavailable coverage

A framebuffer is not an optical photograph. Camera/manual checks are required for
panel inversion, gamma, backlight, tearing/flicker, physical alignment, viewing
angles and actual photons. Real sensor edge accuracy, pressure, wet fingers,
stylus behavior and electrical noise need physical touches. Power loss/brownout,
RF interference/range, long battery/thermal endurance and Linux runtime require
appropriate hardware/environment. Mark these NOT RUN, never PASS by inference.

## Cleanup

Disable injection, release USB capture ownership, flash the normal production
profile, restart the regular desktop app, and confirm live BLE frames. Do not
commit, publish logs, or expose pairing keys as part of this task.

## Detailed procedures (panel coordinates are derived from 320×240 scene coordinates)

Every isolated case starts from a fresh eight-session fixture unless specified.
Inject through USB, then wait for the required BLE input count and **two fresh,
consecutive** acknowledgments of the reference CRC. Historical matches never
satisfy the barrier. Capture the physical board's committed buffer only after this
barrier; assert ordered offsets, exactly 261120 bytes, snapshot CRC and exact
pixel equality. Save PNG and raw RGB565 even when a pixel assertion fails.

| IDs | Actions | Expected result |
|---|---|---|
| 01 | Boot diagnostic image; connect authenticated host; send first frame | Full 480×272 image with no padding; board/host/reference CRCs agree |
| 02 × 8 | Press/release each seat center, rows y=80/160 and x=40/120/200/280 | Correct hand, portrait, title and task; one event per tap |
| 03–04 | Select same seat twice; hold a fresh press for 500 ms before release | Second tap clears; hold alone does not activate; release activates once |
| 05–09 | Drag >8 panel pixels off the rail; introduce second contact; press outside; release alone; move outside | No tap, no new selection, no extra input count |
| 10–12 | Tap footer; drag rail to y=239 then y=192; touch rail without focus | Footer advances patron; last/first task pages match; rail opens focus |
| 13–17 | Render zero/one/ten sessions; long name/usage; focus absent task/Unicode task | No overflow outside frame; OUTSIDE indicator for hidden seat; missing-task label; glyph limitations recorded |
| 18–21 | Waiting patron, manual selection while waiting, replace selected task, remove sessions | Attention remains visible; manual focus is respected; replacement resets page; removal clears focus |
| 22–23 | Twelve rapid taps; ten select/clear cycles | All expected inputs consumed; final state correct; superseded intermediate frames allowed; no corruption |
| 24–25 | Stop/restart test host, await new auth/full sync, then tap | New connection is usable; fresh tap renders correct selection |
| 26–29 | Four panel corners; repeat bottom rail endpoint; leave panel while scrolling; malformed USB commands | Coordinate mapping and clamping correct; invalid movement/commands cause no unintended action |
| 30 | Hold press; stop host; reconnect; release without resetting test mode | Old gesture produces **zero** new-session inputs |
| 31–34 | Footer from eighth seat with ten sessions; advance removal animation; Unicode final page; advance attention clock | Overflow patron reached; seats clear; long-word wrapping stays bounded; attention rotates |
| 35 | Leave deterministic scene running for 60 seconds, then recapture | Same pixels and healthy connection; this is a short soak, not endurance qualification |

### Fault and transport acceptance

Run `--bluetooth-test` on the real board separately from the scene host so only one
BLE central owns the connection. Assert wrong-key rejection before any frame,
full and sparse-delta CRCs, atomic malformed-RLE rejection, staged-update abort
restoring the committed baseline, and reconnect resynchronization. Exercise all
Rust-encoded fixture frames and transitions against the actual C decoder under
ASan/UBSan, including random/raw fallback, long overlapping LZ4 copies, invalid
backreferences, truncation and all 65536 packet-length header values.

### Result classification

PASS requires executed assertions and evidence. FAIL is a reproduced mismatch or
threshold breach. NOT RUN means unavailable environment or a manual/optical
requirement. A test-runner failure is retained separately from product defects.
A shared renderer can reproduce the same renderer defect in both model and host;
visual inspection and existing independent scene tests supplement pixel equality.
Unicode-safe wrapping does not imply full Unicode font support.

## Double-tap navigation extension (2026-10-13)

Run with updated firmware and desktop decoder together. Inject two short contacts
at the same patron with releases less than 450 ms apart. Verify one immediate
single-tap event followed by one kind-3 activation, stable patron focus, and
exactly one validated navigation URL. Exercise both Claude Desktop and Codex
fixtures; the deterministic host records URLs rather than opening fake sessions.

Negative cases: releases more than 450 ms apart toggle normally; different
patrons do not activate; missing desktop targets remain focused; footer double
taps continue cycling patrons; intervening multi-touch cancels the pair. Existing
reconnect, held-touch, drag, bounds and scroll tests remain required. Portable C
tests cover the exact threshold, radius boundary, first-coordinate anchor,
triple-tap behavior, long holds, connection epochs and clock rollover.

After fixture testing, inject a double tap into the native live Mac app and
verify LaunchServices dispatch for a real mapped session. Separately report
whether the destination app/session was visually observed; successful `open`
exit status alone does not establish the destination UI. Restore production
firmware with diagnostic USB disabled, and relaunch the normal BLE companion.
Physical finger accuracy and optical output still require physical verification.

## Physical orientation acceptance gate

Use the [required LCD settings](../protocol/operations.md#lcd-orientation-and-color-settings):
MADCTL `0xc0`, inversion ON (`0x21`), and reversed GT911 axes. Read the boot LCD
register diagnostics, then verify upright text and normal colors physically;
inversion OFF is not the correct color setting for this panel. Repeat the visual
check after reset and backlight sleep/wake.

After changing LCD orientation, touch acquisition, or production firmware, verify
physical taps at the four corner patron cells against their visible labels.
Then physically double-tap one known Codex and one known Claude patron and check
the intended destination. USB-injected samples are display coordinates: they
cannot establish correct GT911-to-display orientation. Keep the portable raw
coordinate conversion tests alongside gesture tests, and never count synthetic
hit-area passes as physical calibration evidence.
