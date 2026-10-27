# Patron artwork

Collected/prepared 2026-10-27. Runtime assets are embedded; the app never downloads art.

## Runtime animation assets

`poses.rgba` contains 28 frames per character: six two-frame seated actions followed by four four-frame directional walks. The renderer adds articulated keyboard forepaws and, for PMD eating frames, a coffee cup anchored to the source head markers in `cup-anchors.bin`. NightWing colors receive a modest runtime fill light for contrast against the café floor. Source images are resampled with nearest-neighbor filtering and aligned to a fixed baseline at one scale per character.

- `source/poses/{name}.png`: newly generated fan-art pose sheets for the eight Wings of Fire dragons and Dragonite, made with built-in imagegen. [Exact prompt](source/poses/PROMPT.md). Front three-quarter gait is mirrored for left travel; dedicated rear frames cover upward travel.
- `source/pmd/{name}/`: collected [PMDCollab/SpriteCollab](https://github.com/PMDCollab/SpriteCollab) animation sheets for Pikachu, Eevee, Bulbasaur, Charmander, Squirtle, Jigglypuff and Gengar. Paths: `sprite/{four-digit-Pokedex-number}/`. XML frame sizes are honored; the selected sequences are Nod, Pose, Eat, Sleep, DeepBreath and Walk. Four directional walk rows are retained. Rear-only Sit sheets are retained as source references but are not used at the front-facing café desks.

PMD credits: **CHUNSOFT** for original animation art; **Emboarger** for Gengar's additional action sequences; **JFain** for Jigglypuff's additional action sequences. Original per-character `credits.txt`, contributor names, and the upstream README are preserved in `source/pmd/`. The project documents non-commercial, credited reuse for contributed material; official sprites retain their owners' rights. Gengar's contribution records `PMDCollab_1`, Jigglypuff's records `CC_BY-NC_4`; consult those source credits and upstream policy before redistribution. No commercial clearance is claimed.

Changes to PMD material: selected animation frames/directions, nearest-neighbor scaling, baseline alignment, desktop occlusion and supplemental keyboard paws/cup. Unused downloaded actions remain reference-only; Dragonite's PMD walk is used alongside generated seated art.

## Original static Pokémon collection (reference only)

Eight original PNGs in `source/`: Pikachu (25), Eevee (133), Bulbasaur (1), Charmander (4), Squirtle (7), Jigglypuff (39), Gengar (94), Dragonite (149).

Source: [PokéAPI sprites](https://github.com/PokeAPI/sprites), path `sprites/pokemon/versions/generation-v/black-white/{number}.png` on the master branch. These are static front sprites, not animation sheets. The downloaded license is preserved in `source/POKEAPI-LICENCE.txt`; it explicitly attributes image copyright to The Pokémon Company. Repository licensing does not transfer ownership of Pokémon characters.

## Wings of Fire dragons

`source/dragons.png` is a newly generated fan-art atlas, made with the built-in imagegen tool, not downloaded art by a fan artist or official book art. It depicts interpretations of Clay, Tsunami, Glory, Starflight, Sunny, Peril, Winter and Moonwatcher. Wings of Fire characters are associated with Tui T. Sutherland / Scholastic.

Research/reference collection (not bundled or copied):

- [Pyrrhian tribe pixel sprites, commissioned fan art](https://www.reddit.com/r/WingsOfFire/comments/19ay7fw): relevant pixel-art reference; no reuse permission established.
- [Pixel icon animations](https://www.reddit.com/r/WingsOfFire/comments/1mwmq52): animation reference; no reuse permission established.
- [DragonFox animation collection](https://boosty.to/dragonfox): creator advertises downloadable Wings of Fire GIFs; no assets downloaded from this collection.
- [Scholastic Wings of Fire game tokens](https://www.scholastic.com/pdfs/WoF-GameTokensInstructions.pdf): official character-art reference, not pixel sprites.

## Rebuild

Run `python3 tools/build_patrons.py` with Pillow installed. It identifies transparent row gutters in generated atlases, uses the XML cell dimensions for PMD sheets, and writes `poses.rgba`, `docs/patron-poses.png` (all frames) and `docs/patrons.png` (roster). Per-character scale stays fixed across poses so sleeping bodies are not inflated to standing height. The renderer uses a hard alpha threshold for crisp pixels.

Run `cargo run --example screenshots -- --patrons` for three café PNGs and three 4.8-second animated GIFs from the real scene renderer.

## Original static atlas prompt (reference only)

Create one game sprite atlas on a genuinely transparent background: eight distinct cute Wings of Fire inspired dragon patrons in a strict 4 column by 2 row equal-cell grid. No text, no labels, no scenery, no shadows, no grid lines. Each full-body dragon centered completely inside its cell with generous transparent margins, same size and baseline. Tiny chunky 32x32-style pixel art, hard square pixels, limited palette, dark outlines, simple highly readable silhouettes suitable for a 320x240 JRPG cafe game. Seated upright, facing slightly right/front, visible head, wings and curled tail. In reading order: Clay the earthy brown MudWing with broad snout; Tsunami the teal SeaWing with aqua fins and glowing spots; Glory the emerald RainWing with rainbow frill; Starflight the dark purple NightWing with star-speckled wings; Sunny the golden SandWing with pointed tail; Peril the scarlet-orange SkyWing with large wings; Winter the icy pale blue IceWing with crown spikes; Moonwatcher the midnight violet NightWing with silver teardrop scales. Make each silhouette distinct, compact chibi proportions, expressive friendly eyes. All eight clearly separate, nothing between cells.

Dragonite uses its bundled PMDCollab four-direction walking sheet for an unambiguous heading; seated poses retain the generated artwork. PMD horizontal rows are right=2 and left=6. Room cells are 48×48 logical / 96×96 at the desktop 2× detail layer, while portraits remain 32×32 logical.

The seven classic PMD Pokémon use an edge-aware Scale2x resampling pass for the desktop HD cells, preserving the source palette. This refines existing contours without inventing new art. Room size, source normalization, cup anchors, and directional frame selection are unchanged; generated dragon/Dragonite seated art retains direct source sampling.
