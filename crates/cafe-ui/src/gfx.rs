//! Software drawing into the 480x272 frame (0x00RRGGBB pixels): rectangles,
//! shadows, palette sprites and the 5x7 font, the primitives design/cafe.js
//! draws with. Every call clips to the screen and to the current clip
//! rectangle.

use crate::assets::{BASE_PAL, FONT, FONT_HAS, H, Pal, Sprite, W};

impl Pal {
    pub fn base() -> Pal {
        BASE_PAL
    }

    pub fn set(&mut self, slot: usize, c: u32) {
        self.c[slot] = c;
        self.set |= 1 << slot;
    }
}

pub struct Gfx<'a> {
    fb: &'a mut [u32],
    cx0: i32,
    cy0: i32,
    cx1: i32,
    cy1: i32,
}

pub fn text_width(s: &str, k: i32) -> i32 {
    let n = s.chars().count() as i32;
    if n > 0 { (n * 6 - 1) * k } else { 0 }
}

fn glyph(ch: u8) -> &'static [u8; 7] {
    let mut c = ch.to_ascii_uppercase() as usize;
    if !(32..=127).contains(&c) || !FONT_HAS[c - 32] {
        c = b'?' as usize;
    }
    &FONT[c - 32]
}

impl<'a> Gfx<'a> {
    pub fn new(fb: &'a mut [u32]) -> Self {
        assert_eq!(fb.len(), W * H);
        Gfx { fb, cx0: 0, cy0: 0, cx1: W as i32, cy1: H as i32 }
    }

    /// Half-open box.
    pub fn clip(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        self.cx0 = x0.max(0);
        self.cy0 = y0.max(0);
        self.cx1 = x1.min(W as i32);
        self.cy1 = y1.min(H as i32);
    }

    pub fn noclip(&mut self) {
        self.clip(0, 0, W as i32, H as i32);
    }

    pub fn px(&mut self, x: i32, y: i32, c: u32) {
        if x < self.cx0 || y < self.cy0 || x >= self.cx1 || y >= self.cy1 {
            return;
        }
        self.fb[y as usize * W + x as usize] = c;
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        let (x0, y0) = (x.max(self.cx0), y.max(self.cy0));
        let (x1, y1) = ((x + w).min(self.cx1), (y + h).min(self.cy1));
        for j in y0..y1 {
            for i in x0..x1 {
                self.fb[j as usize * W + i as usize] = c;
            }
        }
    }

    /// Multiply the pixels under a box by k (shadows).
    pub fn darken(&mut self, x: i32, y: i32, w: i32, h: i32, k: f64) {
        let (x0, y0) = (x.max(self.cx0), y.max(self.cy0));
        let (x1, y1) = ((x + w).min(self.cx1), (y + h).min(self.cy1));
        // The S3 emulates f64. Evaluate each channel once, preserving the
        // original truncation exactly, instead of doing three soft-float
        // multiplications for every shadow pixel.
        const fn shadow_table(k: f64) -> [u32; 256] {
            let mut table = [0; 256];
            let mut i = 0;
            while i < 256 {
                table[i] = (i as f64 * k) as u32;
                i += 1;
            }
            table
        }
        const DESK_SHADOW: [u32; 256] = shadow_table(0.55);
        let custom;
        let channels = if k == 0.55 {
            &DESK_SHADOW
        } else {
            custom = shadow_table(k);
            &custom
        };
        let scale = |v: u32| channels[(v & 255) as usize];
        for j in y0..y1 {
            for i in x0..x1 {
                let p = &mut self.fb[j as usize * W + i as usize];
                *p = (scale(*p >> 16) << 16) | (scale(*p >> 8) << 8) | scale(*p);
            }
        }
    }

    /// Sprites use palette slots; None means the base palette.
    pub fn blit(&mut self, s: &Sprite, x: i32, y: i32, pal: Option<&Pal>, flip: bool) {
        let pal = pal.unwrap_or(&BASE_PAL);
        let w = s.w as usize;
        for j in 0..s.h as usize {
            let row = &s.px[j * w..(j + 1) * w];
            for i in 0..w {
                let slot = row[if flip { w - 1 - i } else { i }] as usize;
                if slot == 0 || (pal.set >> slot) & 1 == 0 {
                    continue;
                }
                self.px(x + i as i32, y + j as i32, pal.c[slot]);
            }
        }
    }

    /// Uniform 3:2 nearest-neighbor enlargement for room characters.
    pub fn blit_large(&mut self, s: &Sprite, x: i32, y: i32, pal: Option<&Pal>, flip: bool) {
        let pal = pal.unwrap_or(&BASE_PAL);
        for dy in 0..(s.h as i32 * 3 / 2) {
            for dx in 0..(s.w as i32 * 3 / 2) {
                let sx = dx * 2 / 3;
                let sx = if flip { s.w as i32 - 1 - sx } else { sx };
                let slot = s.px[(dy * 2 / 3 * s.w as i32 + sx) as usize] as usize;
                if slot != 0 && (pal.set >> slot) & 1 != 0 {
                    self.px(x + dx, y + dy, pal.c[slot]);
                }
            }
        }
    }

    /// k-times enlarged sprite, optionally limited to the box [x0, y0, x1, y1).
    pub fn blit_scaled(&mut self, s: &Sprite, x: i32, y: i32, pal: Option<&Pal>, k: i32, bx: Option<[i32; 4]>) {
        let pal = pal.unwrap_or(&BASE_PAL);
        let w = s.w as usize;
        for j in 0..s.h as usize {
            for i in 0..w {
                let slot = s.px[j * w + i] as usize;
                if slot == 0 || (pal.set >> slot) & 1 == 0 {
                    continue;
                }
                for v in 0..k {
                    for u in 0..k {
                        let (px, py) = (x + i as i32 * k + u, y + j as i32 * k + v);
                        if let Some(b) = bx
                            && (px < b[0] || py < b[1] || px >= b[2] || py >= b[3])
                        {
                            continue;
                        }
                        self.px(px, py, pal.c[slot]);
                    }
                }
            }
        }
    }

    /// Case-sensitive permission text; lowercase glyphs must not be uppercased.
    pub fn text_literal(&mut self, s: &str, x: i32, y: i32, color: u32) {
        const LOWER: [[u8; 7]; 26] = [
            [0, 0, 14, 1, 15, 17, 15],
            [16, 16, 30, 17, 17, 17, 30],
            [0, 0, 14, 16, 16, 17, 14],
            [1, 1, 15, 17, 17, 17, 15],
            [0, 0, 14, 17, 31, 16, 14],
            [6, 9, 8, 28, 8, 8, 8],
            [0, 15, 17, 17, 15, 1, 14],
            [16, 16, 30, 17, 17, 17, 17],
            [4, 0, 12, 4, 4, 4, 14],
            [2, 0, 6, 2, 2, 18, 12],
            [16, 16, 18, 20, 24, 20, 18],
            [12, 4, 4, 4, 4, 4, 14],
            [0, 0, 26, 21, 21, 21, 21],
            [0, 0, 30, 17, 17, 17, 17],
            [0, 0, 14, 17, 17, 17, 14],
            [0, 0, 30, 17, 30, 16, 16],
            [0, 0, 15, 17, 15, 1, 1],
            [0, 0, 22, 25, 16, 16, 16],
            [0, 0, 15, 16, 14, 1, 30],
            [8, 8, 28, 8, 8, 9, 6],
            [0, 0, 17, 17, 17, 19, 13],
            [0, 0, 17, 17, 17, 10, 4],
            [0, 0, 17, 17, 21, 21, 10],
            [0, 0, 17, 10, 4, 10, 17],
            [0, 0, 17, 17, 15, 1, 14],
            [0, 0, 31, 2, 4, 8, 31],
        ];
        for (n, ch) in s.bytes().enumerate() {
            let rows = if ch.is_ascii_lowercase() { &LOWER[(ch - b'a') as usize] } else { glyph(ch) };
            for (j, bits) in rows.iter().enumerate() {
                for i in 0..5 {
                    if bits & (1 << (4 - i)) != 0 {
                        self.px(x + n as i32 * 7 + i, y + j as i32, color);
                    }
                }
            }
        }
    }

    /// 5x7 capitals, 6 px advance per character at scale 1; lower case is
    /// upper-cased. The shadow is drawn 1 px down-right first.
    pub fn text(&mut self, s: &str, x: i32, y: i32, c: u32, k: i32, shadow: Option<u32>) {
        let passes: &[(u32, i32)] = match shadow {
            Some(sh) => &[(sh, 1), (c, 0)],
            None => &[(c, 0)],
        };
        for &(col, o) in passes {
            for (n, ch) in s.chars().enumerate() {
                let g = glyph(if ch.is_ascii() { ch as u8 } else { b'?' });
                for (j, bits) in g.iter().enumerate() {
                    for i in 0..5 {
                        if (bits >> (4 - i)) & 1 == 0 {
                            continue;
                        }
                        self.rect(x + (n as i32 * 6 + i) * k + o, y + j as i32 * k + o, k, k, col);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shadow_lookup_is_pixel_exact() {
        let mut frame = alloc::vec![0; W * H];
        for k in [0.0, 0.55, 0.8, 1.0] {
            for (i, p) in frame[..256].iter_mut().enumerate() {
                *p = (i as u32) << 16 | ((255 - i) as u32) << 8 | ((i * 37 % 256) as u32);
            }
            let original = frame[..256].to_vec();
            Gfx::new(&mut frame).darken(0, 0, 256, 1, k);
            for (p, v) in frame.iter().zip(original) {
                let scale = |x: u32| ((x & 255) as f64 * k) as u32;
                assert_eq!(*p, scale(v >> 16) << 16 | scale(v >> 8) << 8 | scale(v));
            }
        }
    }
}
