//! Seat-based appearance configuration, independent of session identity/status.
use crate::gfx::Gfx;
use crate::scene::SEATS;

pub const NAMES: [&str; 17] = [
    "human",
    "clay",
    "tsunami",
    "glory",
    "starflight",
    "sunny",
    "peril",
    "winter",
    "moonwatcher",
    "pikachu",
    "eevee",
    "bulbasaur",
    "charmander",
    "squirtle",
    "jigglypuff",
    "gengar",
    "dragonite",
];
const FRAMES: usize = 28;
const HD_PIXELS: &[u8; 16 * FRAMES * 64 * 64 * 4] = include_bytes!("../../../assets/patrons/poses-hd.rgba");
const PIXELS: &[u8; 16 * FRAMES * 32 * 32 * 4] = include_bytes!("../../../assets/patrons/poses.rgba");

/// Each seated action has actual articulated artwork. Walk clips have four
/// gait frames and use distance rather than time, so a stopped patron stays still.
#[derive(Clone, Copy, Debug)]
pub enum Animation {
    Type,
    Think,
    Wave,
    Sip,
    Sleep,
    Stretch,
    WalkDown,
    WalkUp,
    WalkLeft,
    WalkRight,
}

impl Animation {
    fn frame(self, clock: u64) -> usize {
        let (start, count, period) = match self {
            Self::Type => (0, 2, 180),
            Self::Think => (2, 2, 850),
            Self::Wave => (4, 2, 280),
            Self::Sip => (6, 2, 1100),
            Self::Sleep => (8, 2, 1000),
            Self::Stretch => (10, 2, 650),
            Self::WalkDown => (12, 4, 7),
            Self::WalkUp => (16, 4, 7),
            Self::WalkLeft => (20, 4, 7),
            Self::WalkRight => (24, 4, 7),
        };
        start + (clock / period % count) as usize
    }
}
const CUP_ANCHORS: &[u8; 16 * 4] = include_bytes!("../../../assets/patrons/cup-anchors.bin");
pub const ROSTERS: [&str; 4] = ["humans", "dragons", "pokemon", "mixed"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patrons {
    seats: [usize; SEATS],
}

impl Default for Patrons {
    fn default() -> Self {
        Self { seats: [0; SEATS] }
    }
}

impl Patrons {
    pub fn roster(name: &str) -> Result<Self, String> {
        let seats = match name {
            "humans" => [0; SEATS],
            "dragons" => [1, 2, 3, 4, 5, 6, 7, 8],
            "pokemon" => [9, 10, 11, 12, 13, 14, 15, 16],
            "mixed" => [1, 9, 2, 10, 3, 11, 6, 16],
            _ => return Err(format!("unknown roster {name:?}; choose {}", ROSTERS.join(", "))),
        };
        Ok(Self { seats })
    }

    pub fn name(&self, seat: usize) -> &'static str {
        NAMES[self.seats[seat]]
    }

    /// One-based seat assignment, e.g. 3=glory. Swaps an existing creature.
    pub fn assign(&mut self, spec: &str) -> Result<(), String> {
        let (seat, name) = spec.split_once('=').ok_or("--seat expects NUMBER=CHARACTER (e.g. 1=clay)")?;
        let seat =
            seat.parse::<usize>().ok().filter(|n| (1..=SEATS).contains(n)).ok_or("seat number must be from 1 to 8")?
                - 1;
        let character = NAMES
            .iter()
            .position(|n| *n == name)
            .ok_or_else(|| format!("unknown character {name:?}; use --list-patrons"))?;
        self.select(seat, character);
        Ok(())
    }

    fn select(&mut self, seat: usize, character: usize) {
        if character != 0
            && let Some(other) = self.seats.iter().position(|&n| n == character)
        {
            self.seats.swap(seat, other);
        } else {
            self.seats[seat] = character;
        }
    }

    pub fn cycle(&mut self, seat: usize, backwards: bool) {
        let n = NAMES.len();
        self.select(seat, (self.seats[seat] + if backwards { n - 1 } else { 1 }) % n);
    }

    pub fn is_creature(&self, seat: usize) -> bool {
        self.seats[seat] != 0
    }

    /// Forepaws reach over the desktop, on either side of the laptop lid.
    /// These small articulated parts stay visible even when furniture occludes
    /// the lower source frame; the shoulders and head come from the pose atlas.
    pub fn keyboard_paws(&self, g: &mut Gfx, seat: usize, center: (i32, i32), clock: u64) {
        let id = self.seats[seat];
        if id == 0 {
            return;
        }
        let colors = [
            0x9b6339, 0x2babb6, 0x5dbb47, 0x50436c, 0xeac35a, 0xe86b26, 0xb6def5, 0x4a4865, 0xf8d848, 0xa87840,
            0x70b8a0, 0xf89840, 0x60b8e8, 0xf8a8c8, 0x8060a8, 0xe8a048,
        ];
        let phase = Animation::Type.frame(clock) as i32;
        for (x, lift) in [(center.0 - 10, phase), (center.0 + 9, 1 - phase)] {
            g.rect(x, center.1 - 2 - lift, 4, 4, 0x181021);
            g.rect(x + 1, center.1 - 2 - lift, 2, 3, colors[id - 1]);
            g.px(x + 3, center.1 - lift, colors[id - 1]);
        }
    }

    /// PMD head markers follow the turning face in each Eat frame. Draw the
    /// held cup after furniture so the laptop cannot sever it from the mouth.
    pub fn cup(&self, g: &mut Gfx, seat: usize, feet: (i32, i32), clock: u64) {
        let id = self.seats[seat];
        if !(9..=15).contains(&id) {
            return;
        }
        let offset = (id - 1) * 4 + (Animation::Sip.frame(clock) - 6) * 2;
        let x = feet.0 - 16 + CUP_ANCHORS[offset] as i32 - 2;
        let y = feet.1 - 32 + CUP_ANCHORS[offset + 1] as i32;
        g.rect(x, y, 5, 5, 0x181021);
        g.rect(x + 1, y + 1, 3, 3, 0xf7f3ef);
        g.rect(x + 5, y + 1, 2, 2, 0xf7f3ef);
    }

    /// Anchor on the chair/desk contact line when seated, on feet when walking.
    /// The caller owns the scene clip, allowing hands to overlap the desktop.
    pub fn draw(&self, g: &mut Gfx, seat: usize, feet: (i32, i32), animation: Animation, clock: u64) -> bool {
        let id = self.seats[seat];
        if id == 0 {
            return false;
        }
        let frame = (id - 1) * FRAMES + animation.frame(clock);
        let k = g.scale;
        let side = 32 * k;
        let bytes = side * side * 4;
        let atlas: &[u8] = if k == 2 { HD_PIXELS } else { PIXELS };
        let cell = &atlas[frame * bytes..(frame + 1) * bytes];
        for y in 0..side {
            for x in 0..side {
                let offset = (y * side + x) * 4;
                let p = &cell[offset..offset + 4];
                if p[3] >= 128 {
                    // Soft cafe fill light reveals dark NightWing scales while
                    // preserving black outlines and their purple palette.
                    let lit = (id == 4 || id == 8) && p[..3].iter().copied().max().unwrap() >= 35;
                    let rgb = if lit {
                        [p[0].saturating_add(22), p[1].saturating_add(20), p[2].saturating_add(28)]
                    } else {
                        [p[0], p[1], p[2]]
                    };
                    g.detail_px(
                        (feet.0 - 16) * k as i32 + x as i32,
                        (feet.1 - 32) * k as i32 + y as i32,
                        (rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | rgb[2] as u32,
                    );
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rosters_have_eight_unique_creatures() {
        for name in ["dragons", "pokemon", "mixed"] {
            let p = Patrons::roster(name).unwrap();
            let unique: std::collections::HashSet<_> = p.seats.into_iter().collect();
            assert_eq!(unique.len(), SEATS);
            assert!(!unique.contains(&0));
        }
    }
    #[test]
    fn selecting_an_existing_creature_swaps_seats() {
        let mut p = Patrons::roster("dragons").unwrap();
        p.assign("1=tsunami").unwrap();
        assert_eq!(p.name(0), "tsunami");
        assert_eq!(p.name(1), "clay");
        let before = p.clone();
        for spec in ["0=clay", "9=clay", "x=clay", "1=nope", "clay"] {
            assert!(p.assign(spec).is_err());
            assert_eq!(p, before);
        }
        p.assign("1=human").unwrap();
        p.cycle(0, true);
        assert_eq!(p.name(0), "dragonite");
        p.cycle(0, false);
        assert_eq!(p.name(0), "human");
    }
    #[test]
    fn every_character_has_articulated_seated_clips_and_directional_walks() {
        for character in 0..16 {
            let cell =
                |frame: usize| &PIXELS[(character * FRAMES + frame) * 4096..(character * FRAMES + frame + 1) * 4096];
            for pose in 0..6 {
                let a = cell(pose * 2);
                let b = cell(pose * 2 + 1);
                let changed = a
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(b.as_chunks::<4>().0)
                    .filter(|(p, q)| p != q && (p[3] >= 128 || q[3] >= 128))
                    .count();
                assert!(changed >= 8, "character {character}, pose {pose}: only {changed} changed pixels");
            }
            assert_ne!(cell(0), cell(8), "sleep must be a separate slumped pose");
            assert_ne!(cell(12), cell(16), "upward walking must show the back");
        }
        assert_eq!(Animation::Type.frame(180), 1);
        assert_eq!(Animation::Sleep.frame(1000), 9);
        assert_eq!(Animation::WalkUp.frame(7), 17);
        assert_eq!(Animation::WalkRight.frame(28), 24);
    }

    #[test]
    fn all_assets_have_transparency_and_distinct_visible_pixels() {
        let mut hashes = std::collections::HashSet::new();
        for cell in PIXELS.as_chunks::<4096>().0 {
            assert!(cell.as_chunks::<4>().0.iter().any(|p| p[3] < 128));
            assert!(cell.as_chunks::<4>().0.iter().filter(|p| p[3] >= 128).count() > 50);
            hashes.insert(cell);
        }
        assert!(hashes.len() > 300, "pose atlas unexpectedly collapsed into duplicate frames");
    }
}
