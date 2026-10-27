# Board performance and memory review — October 14, 2026

Scope: the semantic ESP32-S3 firmware, shared Rust renderer/client, C input/LCD
loop, and Bluetooth transport. Earlier uncommitted screen/transition fixes were
retained. Measurements compare against that starting point, not Git HEAD.

## Findings and changes

| Path | Finding | Change |
| --- | --- | --- |
| Scene background | Each scene allocated 522,240 bytes for immutable pixels; reconnect also briefly constructed a second copy. | Encode into compact RGB24/run-length words in mapped flash. The scene no longer owns a heap background. |
| Request rendering | Every frame escaped and wrapped the entire request, allocating a temporary string per character and every output line. | Cache layout when the immutable request is constructed; reuse it across unchanged client synchronizations. Private request access prevents stale approval text. |
| Escaped text | Ordinary ASCII, tabs, backslashes and Unicode escapes allocated temporary strings. | Emit cells directly, with reference-equivalence tests for literal text and original byte offsets. |
| Review frame | Nested filled rectangles repainted nearly the entire PSRAM framebuffer five times. | Draw borders and interior once; before/after framebuffer comparison is exact. |
| Client details | Synchronization cloned and laid out cached text for every session, including invisible patrons. | Materialize only the selected patron's details. The verified blob cache retains other bodies for later selection. |
| Receipt journal | Pending receipts were cloned on each engine tick even when unchanged. | Compare before copying; build a replacement journal only when a receipt can be retired. Persistence failure still retains receipts. |
| Blob cache | Incoming text grew its allocation repeatedly; pinning cloned an unchanged hash. | Reserve the validated, bounded body length once and only replace a changed pin. |

## Evidence

`cargo run --release --example board_performance` records allocator counts, live
scene bytes, transient render allocations, CPU timing, and framebuffer hashes.
Native measurements are profiling evidence, not ESP32 latency claims.

| Native workload | Before | After |
| --- | ---: | ---: |
| Scene heap allocation | 522,256 B | 16 B |
| Short review allocations/frame | 40 | 0 |
| 32 KiB ASCII allocations/frame | 35,229 | 2 |
| 32 KiB escaped-controls allocations/frame | 37,658 | 2 |
| 32 KiB ASCII transient render heap peak | 86,232 B | 16 B |

The unchanged framebuffer hashes are `12b79e433644c49a` (short),
`a2fec7a05441f9f2` (ASCII), and `3e73335d566cd516` (controls).

QEMU uses the actual Xtensa engine with a profiling-only adapter operation. Each
case draws twelve identical review frames; retained free heap and largest free
block must remain stable. Rendering must stay below a generous 50 ms emulator
budget. The standard `tools/qemu_v2.sh test` now includes these cases.

| QEMU workload | Median draw before | Median draw after | Additional free heap |
| --- | ---: | ---: | ---: |
| Short review | 1,392.5 µs | 505.0 µs | 525,060 B |
| 32 KiB ASCII | 120,447.0 µs | 415.0 µs | 997,552 B |
| 32 KiB controls | 130,470.0 µs | 378.0 µs | 1,240,952 B |

QEMU screenshots are byte-identical for all three cases. Profiles and the preserved baseline engine are in ignored
`target/board-performance/`; the production image is in the private backup directory. QEMU is not a
cycle-accurate substitute for physical PSRAM, SPI, Bluetooth, or touch timing.

## Memory tradeoffs and retained constraints

The background's compact run encoding saves 510 KiB RAM per scene. The final
production image shrank from 1,342,512 to 1,027,216 bytes (315,296 bytes saved). The generated representation is included in the compatibility digest;
desktop and board must still run matching builds. Both physical and QEMU partition
layouts remain unchanged. Cached review lines consume bounded resident memory in exchange for
eliminating repeated large transient allocations. Long-body profiles measure the
net effect after those caches are populated.

The canonical RGB888 render buffer, RGB565 output, and previous-panel framebuffer
remain. Removing those would require a broader renderer/presentation redesign.
RGB565 packing and SPI presentation remain measurable costs. Atomic model staging,
full-text/hash verification, bounded protocol queues, and durable approval receipts
remain in place. LCD polarity, orientation, chip-select handling, and touch mapping
were not changed by these optimizations.

## Validation coverage

116 native workspace tests pass, including reference-equivalent escaping and byte
anchors, every decoded background pixel, cached review navigation, selected-patron
text isolation, and existing approval/persistence/gesture tests. Clippy passes with
warnings denied. The complete Xtensa QEMU suite passes: all three ATT capacities,
authentication and replay checks, 64-session behavior, local selection/review,
delayed desktop handoff, scroll, sleep/wake, and the maximum-text profiles, with additional NUL and newline-only cache stress cases.

## Final physical-board results

The final compressed-background image passed the normal 30-selection suite and
100-selection stress suite with a 32 KiB escaped body. Both runs checked eight
seats, scrolling, guarded synthetic approval, both provider handoff/reopen/back
cycles, and framebuffer captures. Actual provider applications are not launched
by these isolated fixtures.

| Physical workload | Press median / p95 / max | Release-to-LCD median / p95 / max |
| --- | --- | --- |
| Normal, 58 samples | 34.5 / 64 / 69 ms | 80 / 98 / 98 ms |
| 32 KiB stress, 128 samples | 32.5 / 55 / 70 ms | 185 / 188 / 225 ms |

Café draw samples are 29–30 ms, packing 15–17 ms. Normal free PSRAM is about
7.06 MB versus about 6.52 MB in the starting fixture. During the final stress
selection loop, free heap stayed around 6.91 MB and the largest free block stayed
at 6,684,672 bytes. Minimum free heap was 6,652,792 bytes. These are finite-run
measurements, not a claim of indefinite leak freedom.

Additional QEMU edge cases passed for 32 KiB NUL and newline-only bodies. The
newline case retained 6,231,380 bytes free, with a 4,657,384-byte low watermark;
repeated drawing did not reduce free heap or the largest block.

Evidence: ignored `target/performance-final-board-qa/`,
`target/performance-final-board-stress/`, and
`target/board-performance/qemu-edge-cases.json`.

Production restoration: the final application image passed esptool hash verification;
the original pairing header was restored byte-for-byte. The rebuilt companion is
running with its production registry. Live serial telemetry confirmed draw 29 ms,
pack 15 ms, LCD 24 ms and 7,322,268 bytes free PSRAM for the user's current sessions.
No production app errors were logged during the post-restoration check.
