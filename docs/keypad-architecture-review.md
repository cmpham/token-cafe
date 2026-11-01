# Board, companion, and keypad implementation review

Reviewed 2026-11-01 against the current working tree (including the accumulated, uncommitted keypad implementation). This is a scoped implementation/structure review, not a claim that every application feature or physical interaction is verified.

## Responsibilities and dependencies

| Module | Owns | Must not own |
| --- | --- | --- |
| `crates/companion-core` | Authority, validated protocol, replica, navigation, approval rules, OLED text, activity semantics | macOS accessibility, Bluetooth platform calls, LVGL |
| `src/companion/state.rs` | Collector-to-authority state and shared view reconciliation | Device HID parsing |
| `src/companion/keypad.rs` | Local non-decision peer, helper lifecycle, command dispatch and OLED rows | Another copy of navigation/approval policy |
| `src/companion/semantic.rs` | Mac-to-board authentication, ordered transfers, reconnects, model/blob scheduling | Physical keypad calibration or discovery |
| `native/keypad/Protocol.swift` | Input validation, transport preference, line output | Device discovery or app activation |
| `native/keypad/BluetoothLink.swift` | Encrypted identity verification, notification subscription, serialized OLED writes, reconnects | Session choice or desktop actions |
| `native/keypad/main.swift` | USB vendor interface, USB/BLE arbitration, bounded stdin frames, process ownership | Global keyboard capture |
| `native/desktop/Controls.swift` | Session/route/control matching and bounded native accessibility helpers | Bluetooth or keypad mapping |
| `native/desktop/main.swift` | Explicit app toggle/native recording invocation | Approval decisions, prompt submission, global-voice fallback |
| `firmware/keypad/module/src/transport.c` | USB/GATT protocol, heartbeat selection, input mapping, frame storage | LVGL widgets |
| `firmware/keypad/module/src/display.c` | LVGL layout, battery overlay and rendering | Pairing and host commands |
| `firmware/shared/keypad_display.h` | Portable bounded, ordered frame parser | Hardware or desktop state |
| `firmware/engine` | Rust board adapter around the shared client and renderer | Keypad Bluetooth connection |
| `firmware/semantic/main/main.c` | ESP32 lifecycle, peripheral GATT, queued receipt, NVS, touchscreen/GPIO/LCD loop | Keypad central/scanner/HID/calibration |

The board C entry point remains a compact hardware integration module, with hardware drawing/touch in `firmware/ble/main/board.c` and semantic behavior behind the engine C API. Splitting it further during this cleanup would change task/lock ownership without fixing a demonstrated defect. It has no remaining direct-keypad code.

## Cleanup performed

- Split firmware transport/state from the OLED renderer; the renderer copies a complete frame under the transport mutex and renders after releasing it.
- Split Mac Bluetooth transport from USB/application entry point and pure protocol helpers. Split native recording helpers from their executable dispatch.
- Moved inline native self-tests into `native/tests/{keypad,desktop}/main.swift`. Production helpers no longer ship a `--self-test` branch. Tests use `precondition`, because Swift `assert` can be removed by optimized `-O` builds. Earlier optimized self-test PASS lines alone were therefore insufficient evidence; the new test runner executes real checks under `-O`.
- Added `tools/test_keypad.sh` to compile/run both pure Swift suites and the C frame parser under address/undefined-behavior sanitizers, with temporary outputs cleaned afterward.
- Removed the unused stock shield keymap (which still mapped the knob to a layer switch). The explicit VIA-aware application keymap is the single build default; stored settings are a separate source of truth and must be read back.
- Saved all known-good non-sensitive VIA bindings in `firmware/keypad/keymap-reference.json`.
- Corrected stale calibration/knob-mode comments and updated build paths for the source split. Bluetooth permission descriptions now mention both keypad and optional board.
- Replaced the current keypad runbook with a single navigation point covering both architectures, verified hardware identity, exact binding codes, build pins, serial recovery, permissions, OLED failure diagnosis, and acceptance limits. Historical claims remain in an explicitly historical document.

## Code deliberately retained

`firmware/v2` uses engine render/status/drop exports for the QEMU harness. `CONFIG_TC_QA_USB` gates local injection/frame capture; `CONFIG_TC_EXTERNAL_IO_DIAGNOSTIC` gates the physical wiring diagnostic. Both default off. Production authentication rejects an injectable QA peer. These are maintained verification tools, not leftover live debugging code; removing them would weaken regression coverage. The display parser tests and Rust regression tests remain outside production behavior.

VIA custom keycodes, settings layer, and macros support remain for compatibility and recovery. Removing them would risk interpreting saved settings differently. The shared client's keypad behavior remains because the **Mac** local peer uses it, even though the board no longer exports keypad entry points.

## Findings and remaining limits

| Priority | Finding | State |
| --- | --- | --- |
| Medium, confirmed | Optimized Swift inline self-tests could report PASS with assertions removed | Fixed; separate optimized suites now use executable preconditions |
| Medium, confirmed | Board view delivery still occasionally stalls 2.25–2.4 s despite one-revision coalescing | Open performance investigation; median 289 ms across the observed 48 acknowledgements. Measurements exclude pre-publish input wait and LCD presentation |
| Medium, confirmed behavior | Normal Finder launch does not enable the optional board connection | Documented; use configured launcher. `Board: optional` is not a successful board link |
| Low, confirmed maintenance constraint | Physical serial/identity is pinned in multiple places | Documented as single-device support; new hardware requires coordinated firmware/helper configuration |
| Low, platform constraint | Ad-hoc rebuilds can invalidate macOS privacy grants | Stable release path and explicit signatures retained; do not promise permanent grants |
| Acceptance gap | Codex Dictate activation and reconnect-to-USB preference need explicit physical confirmation | Kept separate from successful protocol handshakes, pure tests, and Claude recording acceptance |
| Recovery constraint | Historical direct-board implementation has no recorded committed rollback revision | Documented as historical only; saved packages under ignored `target` are not a durable reproducible release |

No new board protocol or keymap behavior was introduced in this modular cleanup. USB priority, encrypted peer checks, no approval commands, offline discard, host-side target validation, and preserved NVS/settings remain the intended contracts. Pure tests and builds cannot establish actual microphone capture, radio stability, or knob-to-screen latency.

## Validation / deployment distinction

- Workspace: 198 tests pass (`target/keypad-review-workspace-tests.log`).
- Native: 50 transport/parser and 18 recording-match checks pass under optimization; C display parser passes ASan/UBSan (`target/keypad-review-native-tests.log`). These tests do not interact with other apps or devices.
- Pinned keypad build succeeds with separated transport/renderer (`target/keypad-review-firmware-build.log`). Upstream deprecated-option/operator-parenthesis warnings remain; no warnings from the local split should remain.
- Signed Mac release app rebuilt at `/Users/cuong/games/token-cafe/release/Token Café.app` (`target/keypad-review-app-build.log`). This publication is not a claim that all existing running processes have reloaded those binaries.
- Board application was built/flashed and synchronized before this cleanup. Its production source was not functionally changed by the cleanup, so it was not reflashed.
- The refactored keypad firmware is build-verified, not flashed. The installed Mac-only keypad build and its verified hash are recorded in the [runbook](pipar-keypad.md).
