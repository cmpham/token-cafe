//! Full-screen request review. Provider requests are immutable while displayed.
use crate::{
    approval::{Decision, Request, lines},
    assets::{self as a, c},
    gfx::Gfx,
};
use alloc::{format, string::String, vec::Vec};
#[derive(Clone)]
pub struct Review {
    request: Request,
    identity: Vec<String>,
    title_lines: usize,
    body: Vec<crate::text::Line>,
    pub actionable: bool,
    pub page: usize,
    pub status: String,
    pub decision: Option<Decision>,
    pub selected_choice: Option<u8>,
    pub allow_approve: bool,
    pub allow_deny: bool,
    pub dismiss_pending: bool,
    pub dismiss_all: bool,
}
impl Review {
    pub fn resolved_status(decision: i32) -> &'static str {
        match decision {
            1 => "APPROVED ON DESKTOP",
            2 => "DENIED ON DESKTOP",
            _ => "REQUEST RESOLVED",
        }
    }
    pub fn new(request: Request, actionable: bool) -> Self {
        let identity = Self::identity_lines(&request);
        let title_lines = if request.choices > 0 { 0 } else { lines(&request.title, 54).len() };
        let body = crate::text::layout(&request.text, 54);
        Self {
            request,
            identity,
            title_lines,
            body,
            actionable,
            page: 0,
            status: String::new(),
            decision: None,
            selected_choice: None,
            allow_approve: true,
            allow_deny: true,
            dismiss_pending: false,
            dismiss_all: false,
        }
    }
    fn identity_lines(request: &Request) -> Vec<String> {
        if request.choices > 0 {
            return Vec::new();
        }
        let mut out = lines(&request.title, 54);
        if !request.tool_name.is_empty() {
            out.extend(lines(&format!("TOOL: {}", request.tool_name), 54));
        }
        out
    }
    pub fn identity_fits(request: &Request) -> bool {
        Self::identity_lines(request).len() <= 8
    }
    /// Keep the source immutable so cached literal lines and approval identity
    /// cannot diverge. Replace the Review when its request changes.
    pub fn request(&self) -> &Request {
        &self.request
    }
    fn visible_lines(&self) -> usize {
        if self.request.choices > 0 { 6 } else { 9 }
    }
    pub fn choice_at(&self, x: i32, y: i32) -> Option<u8> {
        if self.request.choices < 2
            || !self.actionable
            || !self.status.is_empty()
            || !(168..212).contains(&y)
            || !(18..432).contains(&x)
        {
            return None;
        }
        let n = ((x - 18) / 104) as u8;
        (n < self.request.choices).then_some(n)
    }
    fn line_count(&self) -> usize {
        self.body_line_start() + self.body.len()
    }
    fn text_lines(&self) -> impl Iterator<Item = &str> {
        self.identity
            .iter()
            .map(String::as_str)
            .chain(core::iter::repeat_n("", usize::from(self.request.choices == 0)))
            .chain(self.body.iter().map(|line| line.text.as_str()))
    }
    pub fn content_lines(&self) -> Vec<String> {
        self.text_lines().map(String::from).collect()
    }
    pub fn body_line_start(&self) -> usize {
        self.identity.len() + usize::from(self.request.choices == 0)
    }
    pub fn anchor(&self) -> usize {
        self.body.get(self.page.saturating_sub(self.body_line_start())).map_or(0, |l| l.offset)
    }
    pub fn set_anchor(&mut self, anchor: usize) {
        self.page = if anchor == 0 { 0 } else { self.body_line_start() + crate::text::index(&self.body, anchor) };
    }
    pub fn page(&mut self, forward: bool) {
        let last = self.line_count().saturating_sub(self.visible_lines());
        self.page = if forward {
            self.page.saturating_add(self.visible_lines() - 1).min(last)
        } else {
            self.page.saturating_sub(self.visible_lines() - 1)
        };
    }
    pub fn draw(&self, g: &mut Gfx) {
        let (pal, style) = crate::scene::review_appearance(&self.request.session);
        self.draw_for_patron(g, &self.request.session, &pal, style);
    }

    pub fn draw_for_patron(&self, g: &mut Gfx, name: &str, pal: &a::Pal, style: usize) {
        g.noclip();
        // Use the room dialogue's stepped gradient and beveled pixel frame.
        // Paint borders only: filling nested panels would write the full PSRAM
        // framebuffer five times before any text is drawn.
        fn border(g: &mut Gfx, x: i32, y: i32, w: i32, h: i32, color: u32) {
            g.rect(x, y, w, 1, color);
            g.rect(x, y + h - 1, w, 1, color);
            g.rect(x, y, 1, h, color);
            g.rect(x + w - 1, y, 1, h, color);
        }
        g.rect(0, 0, 480, 3, c::WIN_OUTER);
        g.rect(0, 269, 480, 3, c::WIN_OUTER);
        g.rect(0, 0, 3, 272, c::WIN_OUTER);
        g.rect(477, 0, 3, 272, c::WIN_OUTER);
        border(g, 2, 1, 476, 270, c::WIN_BORDER);
        border(g, 1, 2, 478, 268, c::WIN_BORDER);
        border(g, 3, 3, 474, 266, c::WIN_BORDER_LO);
        border(g, 4, 4, 472, 264, c::WIN_BORDER_LO);
        for y in 5..267 {
            let step = ((y - 5) / 16) as u32;
            let mix = |shift: u32| {
                let hi = (c::WIN0 >> shift) & 255u32;
                let lo = (c::WIN1 >> shift) & 255u32;
                (hi * (16 - step) + lo * step) / 16
            };
            g.rect(5, y, 470, 1, mix(16) << 16 | mix(8) << 8 | mix(0));
        }
        g.text("< CAFE", 16, 22, c::TEXT, 1, Some(c::TEXT_SHADOW));
        g.text("DESKTOP >", 390, 22, c::TEXT, 1, Some(c::TEXT_SHADOW));
        // The selected patron remains prominent across cafe, footer, and review.
        draw_patron_portrait(g, pal, style, 104, 9);
        g.rect(146, 10, 232, 33, c::WIN1);
        g.rect(146, 10, 3, 33, c::GOLD);
        let name = crate::scene::shorten(name, 18);
        g.text(&name, 154, 14, c::GOLD, 2, Some(c::TEXT_SHADOW));
        g.text(
            if self.request.choices > 0 {
                "CHOOSE AN ANSWER"
            } else if self.actionable || !self.request.tool_name.is_empty() {
                "APPROVAL REQUEST"
            } else {
                "QUESTION / TASK DETAILS"
            },
            154,
            33,
            c::TEXT,
            1,
            None,
        );
        g.rect(16, 50, 448, 1, c::WIN1);
        g.rect(16, 51, 448, 1, c::WIN_BORDER_LO);
        let count = self.line_count();
        let last = count.saturating_sub(self.visible_lines());
        g.text(if self.actionable { "REVIEW REQUEST" } else { "SESSION DETAILS" }, 18, 59, c::TEXT_DIM, 1, None);
        if last > 0 {
            let position = format!("{}-{} / {}", self.page + 1, (self.page + self.visible_lines()).min(count), count);
            g.text(&position, 432 - crate::gfx::text_width(&position, 1), 59, c::TEXT_DIM, 1, None);
        }
        g.clip(18, 74, 432, if self.request.choices > 0 { 164 } else { 212 });
        for (i, line) in self.text_lines().skip(self.page).take(self.visible_lines()).enumerate() {
            let color = if self.page + i < self.title_lines { c::GOLD } else { c::TEXT };
            g.text_literal(line, 19, 75 + i as i32 * 15, c::TEXT_SHADOW);
            g.text_literal(line, 18, 74 + i as i32 * 15, color);
        }
        g.noclip();
        // Keep the existing 44px touch target; hide the rail when all text fits.
        if last > 0 {
            g.rect(448, 91, 8, 100, c::WIN1);
            g.rect(456, 91, 1, 100, c::WIN_BORDER_LO);
            g.text("^", 449, 77, if self.page > 0 { c::TEXT } else { c::TEXT_DIM }, 1, None);
            g.text("V", 449, 199, if self.page < last { c::TEXT } else { c::TEXT_DIM }, 1, None);
            let height = (self.visible_lines() * 100 / count).max(10) as i32;
            let pos = (100 - height) * self.page.min(last) as i32 / last as i32;
            g.rect(448, 91 + pos, 8, height, c::WIN_BORDER_LO);
            g.rect(448, 91 + pos, 8, 1, c::WIN_BORDER);
        }
        if self.request.choices > 0 {
            for n in 0..self.request.choices {
                let x = 18 + i32::from(n) * 104;
                g.rect(x, 168, 98, 44, if self.selected_choice == Some(n) { c::GOLD } else { c::WIN_BORDER_LO });
                g.rect(x + 2, 170, 94, 40, c::WIN1);
                g.text(
                    &format!("OPTION {}", n + 1),
                    x + 12,
                    186,
                    if self.selected_choice == Some(n) { c::GOLD } else { c::TEXT },
                    1,
                    None,
                );
            }
        }
        g.rect(16, 215, 448, 1, c::WIN_BORDER_LO);
        if self.dismiss_pending {
            g.rect(12, 220, 420, 44, c::WIN_BORDER_LO);
            g.rect(14, 222, 416, 40, c::WIN1);
            g.text(
                if self.dismiss_all { "CHECKED ALL ON DESKTOP" } else { "CHECKED ON DESKTOP" },
                24,
                237,
                c::TEXT,
                1,
                None,
            );
        } else if !self.status.is_empty() || !self.actionable {
            g.text(
                if self.status.is_empty() { "CONTINUE ON DESKTOP" } else { &self.status },
                12,
                238,
                c::GOLD,
                1,
                None,
            );
        } else if self.request.choices > 0 {
            let label = self
                .selected_choice
                .map_or_else(|| String::from("SELECT AN OPTION ABOVE"), |n| format!("SUBMIT OPTION {}", n + 1));
            draw_button(g, 168, 220, 264, 44, &label);
        } else {
            for (x, w, label, color) in [(12, 144, "DENY", c::WIN1), (168, 264, "APPROVE ONCE", 0x35784d)] {
                if (x == 12 && !self.allow_deny) || (x == 168 && !self.allow_approve) {
                    continue;
                }
                g.rect(x, 220, w, 44, c::WIN_BORDER_LO);
                g.rect(x + 2, 222, w - 4, 40, color);
                g.text(label, x + 12, 237, c::TEXT, 1, None);
            }
        }
    }
}

/// A framed, cropped portrait uses the same hair style and colors as the seated patron.
pub(crate) fn draw_patron_portrait(g: &mut Gfx, pal: &a::Pal, style: usize, x: i32, y: i32) {
    g.rect(x, y, 34, 36, c::GOLD);
    g.rect(x + 2, y + 2, 30, 32, c::WIN1);
    g.blit_scaled(&a::POSES[style].stand_d, x - 5, y - 4, Some(pal), 2, Some([x + 2, y + 2, x + 32, y + 34]));
}

pub(crate) fn draw_button(g: &mut Gfx, x: i32, y: i32, w: i32, h: i32, label: &str) {
    g.rect(x, y, w, h, c::WIN_BORDER);
    g.rect(x + 1, y + 1, w - 2, h - 2, c::WIN1);
    g.rect(x + 1, y + h - 2, w - 2, 1, c::WIN_BORDER_LO);
    g.text(label, x + (w - crate::gfx::text_width(label, 1)) / 2, y + (h - 7) / 2, c::TEXT, 1, None);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_pages_preserve_literal_body_and_byte_anchors() {
        let request = Request {
            choices: 0,
            id: "id".into(),
            session: "session".into(),
            title: "Title".into(),
            tool_name: "Bash".into(),
            text: "LiteralCase \\ \t é\n".repeat(60),
        };
        let mut expected = Review::identity_lines(&request);
        expected.push(String::new());
        expected.extend(lines(&request.text, 54));
        let reference = crate::text::layout(&request.text, 54);
        let mut review = Review::new(request, false);
        assert_eq!(review.content_lines(), expected);
        for anchor in [0, 1, 23, 78, 200, review.request().text.len()] {
            review.set_anchor(anchor);
            let expected_page = if anchor == 0 { 0 } else { 3 + crate::text::index(&reference, anchor) };
            assert_eq!(review.page, expected_page);
            assert_eq!(review.anchor(), reference[expected_page.saturating_sub(3)].offset);
        }
        review.set_anchor(0);
        review.page(true);
        assert_eq!(review.page, 8);
        review.page(false);
        assert_eq!(review.page, 0);
    }
}
#[test]
fn choices_have_no_default_and_page_without_skipping_lines() {
    let request = Request {
        choices: 4,
        id: "q".into(),
        session: "codex:qa".into(),
        title: "CHOOSE AN ANSWER".into(),
        tool_name: "Codex question".into(),
        text: (0..20).map(|i| format!("Line {i}\n")).collect(),
    };
    let mut review = Review::new(request, true);
    assert_eq!(review.selected_choice, None);
    assert_eq!(review.content_lines()[0], "Line 0");
    assert_eq!(review.choice_at(345, 190), Some(3));
    review.page(true);
    assert_eq!(review.page, 5);
    review.page(false);
    assert_eq!(review.page, 0);
    review.status = "REQUEST EXPIRED".into();
    assert_eq!(review.choice_at(35, 190), None);
}
