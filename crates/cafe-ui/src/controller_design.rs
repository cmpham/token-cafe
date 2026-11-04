//! Controller design system v2. Native pixels, shared by rendering and input.
//! See docs/controller-design-system.md before introducing another token/component.
use crate::{
    controller_font,
    gfx::Gfx,
    scene::{State, Why},
};

pub const VERSION: u32 = 2;
/// Eight clockwise phases, one second per revolution.
pub const SPINNER_STEP_MS: u64 = 125;
pub mod color {
    pub const CANVAS: u32 = 0x171c27;
    pub const SURFACE: u32 = 0x252d3d;
    pub const BORDER: u32 = 0x46536a;
    pub const DISABLED: u32 = 0x8491a4;
    pub const TEXT: u32 = 0xf3f3ed;
    pub const SECONDARY: u32 = 0xb9c5d5;
    pub const FOCUS: u32 = 0xffd462;
    pub const ATTENTION: u32 = 0xffd462;
    pub const UNREAD: u32 = 0x58a6ff;
    pub const NOTICE: u32 = 0xffd462;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(self, x: i32, y: i32) -> bool {
        (self.x..self.x + self.w).contains(&x) && (self.y..self.y + self.h).contains(&y)
    }
    pub fn paint(self, g: &mut Gfx<'_>, color: u32) {
        g.rect(self.x, self.y, self.w, self.h, color);
    }
}
pub mod layout {
    use super::Rect;
    pub const WIDTH: i32 = 480;
    pub const HEIGHT: i32 = 272;
    pub const HEADER: i32 = 44;
    pub const FOOTER_Y: i32 = 228;
    pub const SPACE: i32 = 4;
    pub const FOCUS_WIDTH: i32 = 2;
    pub const PAGE_SIZE: usize = 6;
    pub const COLUMNS: usize = 2;
    pub const TILE_WIDTH: i32 = 234;
    pub const TILE_HEIGHT: i32 = 56;
    pub const MODE: Rect = Rect::new(0, 0, 88, HEADER);
    pub const PREV: Rect = Rect::new(0, FOOTER_Y, 56, 44);
    pub const NEXT: Rect = Rect::new(56, FOOTER_Y, 56, 44);
    pub const ORDER: Rect = Rect::new(300, FOOTER_Y, 116, 44);
    pub const DETAILS: Rect = Rect::new(416, FOOTER_Y, 64, 44);
    pub const TITLE_WIDTH: i32 = 202;
    pub const SUMMARY: Rect = Rect::new(112, FOOTER_Y, 188, 44);
    pub const NOTICE: Rect = Rect::new(116, 247, 180, 25);
    pub const DETAIL_BODY: Rect = Rect::new(0, HEADER, WIDTH, HEIGHT - HEADER);
    pub const NOTICE_WIDTH: i32 = 174;
    pub const fn filter(index: usize) -> Rect {
        Rect::new(88 + index as i32 * 98, 0, 98, HEADER)
    }
    pub const fn tile(index: usize) -> Rect {
        Rect::new(
            SPACE + (index % COLUMNS) as i32 * (TILE_WIDTH + SPACE),
            HEADER + SPACE + (index / COLUMNS) as i32 * (TILE_HEIGHT + SPACE),
            TILE_WIDTH,
            TILE_HEIGHT,
        )
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Text {
    Caption,
    Control,
    Title,
}
impl Text {
    pub const fn scale(self) -> i32 {
        match self {
            Self::Caption => 1,
            Self::Title => 2,
            Self::Control => 3,
        }
    }
}
/// Pixel-measured single-line label. Clipping/truncation is never character-count based.
pub fn label(g: &mut Gfx<'_>, text: &str, bounds: Rect, role: Text, color: u32) {
    let fitted = if text_width(text, role) <= bounds.w {
        alloc::borrow::Cow::Borrowed(text)
    } else {
        alloc::borrow::Cow::Owned(controller_font::fit(text, bounds.w, role.scale()))
    };
    g.ui_text(&fitted, bounds.x, bounds.y, color, role.scale(), None);
}
/// Header option with one active underline; the entire rectangle is its hit area.
pub fn tab(g: &mut Gfx<'_>, bounds: Rect, text: &str, selected: bool) {
    if selected {
        bounds.paint(g, color::SURFACE);
    }
    centered(g, text, bounds, Text::Control, if selected { color::TEXT } else { color::SECONDARY });
    if selected {
        g.rect(bounds.x, bounds.y + bounds.h - 3, bounds.w, 3, color::FOCUS);
    }
}
/// Centered labels retain padding and measured truncation.
pub fn centered(g: &mut Gfx<'_>, text: &str, bounds: Rect, role: Text, color: u32) {
    let width = text_width(text, role).min((bounds.w - 8).max(0));
    label(
        g,
        text,
        Rect::new(bounds.x + (bounds.w - width) / 2, bounds.y + (bounds.h - 22) / 2, width, 22),
        role,
        color,
    );
}
/// Visible chrome is inset inside the unchanged 44px-or-larger hit area.
pub fn button(g: &mut Gfx<'_>, bounds: Rect, text: &str, enabled: bool, active: bool) {
    chrome(g, bounds, enabled);
    centered(
        g,
        text,
        bounds,
        Text::Control,
        if !enabled {
            color::DISABLED
        } else if active {
            color::FOCUS
        } else {
            color::TEXT
        },
    );
}
fn chrome(g: &mut Gfx<'_>, bounds: Rect, enabled: bool) {
    let r = Rect::new(bounds.x + 2, bounds.y + 2, bounds.w - 4, bounds.h - 4);
    r.paint(g, if enabled { color::SURFACE } else { color::CANVAS });
    if enabled {
        g.rect(r.x, r.y, r.w, 1, color::BORDER);
        g.rect(r.x, r.y + r.h - 1, r.w, 1, color::BORDER);
        g.rect(r.x, r.y, 1, r.h, color::BORDER);
        g.rect(r.x + r.w - 1, r.y, 1, r.h, color::BORDER);
    }
}
pub fn page_button(g: &mut Gfx<'_>, bounds: Rect, forward: bool, enabled: bool) {
    chrome(g, bounds, enabled);
    let color = if enabled { color::TEXT } else { color::DISABLED };
    centered(
        g,
        if forward { "Next" } else { "Prev" },
        Rect::new(bounds.x, bounds.y + 1, bounds.w, 22),
        Text::Control,
        color,
    );
    centered(g, "page", Rect::new(bounds.x, bounds.y + 21, bounds.w, 20), Text::Caption, color);
}
/// Footer request/result feedback cannot cover the adjacent order control.
pub fn notice(g: &mut Gfx<'_>, text: &str) {
    layout::NOTICE.paint(g, color::CANVAS);
    let label_text = match text {
        "OPENING SESSION" => "Opening...",
        "OPEN REQUESTED" => "Open requested",
        "COULD NOT OPEN - RETRY" => "Could not open - retry",
        "DETAILS ONLY - NO DESKTOP TARGET" | "NO DESKTOP TARGET" => "No desktop target",
        "DESKTOP NAVIGATION REQUIRES MACOS" => "Requires macOS",
        other => other,
    };
    label(g, label_text, Rect::new(120, 249, layout::NOTICE_WIDTH, 22), Text::Control, color::NOTICE);
}
/// Tile surface plus the sole selection indicator. Does not imply desktop focus.
pub fn tile(g: &mut Gfx<'_>, bounds: Rect, selected: bool) {
    bounds.paint(g, color::SURFACE);
    if selected {
        let b = layout::FOCUS_WIDTH;
        g.rect(bounds.x, bounds.y, bounds.w, b, color::FOCUS);
        g.rect(bounds.x, bounds.y + bounds.h - b, bounds.w, b, color::FOCUS);
        g.rect(bounds.x, bounds.y, b, bounds.h, color::FOCUS);
        g.rect(bounds.x + bounds.w - b, bounds.y, b, bounds.h, color::FOCUS);
    }
}
/// A locally active control: same surface as tiles; gold denotes the active option.
pub fn option(g: &mut Gfx<'_>, bounds: Rect, text: &str) {
    button(g, bounds, text, true, true);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusIcon {
    Working,
    NewOutput,
    Input,
    Approval,
    Offline,
    Idle,
}
impl StatusIcon {
    /// Primary icon precedence preserves the current desktop/board behavior.
    /// Unread is orthogonal: sorting may prioritize it even while work continues.
    pub fn resolve(state: State, why: Why, unread: bool, offline: bool) -> Self {
        if offline {
            Self::Offline
        } else {
            match state {
                State::Work => Self::Working,
                State::Wait => {
                    if why == Why::Perm {
                        Self::Approval
                    } else {
                        Self::Input
                    }
                }
                State::Idle => {
                    if unread {
                        Self::NewOutput
                    } else {
                        Self::Idle
                    }
                }
            }
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::NewOutput => "New output",
            Self::Input => "Waiting for input",
            Self::Approval => "Waiting for approval",
            Self::Offline => "Disconnected",
            Self::Idle => "Idle",
        }
    }
    pub fn draw(self, g: &mut Gfx<'_>, x: i32, y: i32) {
        self.draw_at(g, x, y, 0);
    }
    pub fn draw_at(self, g: &mut Gfx<'_>, x: i32, y: i32, now_ms: u64) {
        match self {
            Self::Offline => g.ui_text("X", x, y, color::SECONDARY, 2, None),
            Self::Working => {
                let head = ((now_ms / SPINNER_STEP_MS) % 8) as usize;
                for (index, (dx, dy)) in
                    [(0, 0), (4, -2), (8, 0), (10, 4), (8, 8), (4, 10), (0, 8), (-2, 4)].into_iter().enumerate()
                {
                    let age = (head + 8 - index) % 8;
                    let alpha = [255, 190, 125, 75, 45, 45, 45, 45][age];
                    for py in 0..2 {
                        for px in 0..2 {
                            g.blend_px(x + dx + px, y + dy + py, color::TEXT, alpha);
                        }
                    }
                }
            }
            Self::Approval | Self::Input => {
                g.ui_text(if self == Self::Approval { "!" } else { "?" }, x, y, color::ATTENTION, 2, None)
            }
            Self::NewOutput => {
                for dy in -4..=5 {
                    for dx in -4..=5 {
                        if (2 * dx - 1) * (2 * dx - 1) + (2 * dy - 1) * (2 * dy - 1) <= 100 {
                            g.px(x + 5 + dx, y + 5 + dy, color::UNREAD);
                        }
                    }
                }
            }
            Self::Idle => (),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn targets_fit_canvas_and_do_not_overlap() {
        let mut targets = alloc::vec![layout::MODE, layout::PREV, layout::NEXT, layout::ORDER, layout::DETAILS];
        targets.extend((0..4).map(layout::filter));
        targets.extend((0..layout::PAGE_SIZE).map(layout::tile));
        for (i, a) in targets.iter().enumerate() {
            assert!(a.w >= 44 && a.h >= 44);
            assert!(a.x >= 0 && a.y >= 0 && a.x + a.w <= layout::WIDTH && a.y + a.h <= layout::HEIGHT);
            for b in &targets[i + 1..] {
                assert!(a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y);
            }
        }
    }
    #[test]
    fn status_semantics_are_explicit() {
        assert_eq!(StatusIcon::resolve(State::Idle, Why::None, true, false), StatusIcon::NewOutput);
        assert_eq!(StatusIcon::resolve(State::Wait, Why::Perm, true, false), StatusIcon::Approval);
        assert_eq!(StatusIcon::resolve(State::Work, Why::None, true, false), StatusIcon::Working);
        assert_eq!(StatusIcon::resolve(State::Wait, Why::Ask, false, true), StatusIcon::Offline);
    }
}

/// Measure with the exact atlas used by the renderer (not character count).
pub fn text_width(text: &str, role: Text) -> i32 {
    controller_font::width(text, role.scale())
}

#[cfg(test)]
mod typography_tests {
    use super::*;
    #[test]
    fn ellipsis_never_exceeds_its_allocation() {
        for role in [Text::Caption, Text::Control, Text::Title] {
            for width in 0..180 {
                let fitted = controller_font::fit("WWWW iii mixed Case title", width, role.scale());
                assert!(text_width(&fitted, role) <= width);
            }
        }
    }
    #[test]
    fn proportional_case_sensitive_type_is_used() {
        assert_ne!(text_width("WWW", Text::Title), text_width("iii", Text::Title));
        assert_ne!(text_width("ABC", Text::Title), text_width("abc", Text::Title));
    }
}

/// Bounded, pixel-measured detail preview. Escapes controls/unsupported Unicode;
/// hard wrapping preserves character order, and truncation is always explicit.
pub struct Wrapped {
    pub lines: alloc::vec::Vec<alloc::string::String>,
    pub truncated: bool,
}
pub fn wrap(text: &str, width: i32, role: Text, max_lines: usize) -> Wrapped {
    use alloc::{string::String, vec::Vec};
    let mut out = Wrapped { lines: Vec::new(), truncated: false };
    if max_lines == 0 || width <= 0 {
        out.truncated = !text.is_empty();
        return out;
    }
    let mut line = String::new();
    let mut used = 0;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            out.lines.push(core::mem::take(&mut line));
            used = 0;
            if out.lines.len() == max_lines {
                out.truncated = chars.peek().is_some();
                return out;
            }
            continue;
        }
        for ch in c.escape_default() {
            let advance = controller_font::advance(ch, role.scale());
            if advance > width {
                out.truncated = true;
                return out;
            }
            if used + advance > width {
                out.lines.push(core::mem::take(&mut line));
                used = 0;
                if out.lines.len() == max_lines {
                    out.truncated = true;
                    return out;
                }
            }
            line.push(ch);
            used += advance;
        }
    }
    if !line.is_empty() || out.lines.is_empty() {
        out.lines.push(line);
    }
    out
}
#[cfg(test)]
mod wrap_tests {
    use super::*;
    #[test]
    fn wide_text_wraps_without_dropping_content() {
        let text = "WWWWWWW iiiii";
        let out = wrap(text, 27, Text::Control, 30);
        assert!(!out.truncated);
        assert_eq!(out.lines.concat(), text);
        assert!(out.lines.iter().all(|s| text_width(s, Text::Control) <= 27));
    }
    #[test]
    fn truncation_and_escaping_are_explicit() {
        let out = wrap("a\t\0é", 400, Text::Control, 1);
        assert_eq!(out.lines[0], "a\\t\\u{0}\\u{e9}");
        assert!(!out.truncated);
        assert!(wrap("a very long preview", 12, Text::Control, 1).truncated);
        assert!(wrap("W", 1, Text::Control, 7).truncated);
    }
}

#[cfg(test)]
mod motion_tests {
    use super::*;
    fn render(icon: StatusIcon, time: u64) -> alloc::vec::Vec<u32> {
        let mut frame = alloc::vec![color::SURFACE;480*272];
        icon.draw_at(&mut Gfx::new(&mut frame), 30, 30, time);
        frame
    }
    #[test]
    fn working_ring_advances_and_loops() {
        assert_eq!(render(StatusIcon::Working, 0), render(StatusIcon::Working, 124));
        assert_ne!(render(StatusIcon::Working, 0), render(StatusIcon::Working, 125));
        assert_eq!(render(StatusIcon::Working, 0), render(StatusIcon::Working, 1000));
    }
    #[test]
    fn other_statuses_do_not_animate() {
        for icon in
            [StatusIcon::NewOutput, StatusIcon::Input, StatusIcon::Approval, StatusIcon::Offline, StatusIcon::Idle]
        {
            assert_eq!(render(icon, 0), render(icon, 125));
        }
    }
    #[test]
    fn motion_stays_in_the_icon_area() {
        let a = render(StatusIcon::Working, 0);
        let b = render(StatusIcon::Working, 125);
        for (i, (a, b)) in a.iter().zip(&b).enumerate() {
            if a != b {
                let (x, y) = ((i % 480) as i32, (i / 480) as i32);
                assert!((28..44).contains(&x) && (28..44).contains(&y));
            }
        }
    }
}
