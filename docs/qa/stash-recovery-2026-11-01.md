# Selective optimization recovery — November 1, 2026

Reviewed the saved root changes in stash `ced8b2f` against `master` at
`5bbc9a9`. Recovered only relevant non-Android work, preserving the current
creature renderer, roster synchronization, approval guards and Bluetooth cleanup.
The stash remains available; Android stays on `codex/android-companion`.

## Adoption decisions

| Area | Decision and adjustments |
| --- | --- |
| Usage parsing | Adopted reusable line storage and propagation of Codex transcript read errors. Previously, `map_while(Result::ok)` could silently cache partial totals. Added parser-level failure injection and bumped the cache version from 2 to 3 so old partial entries are recalculated. Claude uses the same reader while retaining account attribution and streamed-response deduplication. |
| Incremental transcript collection | Adopted buffer reuse between records; existing bounds and incomplete-record handling remain. |
| Desktop frame presentation | Ported bulk unchanged-row comparison and allocation-free painting. Reworked the original 1× implementation for current 1×/2× source frames, odd/even window scales, downsampling, clipped windows and partial repainting. |
| Shared graphics | Adopted contiguous clipped rectangle fills at both detail levels. Independent pixel-oracle tests cover empty/inverted/off-screen clips and rectangle edges. |
| Scene labels and approval previews | Adopted cached nameplates, formatted usage labels and bounded preview layout. Corrected the stash's double escaping, premature 32-column wrapping and replacement of the contextual empty-request fallback. Cache the original 60-column first line and render it without escaping again. |
| Desktop/board state | Adopted borrowing of scene inputs, reuse of identical blob storage, one fewer session-map clone, and copying review bodies only when their identity/content changes. Preserved roster fields and 30-second blob retention. |
| Bluetooth polling | Adopted revision checking before cloning the model. Discovery, connection timeouts, cancellation cleanup and reconnect tests are unchanged. |
| Encrypted fragments | Adopted stack-allocated associated data, in-place AEAD and bounded reassembly reservation. Added byte-for-byte comparisons against the previous allocating AEAD API across all 477 payload lengths in both directions. Existing independent vectors and corruption/replay checks remain. |
| LCD adapter | Adopted row-and-column damage bounds and shared RGB565 staging. Only transmitted pixels update the previous-frame buffer. Added the portable sanitizer regression suite to CI. |
| Benchmarks | Recovered transport allocation profiling and expanded scene profiling to all four rosters at both detail levels, including escaped/empty/maximum-size previews. |

## Intentionally excluded

- Android app, bridge, build scripts, CI and Cargo workspace/dependency changes:
  already isolated on their own branch and explicitly outside this task.
- Claude account discovery, iTerm navigation, usage/account views and borrowing
  provider sessions without approval overlays: already present on `master`.
- Wholesale copies of the older app, scene and synchronization files: these
  would remove newer creature rendering, roster synchronization and BLE fixes.
- The preview cache as originally written and the original 1×-only painter:
  replaced by the corrected adaptations described above.
- The historical October 15 deployment ledger and README changes: they do not
  establish validation of this revision; Android/stale documentation was omitted.

## Validation of the recovered implementation

All following checks passed locally:

- macOS ARM64, Rust 1.99.1: `cargo test --workspace --locked --all-targets`
  (195 tests), plus the same command with `--no-default-features` (174 tests).
- Strict Clippy with and without Bluetooth; workspace formatting and diff checks.
- `cargo check -p companion-core --no-default-features` and actual Xtensa engine
  compilation, covering the shared code without `std`.
- Linux ARM64, Rust 1.88: 191 workspace/all-target tests, strict Clippy, build,
  and five-second Xvfb startup smoke test with synthetic demo data. Source was
  mounted read-only into a disposable container. Two boolean expressions in the
  touched Claude parser were simplified to satisfy that toolchain's Clippy.
- macOS release app bundle build. The production application was not replaced.
- 35 baseline/final render hashes match: three full-review cases and 32 animated
  sequences (four rosters × two detail levels × four preview cases, 300 frames
  per sequence). Independent pixel oracles additionally cover presentation
  scales 1–7, detail levels 1–2, clipping and partial-buffer updates.
- Seven native C ASan/UBSan suites: double tap, touch gestures, screen idle,
  LCD damage, external button/LED input, legacy protocol and one million bounded
  semantic-decoder inputs. The LCD simulation covers random updates, all RGB565
  values, strip boundaries, feedback restoration and legacy staging.
- Four patron-asset tests.
- Full `tools/qemu_v2.sh test`: boot/disconnect instructions, creature rosters,
  attention/sleep transitions, all three ATT capacities (182/244/509), snapshots,
  local navigation/review/scroll, approval guards, replay rejection, reconnect,
  64 sessions, delayed handoff, Codex decisions and desktop resolution.
- Five QEMU long-text/heap profiles passed. Largest observed review draw was
  2.872 ms against the suite's 50 ms limit; repeated renders retained stable heap.
- Production semantic board build: image size `0x285580`, fitting the
  `0x3f0000` app partition with 36% remaining.

The first QEMU attempts exposed stale ignored local build inputs: a 1 MiB app
partition and Python bindings predating `View.patron_ids`. The local sdkconfig
was backed up and aligned with the already-committed partition settings; Python
bindings were regenerated from the current schema. No schema/partition source
change was needed. An initial external-I/O sanitizer invocation lacked its
include path; the corrected invocation passed.

## Measurements and limits

Publication integration: before pushing, the recovery commit was rebased onto
`cmpham/token-cafe` master `52d9815` (Pipar keypad integration). The Bluetooth
merge keeps that revision's one-unacknowledged-model limit and all new keypad
activity/priority handling, while skipping unchanged model clones. Formatting
and equivalent conditional cleanups address pre-existing rustfmt/Clippy issues
in the keypad revision. The combined code passed 215 native tests, 190 without
Bluetooth, strict Clippy in both configurations, a release build, and the native
keypad C/Swift checks (including 50 protocol and 18 desktop-routing assertions).
The QEMU firmware was rebuilt against the new schema and the full simulator
suite passed again, including the five long-text/heap profiles.
The earlier Linux and physical-board build results above describe the recovery
before this rebase; they were not repeated for the keypad integration.

Synthetic native measurements against `5bbc9a9`:

| Workload | Before → after |
| --- | --- |
| Encrypted fragment round trip | 11 → 2 allocations |
| Blob envelope, ATT 182 / 244 / 509 | 76 / 53 / 41 → 28 / 24 / 22 allocations |
| Human-roster ordinary frame, 1× | 58 → 15 allocations |
| Human-roster 32 KiB approval preview, 1× | 4,437 → 14 allocations; transient peak 136,860 → 1,420 bytes |
| Desktop paint | 1 → 0 temporary allocations, including 2× source frames |
| Synthetic 78 MiB / 20,000-record line scan | 20,001 → 2 allocations |
| LCD one-pixel update | 64 → 2 payload bytes |
| LCD 16×8 update within a strip | 1,024 → 256 payload bytes |

Timing improvements vary by scale/workload; one 1×-source/2×-window full-paint
case measured approximately 5% slower despite removing its allocation. These
microbenchmarks do not establish whole-app CPU, battery life or physical latency.
Reusable transcript storage retains the largest line until that file scan ends.

Logs and benchmark sources/results are under ignored `target/stash-recovery-qa/`;
QEMU captures and detailed results are under `target/protocol-v2-qa/`.
No physical flashing, production app launch, real approval dispatch, or new
hardware/BLE timing validation was performed. Shared renderer edits change the
compatibility digest: deploy matching desktop and board builds together through
the existing simulation-before-hardware procedure.
