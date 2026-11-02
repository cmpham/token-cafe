# Patron detail at 3× — 2026-11-02

The later [shared framebuffer change](shared-framebuffer-2026-11-02.md) supersedes the separate desktop HD path described here; the improved native board artwork is retained.

This records the initial desktop-only change and first reload. The later
[board detail follow-up](board-patron-detail-2026-11-02.md) supersedes its unchanged-board-rendering result.

Desktop patrons now use 144×144 RGBA animation cells, recompiled from the source
art at 3× rather than enlarging the previous 96×96 cells. All 448 frames retain
their 48×48 logical footprint and animation/seat anchors. The desktop renders
1440×816 internally; furniture, text, input coordinates and window shortcuts keep
their existing logical grid. Portraits sample the same new atlas.

The shared renderer accepts native, 2× and 3× buffers. Native board rendering
continues to use its compressed 48×48 cells and 480×272 framebuffer. The HD atlas
remains excluded from no_std firmware. Its desktop payload grows from 15.75 MiB
to 35.4375 MiB; the two desktop frame buffers together grow by about 5 MiB.

Validation on the development Mac:

- 241 workspace tests and 198 tests without default features passed.
- Strict all-target workspace Clippy and formatting passed.
- Four Python asset tests passed, including detail/anchor/palette checks.
- The shared renderer compiled without default features (no_std).
- Added coverage for full-atlas sampling and clipped portraits, background grid
  preservation, and 3× damage/presentation. Pixel-oracle presentation checks cover
  window scales 1–7, small windows, clipping and buffer age.
- All 19 native board frame/sequence hashes matched the 2× baseline, including
  4,800 animated frames across four rosters and four review/café fixtures.
- Release profiling measured mixed-roster 3× rendering at approximately 0.74 ms
  per frame, versus 0.37 ms for the prior 2× renderer, within the 33 ms interval.
  Render allocation counts stayed at seven per frame. These are local software
  measurements, not physical board timings.
- Rendered every creature's seated actions, arrivals and departures. Inspected
  dragon/Pokémon wave/sip contact sheets and regenerated roster/focus PNGs and
  representative GIF frames. Media uses fictional sessions.
- Built the release desktop executable.

Deployment was completed after the subsequent board-reload request. The full
QEMU suite passed (connection instructions, patrons, attention, protocol at ATT
capacities 182/244/509, transitions, Codex/Claude resolution and performance).
The matching production board application is 2,616,864 bytes and fits the 4 MiB
layout. Its renderer digest matches the rebuilt desktop app.

The identified ESP32-S3 board (USB serial `44:BD:8D:7A:63:54`) was backed up in full
before application-only flashing at `0x10000`. Esptool verified the flashed hash;
readback confirmed the new application metadata and byte-identical first 64 KiB,
preserving the bootloader, partition table and pairing NVS. Backups and deployment
logs remain private under ignored `target/deploy-patron-3x/`.

The signed stable macOS app was rebuilt and restarted. Its window title confirmed
`Board: TokenCafe-6336 synced v2` and `Keypad: USB + OLED ready`. Keypad firmware and
all three native helper binaries are unchanged. Physical panel appearance, real
touch interaction and hardware performance were not independently checked.
