# Patron visual QA — 2026-10-27

Reviewed the actual rendered pixels, not just test results:

- 192 seated views: 16 characters × six actions × both animation phases, arranged in six labeled review boards.
- 48 sampled arrival/departure views across the two rosters, including front, back and side gait, aisle travel, furniture occlusion, chair arrival, and exit.
- The rebuilt macOS binary in an isolated demo app with synthetic session names: dragon and Pokémon rosters at 3×, then Pokémon at the minimum 1× window scale. Inspected native screenshots at successive phases and exercised roster switching/resizing.

## Issues found and repaired

1. Pokémon coffee cups used a fixed screen offset, leaving several cups beside the eye/cheek or detached from the turning head. The asset compiler now extracts the PMD per-frame head markers from Eat-Offsets.png. Cups follow those anchors and render after the laptop. Both phases were visually rechecked for every affected character.
2. Starflight and Moonwatcher lost body/wing details against the dark floor. A modest renderer fill light lifts the purple scale colors while retaining black outlines. Rechecked the scene and native desktop screenshots.

## Final observations

- Seated body positions remain on the chair/desk contact line. Typing forepaws are visible beside the laptop; desk fronts and nameplates remain unobstructed.
- Thinking, waving, sipping, sleeping and stretching visibly change the character itself. Sleep uses curled/slumped/resting art instead of merely adding Z particles to the work pose.
- Character portraits correspond to the waiting patrons. Both question and permission indicators remain readable.
- Walkers face the travel direction and pass behind desks as expected. Generated dragon side walks use mirrored three-quarter views; PMD Pokémon have directional source art.
- Scene contents remain unclipped at the minimum window size. macOS abbreviates the long shortcut-help window title at that width; the selected seat/character stays visible, and full help is available with --help / the README.
- The animation style remains deliberately low-frame-count pixel animation. This review used paired frames and sampled timeline images, not an assertion that every millisecond of every animation was watched.

## Evidence

- [Dragon typing/thinking](dragons-type-think-board.png), [waving/sipping](dragons-wave-sip-board.png), [sleeping/stretching](dragons-sleep-stretch-board.png).
- [Pokémon typing/thinking](pokemon-type-think-board.png), [waving/sipping](pokemon-wave-sip-board.png), [sleeping/stretching](pokemon-sleep-stretch-board.png).
- [Dragon arrivals](dragons-arrival-board.png), [departures](dragons-departure-board.png); [Pokémon arrivals](pokemon-arrival-board.png), [departures](pokemon-departure-board.png).
- [Native dragon window](desktop-dragons.png), [native Pokémon window](desktop-pokemon.png), [minimum-size window](desktop-minimum-size.png).
- Updated animated scene previews: [dragons](../cafe-dragons.gif), [Pokémon](../cafe-pokemon.gif), [mixed](../cafe-mixed.gif).

## Reproduce

```
python3 tools/build_patrons.py
cargo run --locked --example patron_qa
python3 tools/qa_patron_boards.py
cargo run --locked --example screenshots -- --patrons
```

The QA renderer writes source frames and boards under target/patron-qa. Selected boards are copied here for review. Pillow is required for the Python scripts.

Validation after fixes: all 20 Rust tests passed; strict all-target Clippy passed; formatting and diff checks passed. The macOS release app built and ran. The local toolchain emitted its existing debug-symbol stripping warning about libLLVM; it did not prevent the build or native visual check. Linux and physical hardware were not exercised in this visual QA.

## High-resolution patron follow-up

Desktop now renders 960×720 with 96×96 patron animation cells (3× each dimension). Room geometry, furniture and text remain on the original 320×240 logical grid. Full-resolution authored sources are compiled directly; classic PMD Pokémon preserve native pixel detail and are not newly redrawn artwork.

Visually reviewed six `hd-*-board.png` boards: all 16 characters, six seated actions, two phases (192 views). Faces and horns are clearer; seated contact, foreground hands, cups, sleeping poses and raised wings remain aligned. Also inspected full-resolution dragon arrivals and Pokémon departures, and native desktop screenshots of both rosters at default size plus the minimum Pokémon window. No new clipping or alignment issue observed. Minimum title still truncates shortcut help, as before.

Validation: 21 tests pass, including high-detail dirty-region repaint parity at window scales 1–6 and exact native-resolution presentation; strict Clippy passes. macOS app rebuilt and launched successfully. Build retains the local toolchain's nonfatal rust-objcopy/libLLVM debug-stripping warning.

## 2× detail trial

Changed the current desktop detail layer to 2×: 64×64 patron cells and a 640×480 framebuffer. Window sizes and room coordinates are unchanged. Rebuilt from source art. Visually inspected the new dragon and Pokémon wave/sip boards (all 16 patrons, both phases), including facial detail, cup placement and desk overlap. These are saved as `2x-*-wave-sip-board.png`; earlier `hd-*` screenshots document the previous 3× version. All 21 tests and strict Clippy pass, including repaint parity across window scales 1–6. macOS app rebuilt; the existing nonfatal debug-stripping warning remains.

## Laptop logo occlusion repair

Reproduced the reported cracks: the creature contact-strip pass overwrote laptop lid pixels. Moved that pass before laptop rendering; held cups and keyboard paws retain their foreground pass. A regression compares the entire laptop lid against isolated laptop rendering for all 16 creatures, four timestamps and both render resolutions; it failed before the fix and now passes. All 22 tests and strict Clippy pass. Rebuilt the macOS app and regenerated the 2× previews; visually reviewed both rosters’ wave/sip boards for intact logos and preserved cups.

## Fork integration

Integrated the complete cmpham branch through 6b43472. Rendering now lives in crates/cafe-ui with 480×272 logical coordinates; desktop 2× output is 960×544. Rebuilt full-source patron assets remain 64×64. P changes rosters, C selects a character seat, brackets change its character; Tab retains session focus navigation. Creature portraits are used in the footer and review, and the inherited animated Codex logo is retained. Semantic firmware keeps its native human roster; desktop-only patron assets are excluded from no_std builds. Legacy mirrored frames retain their native dimensions and fingerprint buffer. The renderer digest changed, so semantic desktop/board versions must be updated together. No physical device validation was performed for this integration.
