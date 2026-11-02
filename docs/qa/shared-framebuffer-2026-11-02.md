# One native framebuffer — 2026-11-02

Desktop and board now render through the same native 480×272 software renderer,
using the same 48×48 patron animation cells, 32×32 portraits and alpha compositing.
The desktop only enlarges the finished frame when presenting it in a window.
Window shortcuts, Retina/DPI sizing, letterboxing and input coordinates retain
their existing behavior. The physical board still receives RGB565 output; its
native u32 scene framebuffer uses the same format as the desktop renderer.

Removed the desktop HD atlas and its platform-specific selection, scale-dependent
primitives/background code, high-detail framebuffer detection, sampling branches,
and obsolete multi-resolution tests. Gfx now requires exactly 480×272 pixels.
The separate legacy mirroring framebuffer and its second scene render are gone;
legacy publishing reuses the desktop frame. Review verification takes its immutable
visual fingerprint from the pre-input frame, so a batch cannot substitute a later
render as the display baseline. The semantic protocol still synchronizes state
and the board still renders locally; no framebuffer streaming was added.

The improved native artwork is retained byte for byte. Supersampling at 3× happens
only during asset generation, then resolves into shared native assets; the HD
intermediate is neither emitted nor embedded. `portraits.rgba` replaces the former
board-specific name because both platforms use it. Original source art and credits
remain intact. Runtime assets and source changes remain covered by the existing
compatibility digest; app and board therefore require a paired reload.

## Validation

- macOS: 242 workspace/all-target tests and 199 without default features passed;
  strict workspace/all-target Clippy and formatting passed.
- Linux ARM64: 239 workspace/all-target tests and 195 without default features,
  plus strict Clippy, passed in the existing Rust 1.88 Debian container. The final
  desktop-input change was rechecked there with tests and lint. Runs were offline,
  with read-only source and independent build caches; no host device access. The
  actual Linux executable also passed an isolated Xvfb startup smoke test.
- The shared renderer compiled without std. Six Python asset tests passed, including
  every native animation/portrait frame regenerated from source and fractional
  coverage checks. All three native artwork/anchor payloads match the prior board.
- Native pixel oracles cover alpha and rectangle clipping, damage, window scales
  1–7, small windows, partial painting and buffer age. A contract regression rejects
  an oversized scene framebuffer.
- All 19 native frame/animation hashes match the previous board renderer, covering
  4,800 animated frames plus static review fixtures. Actual Xtensa seated captures
  for dragons, Pokémon and mixed rosters also match the prior version pixel for pixel.
- Complete QEMU suite passed: connection hint, roster changes/walking/touch identity,
  attention, protocol at ATT capacities 182/244/509, transitions, provider resolution
  and long-text performance. All-roster render samples retain stable free/largest
  heap; maximum draws were Pokémon 1,487 µs, dragons 1,645 µs, mixed 1,484 µs and
  humans 917 µs, below the 50 ms simulator budget. These are not physical timings.
- Regenerated roster/focus screenshots and GIFs; inspected representative frames.
  PNG checks confirm every preview is an integer enlargement of native 480×272
  pixels. Media contains fictional sessions.

## Resource changes

The default Bluetooth desktop's two scene/history frames shrink from 1440×816 to
480×272, and its extra native mirroring frame is removed. Those buffers together
use 8,878,080 fewer bytes (about 8.47 MiB); window/compositor buffers are unaffected.
The locally signed executable shrank from 45,803,088 to 8,538,400 bytes. The generated
35.44 MiB HD payload is no longer embedded or kept as a runtime artifact.

The local release fixture renders mixed patrons in approximately 0.10 ms per frame
versus 0.74 ms for the earlier 3× path, with unchanged render allocation counts.
These are diagnostic host measurements, not a physical-board or battery-life claim.
The production board application is 3,036,544 bytes, below its existing 0x3f0000-byte
partition. The board's appearance stays unchanged; desktop enlargement now shows
that same native pixel detail rather than the former independent HD detail.

## Deployment

The stable macOS bundle was rebuilt after preserving its previous copy. The board
was identified as USB serial `44:BD:8D:7A:63:54`; a private full-flash backup and
simulator/deployment evidence are under ignored `target/deploy-shared-framebuffer/`.
Only the application at `0x10000` was flashed. Esptool verified its hash; readback
confirmed the application metadata and byte-identical first 64 KiB, preserving
bootloader, partitions and pairing NVS. The restarted stable app reported
`Board: TokenCafe-6336 synced v2` and `Keypad: USB + OLED ready`. Keypad firmware and
all three native helpers are unchanged. Final status is recorded in the private manifest.
Physical panel appearance and real finger interaction were not independently tested.
