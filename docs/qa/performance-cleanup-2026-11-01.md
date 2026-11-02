# Desktop and board cleanup — 2026-11-01

Baseline: `1832a12` (`Focus keypad sessions in one press and refresh docs`). Android is outside this review. The initial commit includes the previously completed keypad flow, README/CLAUDE updates, and refreshed media.

## Review scope and adopted changes

Reviewed the desktop update/render path, local session/transcript collectors, monthly accounting, native keypad/recording helpers, host authority/model mapping, shared client/renderer, board engine FFI, and semantic BLE/LCD/touch adapter. This is a maintenance/performance pass, not a claim of exhaustive security or physical-hardware acceptance testing.

| Change | Reason and preserved behavior |
| --- | --- |
| Remove the renderer's old waiting-focus counter/list and write-only per-agent branch/subagent copies | Automatic footer rotation was removed earlier. These fields only fed obsolete bookkeeping, not the visible UI. Session/wire metadata remains available. The waiting-focus regression now checks the actual attention identity. |
| Remove `Scene::footer_id`, `Replica::new_authority`, and legacy `Companion::drain_input` | No callers in tracked sources, tests, tools or FFI. Verified-frame input consumption remains intact. |
| Remove the obsolete `--monologues` screenshot mode | It no longer exercised a UI feature; current previews cover selection and attention instead. |
| Use fixed seat arrays during animation ticks | Replace per-tick ID cloning, sorting and temporary blocked-seat lists. Preserve seat order, active/idle swaps, walking and departed-patron handling. |
| Cache task byte offsets alongside existing display pages | Stop laying out the full task on every anchor read/write. Refresh both representations when task text changes; remove both when sessions/details disappear. |
| Use binary search for ordered text anchors | Preserve the first line when escaping one Unicode/control character creates duplicate offsets. Regression tests compare against the original linear algorithm across boundary cases. |
| Borrow IDs during overflow sorting | Keep the same priority/arrival/ID ordering without allocating sort keys. |
| Move the committed model while rebuilding desktop state | Avoid cloning the entire model every synchronization. The store mutex protects the replacement; failed validation restores the committed model. Added a rejection regression. |
| Traverse attention keys directly | NEXT/wrapping behavior is unchanged; render-time temporary key vectors are gone. |

## Measurements

`examples/ui_performance.rs` measures synthetic fixtures without process discovery, credentials, devices or application interaction. Release builds use an allocation-counting system allocator. Times are local microbenchmarks and vary with machine load; the allocation reductions are the more reliable comparison. This does not establish a whole-app CPU/FPS improvement.

| Operation | Before allocations/op | After allocations/op |
| --- | ---: | ---: |
| Reconcile eight unchanged sessions | 43 | 25 |
| Animation tick, eight patrons | 11 | <0.01 (occasional particle capacity growth) |
| Read/restore task anchor, 30,000-byte mixed-text fixture | 12,022 | 0 |
| Render café at either scale, human or mixed fixture | 9 | 7 |
| Desktop model synchronization, eight sessions | 227 | 163 |
| Desktop model synchronization, 64 sessions | 1,735 | 1,202 |

The task-anchor round trip went from approximately 448 µs to 23 ns in the native profile. Baseline output called this case “32k”; its actual text length is 30,000 bytes, and the maintained example now labels it “30k”.

The real Xtensa engine was built and profiled in Espressif QEMU with short text and exact 32,768-byte ASCII, tabs, NULs and newlines. Repeated review renders retained identical free-heap/largest-block measurements within each case, and all stayed under the existing 50 ms draw budget. The five before/after framebuffer captures were byte-identical.

Caching offsets trades retained memory for lower temporary allocation peaks. In the newline-heavy case, retained heap increased by 135,132 bytes, but minimum free heap improved from 4,656,460 to 5,569,908 bytes (913,448 bytes). For ASCII/tabs/NULs, retained increases were 4,060/8,156/16,328 bytes. These simulator figures include the profile's cache history and are not physical-device telemetry.

## Retained deliberately

- Legacy framebuffer support, protocol/schema fields, engine C exports and wiring diagnostics still serve explicit rollback, compatibility or hardware/simulator paths. They are not dead merely because the normal Mac-first route bypasses them.
- Collector liveness checks, process-start validation, bounded transcript readers, attribution rules, verified request text, decision receipts and authentication limits remain unchanged. Broader caching here could hide process reuse, stale requests or account changes.
- LCD orientation restoration, SPI timing, input debounce, backlight and transport timeout settings remain unchanged. Simulator timings cannot establish a safe hardware timing change.
- The shared renderer still repaints animated scenes; no speculative frame-skipping or mutable-request reuse was introduced.

## Validation and deployment

Validation commands:

```sh
cargo test --workspace --locked
cargo test --workspace --locked --no-default-features
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
bash tools/test_keypad.sh
cargo run --release --locked --example ui_performance
bash tools/qemu_v2.sh build
bash tools/qemu_v2.sh test
bash tools/qemu_v2.sh board-build
sh tools/build_macos.sh
```

Passed: 231 workspace Rust tests, 196 tests without default features, strict Clippy and formatting, 50 native keypad checks, 16 native recording checks, sanitized OLED parsing, the complete QEMU suite, the physical semantic board build, and the signed macOS bundle build. The six existing native board C tests also run under AddressSanitizer/UndefinedBehaviorSanitizer. The physical board image fits its app partition with 37% free.

All 15 existing top-level documentation PNG/GIF files were byte-identical after regeneration with the optimized renderer. No visual assets were changed.

The macOS bundle is rebuilt at the stable release path. This pass does not flash devices or restart the running app. Changes to renderer source change the compatibility digest: deploy the matching semantic board firmware and desktop app together before restarting into this build. Keypad firmware is unchanged. Physical LCD/touch/Bluetooth soak testing remains separate from simulator/build results.

Local evidence (ignored): `target/cleanup-{tests,no-default-tests,clippy,native,board-native,macos-build,qemu-tests,board-build}.log`, `target/cleanup-ui-{before,after}.txt`, `target/cleanup-board-{before,after}.json`, and `target/cleanup-board-before-frames/`.
