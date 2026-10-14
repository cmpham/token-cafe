//! Full-screen request review. Provider requests are immutable while displayed.
use crate::{
    approval::{Decision, Request, lines},
    gfx::Gfx,
};
use alloc::{format, string::String, vec::Vec};
#[derive(Clone)]
pub struct Review {
    pub request: Request,
    pub actionable: bool,
    pub page: usize,
    pub status: String,
    pub decision: Option<Decision>,
    pub allow_approve: bool,
    pub allow_deny: bool,
    pub dismiss_pending: bool,
    pub dismiss_all: bool,
}
impl Review {
    pub fn new(request: Request, actionable: bool) -> Self {
        Self {
            request,
            actionable,
            page: 0,
            status: String::new(),
            decision: None,
            allow_approve: true,
            allow_deny: true,
            dismiss_pending: false,
            dismiss_all: false,
        }
    }
    pub fn identity_fits(request: &Request) -> bool {
        lines(&format!("{}\nTOOL: {}", request.title, request.tool_name), 54).len() <= 8
    }
    pub fn content_lines(&self) -> Vec<String> {
        let mut out = lines(&format!("{}\nTOOL: {}", self.request.title, self.request.tool_name), 54);
        out.push(String::new());
        out.extend(lines(&self.request.text, 54));
        out
    }
    pub fn body_line_start(&self) -> usize {
        lines(&format!("{}\nTOOL: {}", self.request.title, self.request.tool_name), 54).len() + 1
    }
    pub fn anchor(&self) -> usize {
        crate::text::layout(&self.request.text, 54)
            .get(self.page.saturating_sub(self.body_line_start()))
            .map_or(0, |l| l.offset)
    }
    pub fn set_anchor(&mut self, anchor: usize) {
        self.page = if anchor == 0 {
            0
        } else {
            self.body_line_start() + crate::text::index(&crate::text::layout(&self.request.text, 54), anchor)
        };
    }
    pub fn page(&mut self, forward: bool) {
        let last = self.content_lines().len().saturating_sub(9);
        self.page = if forward { self.page.saturating_add(8).min(last) } else { self.page.saturating_sub(8) };
    }
    pub fn draw(&self, g: &mut Gfx) {
        g.noclip();
        g.rect(0, 0, 480, 272, 0x101b4c);
        g.rect(2, 2, 476, 268, 0x6176b4);
        g.rect(4, 4, 472, 264, 0x172e78);
        g.rect(4, 4, 472, 44, 0x254fc4);
        g.text("< CAFE", 12, 20, 0xe3e9ff, 1, None);
        g.text("DESKTOP >", 390, 20, 0xe3e9ff, 1, None);
        let title: String = self.request.title.chars().take(32).collect();
        g.text(&title, 12, 55, 0xffd95f, 1, None);
        g.text(&self.request.id.chars().take(32).collect::<String>(), 246, 55, 0x9cb2e6, 1, None);
        let text = self.content_lines();
        for (i, line) in text.iter().skip(self.page).take(9).enumerate() {
            g.text_literal(line, 12, 74 + i as i32 * 15, 0xe3e9ff);
        }
        // A full-height 44px touch rail; tap upper/lower half to page.
        g.rect(440, 72, 28, 138, 0x102057);
        g.text("^", 450, 81, 0xe3e9ff, 1, None);
        g.text("V", 450, 194, 0xe3e9ff, 1, None);
        let last = text.len().saturating_sub(9);
        let pos = (94 * self.page).checked_div(last).unwrap_or(0);
        g.rect(448, 94 + pos as i32, 12, 12, 0xa1b6e8);
        if self.dismiss_pending {
            g.rect(12, 220, 420, 44, 0xa1b6e8);
            g.rect(14, 222, 416, 40, 0x354678);
            g.text(
                if self.dismiss_all { "CHECKED ALL ON DESKTOP" } else { "CHECKED ON DESKTOP" },
                24,
                237,
                0xffffff,
                1,
                None,
            );
        } else if !self.status.is_empty() || !self.actionable {
            g.text(if self.status.is_empty() { "ANSWER IN DESKTOP" } else { &self.status }, 12, 238, 0xffd95f, 1, None);
        } else {
            for (x, w, label, color) in [(12, 144, "DENY", 0x354678), (168, 264, "APPROVE ONCE", 0x35784d)] {
                if (x == 12 && !self.allow_deny) || (x == 168 && !self.allow_approve) {
                    continue;
                }
                g.rect(x, 220, w, 44, 0xa1b6e8);
                g.rect(x + 2, 222, w - 4, 40, color);
                g.text(label, x + 12, 237, 0xffffff, 1, None);
            }
        }
    }
}
