//! Pre-rasterized DejaVu Sans. Alpha coverage is identical on desktop and ESP32.
use crate::gfx::Gfx;
use alloc::string::String;
const DATA: &[u8] = include_bytes!("../../../assets/fonts/controller-alpha.bin");
const STRIDE: usize = 1 + 24 * 22;
fn glyph(ch: char, k: i32) -> usize {
    let code = if ch.is_ascii() && (32..127).contains(&(ch as u32)) { ch as usize } else { b'?' as usize };
    ((if k == 1 { 0 } else { 95 }) + code - 32) * STRIDE
}
pub fn fit(text: &str, width: i32, k: i32) -> String {
    let mut out = String::new();
    let mut used = 0;
    if text.chars().map(|c| i32::from(DATA[glyph(c, k)])).sum::<i32>() <= width {
        return text.into();
    }
    let dots = 3 * i32::from(DATA[glyph('.', k)]);
    for ch in text.chars() {
        let next = i32::from(DATA[glyph(ch, k)]);
        if used + next + dots > width {
            break;
        }
        out.push(ch);
        used += next;
    }
    out.push_str("...");
    out
}
impl Gfx<'_> {
    pub fn ui_text(&mut self, text: &str, mut x: i32, y: i32, color: u32, k: i32, _shadow: Option<u32>) {
        for ch in text.chars() {
            let at = glyph(ch, k);
            let advance = DATA[at] as usize;
            for dy in 0..22 {
                for dx in 0..advance.min(24) {
                    let alpha = DATA[at + 1 + dy * 24 + dx];
                    if alpha != 0 {
                        self.blend_px(x + dx as i32, y + dy as i32, color, alpha);
                    }
                }
            }
            x += advance as i32;
        }
    }
}
