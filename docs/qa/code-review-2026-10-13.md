# Working-version review — 2026-10-13

Reviewed the current uncommitted `cmpham` working tree, preserving the earlier
LCD, BLE, navigation, UI and QA work. This was a local review and repair pass,
not an independent-agent review or a claim that every possible defect is absent.

The review traced desktop input/selection/navigation, native platform guards,
collector/session identity, usage-history traversal, BLE discovery/authentication/
requests/reconnect, frame encoding and staging, firmware touch/initialization,
and the diagnostic test paths. Generated sprite data was not redesigned, and
vendor driver internals were not changed.

## Issues addressed

| Area | Failure before this pass | Resolution / evidence |
| --- | --- | --- |
| Firmware initialization | HMAC calculation, BLE registration/advertising and task creation were inside `assert` expressions. Disabling assertions removes the calls themselves. | Replaced production assertions with an always-evaluated `TC_CHECK`. Both board profiles build; an `NDEBUG` host check confirms side effects still execute. |
| Touch-controller read errors | Clearing gesture state during a held contact allowed its continuation/release to become a new tap. | Cancel until an explicit release. The shared gesture recognizer has a sanitizer regression covering error/hold/release, multi-touch, drag, and reconnect. |
| BLE setup | Adapter, discovery, services and authentication operations had unbounded waits. Failed discovery could leave scanning active; failed connects skipped disconnect cleanup. | Bounded each setup phase, stop scans on errors, disconnect failed/expired connections. Hardware reconnect and authentication checks exercise the normal/recovery paths; OS backend hangs were not physically induced. |
| Capability negotiation | Any failure was treated as old firmware, including CRC, sequence and timeout failures. | Only a fully validated unsupported-opcode status permits legacy fallback. Extracted reply parser tests cover fragmentation, wrong sequence/opcode, CRC corruption, unsupported status and extra/oversized data. |
| Disconnect input/status | Undelivered events stayed queued until a later successful connection; status could still say mirrored during the retry delay. | Clear pending input and publish disconnected/retrying status as soon as the connection loop fails. |
| Session reconciliation | A session that returned during its departure animation stayed in Leaving mode, could not be clicked and was eventually removed despite being live again. | Restore its occupied seat. A regression ticks past the departure duration and verifies its identity and click target survive. |
| Name shortening | Very small requested widths could underflow when preserving a `-suffix`. | Preserve suffixes only when the width can fit them; test widths 0–4. |
| Usage traversal | Recursive scanning followed directory symlinks and counted symlinked transcripts, allowing cycles and duplicate counting. | Visit regular directories/files only. Unix regression includes a directory backlink and duplicate transcript link. |
| QA evidence | The live runner accepted historical launch success, and its captures lacked CRC/header validation. Reader closures could bind to a replacement host's queues. | Require a fresh launch-log suffix and stable seat map, share strict capture validation, require END/size/CRC, and bind readers to their original process and queues. Capture corruption/size tests supplement the existing acknowledgment-barrier tests. |

## Structure

- `src/companion/codec.rs`: pure pixel/frame encoding and bounded update planning.
- `src/companion/reply.rs`: strict reply validation and typed remote status.
- `firmware/ble/main/touch_gesture.h`: hardware-independent gesture state machine,
  used by both real GT911 sampling and injected samples.
- `firmware/ble/main/check.h`: always-evaluated firmware initialization checks.
- Native touch tracing is outside the render-loop body. Both Python runners use
  the same framebuffer-capture validator.
- CI now runs portable C gesture and protocol tests with ASan/UBSan on its macOS
  and Linux jobs, alongside the existing Rust checks.

## Validation

- 48 Rust library + 8 app tests pass; BLE-disabled build: 41 + 8 pass.
- Clippy all targets, formatting, and diff-whitespace checks pass locally.
- 5 Python regression tests pass.
- Portable double-tap, shared-gesture and protocol sanitizer checks pass; the
  protocol check includes all 65,536 header payload lengths.
- 51 Rust encoder → C decoder fixtures pass, including full frames and deltas.
- Diagnostic LCD, production LCD and original headless firmware builds pass.
- macOS app bundle rebuilt successfully.

### Physical board verification

- All **49/49** automated cases passed on the connected JC4827W543, using
  firmware-injected touch samples, authenticated BLE and CRC-checked framebuffer
  captures compared with expected desktop-rendered bytes.
- Coverage includes selection/toggle, hit boundaries, scroll, session removal,
  attention state, disconnect/reconnect, held contacts and double-tap behavior
  with synthetic Codex/Claude destinations.
- 11 measured injected releases reached LCD completion at **660 ms median,
  750 ms p95/max**. No transport errors were reported. These are firmware
  timestamps, not optical measurements; the remaining delay can still be felt.
- The rebuilt native app separately passed wrong-key rejection, full-frame and
  delta transfers, invalid RLE rejection without framebuffer mutation, touch
  preemption/rollback, and reconnect/full resynchronization.
- Inspected captured attention and scrolled-focus views: layout, colors,
  selection marker and footer rendering were consistent with the expected UI.

Local artifacts live under ignored `target/board-review/` (`summary.json`,
`results.json`, captures and logs); provisioned binaries must stay private.

### Restored running state

Production LCD firmware was flashed and hash-verified after QA. Its configuration
has USB QA disabled, and a diagnostic-mode request received no QA acknowledgment.
The rebuilt normal macOS app is running again and authenticated with
`TokenCafe-6356`; repeated CRC-verified live frame updates confirm the connection.
All code changes remain uncommitted.

## Limits

Linux runtime and remote CI were not executed in this pass. Existing compile-time
macOS guards remain intact; local BLE-disabled tests are not a substitute for a
Linux window/BLE smoke run. Touch injection verifies gesture logic and transport,
not physical finger accuracy or optical panel output. Destination apps were not
reopened during this repair pass; prior live session-navigation evidence remains
in the separate touch-area/double-tap reports. The pixel font still has limited
Unicode coverage, and API cost figures remain estimates from the bundled table.

## Follow-up: physical touch orientation regression

The user reported that double-tapping the top-right `cf-bluetooth` Codex patron
opened a Claude session. Live input logs showed the contact arriving in the
bottom-left cell (scene coordinates around 40,156). LCD initialization uses
MADCTL `0xc0`, reversing both axes, while GT911 samples had no matching transform.
The prior 49 injected-touch cases bypassed the controller coordinate space, so
those passes did not establish correct physical hit areas.

Physical GT911 contacts now map to `(479-x, 271-y)` before gesture recognition.
Out-of-range contacts cancel the gesture. Diagnostic samples retain their
existing display-coordinate contract. Portable ASan/UBSan tests cover all eight
seat centers through single/double-tap recognition, both endpoint directions,
and invalid coordinates. Production firmware built and was flashed/hash-verified, then reauthenticated
with the normal app. After the user physically repeated the tap, the host logged
`DoubleTap(284, 78)` and dispatched the expected `codex://threads/` destination
for this cf-bluetooth session, rather than the previous Claude URL. This confirms
physical coordinate routing and URL dispatch. OS launch success alone does not
independently prove the destination application's visible page.
