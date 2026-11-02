# Board patrons from 3× artwork — 2026-11-02

The later [shared framebuffer change](shared-framebuffer-2026-11-02.md) supersedes the separate desktop HD path described here; the improved native board artwork is retained.

The user chose to keep patron size and improve detail. This supersedes the board
rendering described in the initial [desktop-only 3× change](patron-resolution-3x-2026-11-02.md).
The LCD still outputs 480×272 pixels. This change improves how the 3× source
art resolves into that fixed pixel grid; it does not increase physical LCD resolution.

The board previously used independently nearest-sampled 48×48 animation cells
and discarded alpha below 128. It now uses native cells prefiltered from the
visible 144×144 desktop artwork, retaining fractional pixel coverage. The renderer
composites those pixels over the actual room with integer alpha blending. Faces,
small features and contours receive the benefit of 3× source sampling without
larger bodies or a larger framebuffer. Native 32×32 portraits are filtered directly
from the 3× artwork, avoiding a second nearest-neighbor reduction of the room cells.

The source alpha cutoff matches desktop visibility before area filtering. Alpha
premultiplication prevents hidden RGB in transparent pixels from contaminating
edges. NightWing fill light, foreground paws/cups, scene clipping, seat geometry,
walking directions, frame timing, baseline and cup anchors remain in place. The
cup-anchor file is byte-identical to the prior revision. No generated art or
newly invented sprite detail was introduced.

Compression preserves every nonzero-alpha pixel. The board stores 1,808,230 bytes
of pose runs plus 86,016 bytes of row offsets, and 73,345 bytes of portrait runs
plus 4,096 bytes of offsets. Decoding uses one 192-byte row and no new per-frame
heap allocation. The full 35.44 MiB HD atlas remains excluded from no_std firmware.
The production application is 3,044,512 bytes, leaving 1,084,256 bytes free in its
existing app partition. Bootloader and partition layout are unchanged.

## Validation

- 243 workspace tests, 200 without default features, strict all-target Clippy,
  formatting and the no_std renderer build passed.
- Six Python asset tests passed, including every one of the 448 board animation
  frames and 32 portraits matching the 3× conversion, and a known fractional
  coverage/transparent-color case.
- Rust checks cover all alpha values, clipped compositing at 1×/2×/3×, exact
  compressed alpha/color decoding, and both directly filtered portrait phases.
- All desktop 2×/3× fixture hashes remain identical. Human and review-only native
  hashes also match. Only the 12 creature/native animated fixture sequences changed,
  as intended. Native rendering allocation counts did not increase.
- Reviewed all native creature wave/sip poses and before/after captures from the
  actual Xtensa board engine. The [comparison](board-patron-detail-comparison.png)
  uses fictional sessions and native 480×272 captures, not physical LCD photographs.
- The complete QEMU suite passed: connection hint, patrons/roster swaps/walking,
  attention, protocol at ATT capacities 182/244/509, review transitions, Codex and
  Claude resolution, and long-text performance.
- Added an all-roster QEMU performance check. Six repeated renders per roster keep
  free/largest heap unchanged and stay below a 50 ms budget. Maximum sampled draw
  times: Pokémon 2,038 µs, dragons 2,019 µs, mixed 1,659 µs, humans 1,217 µs.
  These are simulator measurements, not physical LCD or Bluetooth latency.

## Deployment

The stable macOS bundle was rebuilt and the ESP32-S3 board's full flash backed up
privately under ignored `target/deploy-board-detail/`. The identified board has USB
serial `44:BD:8D:7A:63:54`. Only the application at `0x10000` was flashed. Esptool
verified its hash; readback confirmed matching new application metadata and an
unchanged first 64 KiB, including bootloader, partition table and pairing NVS.

The matching app was restarted at `/Users/cuong/games/token-cafe/release/Token Café.app`.
Its window title confirmed `Board: TokenCafe-6336 synced v2` and `Keypad: USB + OLED ready`; the same status is recorded in the private deployment manifest.
Keypad firmware is unchanged. Physical LCD appearance and finger-to-patron touch
alignment still require direct user inspection; simulator captures do not prove them.
