//! Native 480x272 session controller shared by desktop and board.
use crate::{
    gfx::Gfx,
    scene::{Kind, Scene, State, Why, shorten},
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::String,
    vec::Vec,
};
#[derive(Default)]
pub struct Controller {
    pub enabled: bool,
    pub status_order: bool,
    pub notice: String,
    pub outputs: BTreeMap<String, String>,
    pub unread: BTreeSet<String>,
    pub filter: u32,
    pub details: bool,
    pub detail_age: u32,
    pub available: BTreeMap<String, bool>,
    pub disconnected: Vec<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    Mode,
    Sort,
    Filter(u32),
    Page(bool),
    Details,
    Session(String),
    None,
}
impl Scene {
    pub fn controller_ids(&self) -> Vec<String> {
        let mut rows: Vec<_> = self
            .sessions()
            .iter()
            .filter(|s| match self.controller.filter {
                1 => s.kind == Kind::Claude,
                2 => s.kind == Kind::Codex,
                3 => s.state == State::Wait || self.selected_id() == Some(s.id.as_str()),
                _ => true,
            })
            .collect();
        rows.sort_by_key(|s| {
            let rank = if !self.controller.status_order {
                0
            } else if self.controller.disconnected.contains(&s.id) {
                4
            } else if s.state == State::Wait {
                0
            } else if self.controller.unread.contains(&s.id) {
                1
            } else if s.state == State::Work {
                2
            } else {
                3
            };
            (rank, &s.id)
        });
        rows.into_iter().map(|s| s.id.clone()).collect()
    }
    pub fn controller_available(&self, id: &str) -> bool {
        self.controller.available.get(id).copied().unwrap_or_else(|| self.session_target(id).is_some())
    }
    pub fn controller_preselect(&mut self) {
        let ids = self.controller_ids();
        let id = ids.iter().find(|id| self.controller_available(id)).or(ids.first()).cloned();
        if let Some(id) = id {
            self.focus_patron(id);
        } else {
            self.clear_selection();
        }
    }
    pub fn controller_toggle(&mut self) {
        self.controller.enabled = !self.controller.enabled;
        self.review = None;
        self.controller.details = false;
        self.controller_preselect();
    }
    pub fn controller_move(&mut self, forward: bool, navigable: bool) -> Option<String> {
        let ids = self.controller_ids();
        if ids.is_empty() {
            return None;
        }
        let current = self.selected_id().and_then(|id| ids.iter().position(|s| s == id));
        for n in 1..=ids.len() {
            let i = match (current, forward) {
                (Some(i), true) => (i + n) % ids.len(),
                (Some(i), false) => (i + ids.len() - n) % ids.len(),
                (None, true) => n - 1,
                (None, false) => ids.len() - n,
            };
            if !navigable || self.controller_available(&ids[i]) {
                let id = ids[i].clone();
                self.focus_patron(id.clone());
                self.controller.details = false;
                return Some(id);
            }
        }
        None
    }
    pub fn controller_hit(&self, x: i32, y: i32) -> Hit {
        if !(0..480).contains(&x) || !(0..272).contains(&y) {
            return Hit::None;
        }
        if !self.controller.enabled {
            return Hit::None;
        }
        if y < 44 {
            return if x < 88 { Hit::Mode } else { Hit::Filter(((x - 88) / 98).min(3) as u32) };
        }
        if self.controller.details {
            return if y >= 228 { Hit::Details } else { Hit::None };
        }
        if y >= 228 {
            return if x < 56 {
                Hit::Page(false)
            } else if x < 112 {
                Hit::Page(true)
            } else if x >= 416 {
                Hit::Details
            } else if x >= 300 {
                Hit::Sort
            } else {
                Hit::None
            };
        }
        let ids = self.controller_ids();
        let page = self.selected_id().and_then(|id| ids.iter().position(|s| s == id)).unwrap_or(0) / 6;
        let col = (x - 4) / 238;
        let row = (y - 48) / 60;
        if x < 4 || y < 48 || col > 1 || row > 2 || (x - 4) % 238 >= 234 || (y - 48) % 60 >= 56 {
            return Hit::None;
        }
        ids.get(page * 6 + row as usize * 2 + col as usize).cloned().map_or(Hit::None, Hit::Session)
    }
    pub fn controller_page(&mut self, forward: bool) {
        let ids = self.controller_ids();
        if ids.is_empty() {
            return;
        }
        let page = self.selected_id().and_then(|id| ids.iter().position(|s| s == id)).unwrap_or(0) / 6;
        let target = if forward { (page + 1).min((ids.len() - 1) / 6) } else { page.saturating_sub(1) };
        self.focus_patron(ids[target * 6].clone());
    }
    pub fn controller_click(&mut self, x: i32, y: i32) -> Option<String> {
        self.controller.notice.clear();
        match self.controller_hit(x, y) {
            Hit::Mode => self.controller_toggle(),
            Hit::Sort => {
                self.controller.status_order = !self.controller.status_order;
            }
            Hit::Filter(f) => {
                self.controller.filter = f;
                self.clear_selection();
                self.controller_preselect();
                self.controller.details = false;
            }
            Hit::Details => {
                self.controller.details = !self.controller.details;
                self.controller.detail_age = 0;
            }
            Hit::Page(forward) => self.controller_page(forward),
            Hit::Session(id) => {
                self.controller.unread.remove(&id);
                self.focus_patron(id.clone());
                let url = self.session_target(&id).and_then(|t| t.url());
                self.controller.notice =
                    if url.is_some() { "OPEN REQUESTED" } else { "DETAILS ONLY - NO DESKTOP TARGET" }.into();
                return url;
            }
            Hit::None => (),
        }
        None
    }
    pub fn draw_controller(&self, fb: &mut [u32]) {
        let mut g = Gfx::new(fb);
        let bg = 0x171c27;
        let panel = 0x252d3d;
        let fg = 0xf3f3ed;
        let muted = 0xb9c5d5;
        let gold = 0xffd462;
        g.rect(0, 0, 480, 272, bg);
        g.ui_text("CAFE", 12, 14, gold, 2, None);
        for (i, label) in ["ALL", "CLAUDE", "CODEX", "NEEDS YOU"].iter().enumerate() {
            let x = 88 + i as i32 * 98;
            g.ui_text(label, x + 4, 17, fg, 1, None);
            if self.controller.filter == i as u32 {
                g.rect(x, 41, 98, 3, gold);
            }
        }
        let ids = self.controller_ids();
        let pos = self.selected_id().and_then(|id| ids.iter().position(|s| s == id)).unwrap_or(0);
        let page = pos / 6;
        for (i, id) in ids.iter().skip(page * 6).take(6).enumerate() {
            let x = 4 + (i % 2) as i32 * 238;
            let y = 48 + (i / 2) as i32 * 60;
            g.rect(x, y, 234, 56, panel);
            if self.selected_id() == Some(id.as_str()) {
                g.rect(x, y, 234, 2, gold);
                g.rect(x, y + 54, 234, 2, gold);
                g.rect(x, y, 2, 56, gold);
                g.rect(x + 232, y, 2, 56, gold);
            }
            let Some(s) = self.sessions().iter().find(|s| s.id == *id) else { continue };
            g.ui_text(&crate::controller_font::fit(&s.name, 202, 2), x + 7, y + 7, fg, 2, None);
            let app = if s.kind == Kind::Claude { "CLAUDE" } else { "CODEX" };
            g.ui_text(
                &shorten(&format!("{} / {}", app, s.branch.as_deref().unwrap_or("SESSION")), 34),
                x + 7,
                y + 30,
                muted,
                1,
                None,
            );
            if !self.controller_available(id) {
                g.ui_text("DETAILS ONLY", x + 7, y + 43, muted, 1, None);
            }
            let ix = x + 216;
            let iy = y + 19;
            if self.controller.disconnected.contains(id) {
                g.ui_text("X", ix, iy, muted, 2, None);
            } else {
                match s.state {
                    State::Work => {
                        for (dx, dy) in [(0, 0), (4, -2), (8, 0), (10, 4), (8, 8), (4, 10), (0, 8), (-2, 4)] {
                            g.rect(ix + dx, iy + dy, 2, 2, muted);
                        }
                    }
                    State::Wait => {
                        g.ui_text(if s.why == Why::Perm { "!" } else { "?" }, ix, iy, gold, 2, None);
                    }
                    State::Idle => {
                        if self.controller.unread.contains(id) {
                            for dy in -3..=3 {
                                for dx in -3..=3 {
                                    if dx * dx + dy * dy <= 9 {
                                        g.px(ix + 5 + dx, iy + 5 + dy, 0x58a6ff);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if ids.is_empty() {
            g.ui_text("NO MATCHING SESSIONS", 40, 120, muted, 2, None);
        }
        g.ui_text("PREV", 4, 239, fg, 1, None);
        g.ui_text("PAGE", 4, 252, fg, 1, None);
        g.ui_text("NEXT", 60, 239, fg, 1, None);
        g.ui_text("PAGE", 60, 252, fg, 1, None);
        let waits = self.sessions().iter().filter(|s| s.state == State::Wait).count();
        let works = self.sessions().iter().filter(|s| s.state == State::Work).count();
        g.ui_text(&format!("{} need you / {} working", waits, works), 120, 234, fg, 1, None);
        g.ui_text(&format!("Page {}/{}", page + 1, ids.len().div_ceil(6).max(1)), 120, 251, muted, 1, None);
        g.rect(300, 228, 116, 44, panel);
        g.ui_text(if self.controller.status_order { "Status order" } else { "Stable order" }, 306, 241, gold, 1, None);
        g.ui_text("DETAILS", 420, 244, fg, 1, None);
        if !self.controller.notice.is_empty() {
            g.rect(116, 247, 180, 25, bg);
            g.ui_text(&crate::controller_font::fit(&self.controller.notice, 174, 1), 120, 251, gold, 1, None);
        }
        if self.controller.details {
            g.rect(0, 44, 480, 228, bg);
            if let Some(id) = self.selected_id() {
                if let Some(s) = self.sessions().iter().find(|s| s.id == id) {
                    g.ui_text(&crate::controller_font::fit(&s.name, 450, 2), 12, 56, fg, 2, None);
                    g.ui_text(
                        match (s.state, s.why) {
                            (State::Work, _) => "WORKING",
                            (State::Wait, Why::Perm) => "WAITING FOR APPROVAL",
                            (State::Wait, _) => "WAITING FOR INPUT",
                            _ => "IDLE",
                        },
                        12,
                        82,
                        gold,
                        1,
                        None,
                    );
                }
                if let Some(d) = self.details().iter().find(|d| d.id == id) {
                    for (i, line) in crate::text::layout_prefix(&d.task, 70, 8).iter().enumerate() {
                        g.ui_text(&line.text, 12, 104 + i as i32 * 12, fg, 1, None);
                    }
                }
            }
            g.ui_text("BACK", 12, 242, fg, 2, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{ForcedPose, Session, SessionDetail, SessionTarget};
    fn scene() -> Scene {
        let mut s = Scene::new();
        let rows = (0..8)
            .map(|i| Session {
                id: format!("s{i}"),
                name: format!("Session {i}"),
                kind: if i % 2 == 0 { Kind::Claude } else { Kind::Codex },
                state: if i < 2 { State::Wait } else { State::Idle },
                why: Why::Ask,
                pose: ForcedPose::Auto,
                age_s: 0,
                branch: Some("main".into()),
                active_subagents: None,
            })
            .collect::<Vec<_>>();
        s.set_sessions(&rows);
        s.set_details(
            (0..7)
                .map(|i| SessionDetail {
                    unread: None,
                    id: format!("s{i}"),
                    task: "Task".into(),
                    target: Some(SessionTarget::Codex(format!("id-{i}"))),
                    output: None,
                })
                .collect(),
        );
        s.controller_toggle();
        s
    }
    #[test]
    fn knob_browses_and_click_opens_once() {
        let mut s = scene();
        assert_eq!(s.selected_id(), Some("s0"));
        assert_eq!(s.controller_move(true, false), Some("s1".into()));
        assert_eq!(s.controller_click(10, 55), Some("codex://threads/id-0".into()));
        assert_eq!(s.selected_id(), Some("s0"));
    }
    #[test]
    fn grid_hit_geometry_and_gaps() {
        let s = scene();
        assert_eq!(s.controller_hit(10, 55), Hit::Session("s0".into()));
        assert_eq!(s.controller_hit(250, 115), Hit::Session("s3".into()));
        assert_eq!(s.controller_hit(239, 55), Hit::None);
        assert_eq!(s.controller_hit(479, 227), Hit::None);
        assert_eq!(s.controller_hit(-1, 0), Hit::None);
    }
    #[test]
    fn filtering_preselects_without_navigation() {
        let mut s = scene();
        assert_eq!(s.controller_click(290, 20), None);
        assert_eq!(s.controller.filter, 2);
        assert_eq!(s.selected_id(), Some("s1"));
        assert_eq!(s.controller_ids().len(), 4);
    }
    #[test]
    fn direct_navigation_skips_details_only_and_wraps() {
        let mut s = scene();
        s.focus_patron("s6".into());
        assert_eq!(s.controller_move(true, true), Some("s0".into()));
        assert_eq!(s.controller_move(false, false), Some("s7".into()));
    }
    #[test]
    fn pages_clamp_at_edges() {
        let mut s = scene();
        s.controller_page(true);
        assert_eq!(s.selected_id(), Some("s6"));
        s.controller_page(true);
        assert_eq!(s.selected_id(), Some("s6"));
        s.controller_page(false);
        assert_eq!(s.selected_id(), Some("s0"));
    }
    #[test]
    fn empty_and_reverse_navigation() {
        let mut s = scene();
        s.clear_selection();
        assert_eq!(s.controller_move(false, false), Some("s7".into()));
        s.set_sessions(&[]);
        assert_eq!(s.controller_move(true, true), None);
        s.controller_page(true);
        let mut fb = alloc::vec![0;480*272];
        s.draw_controller(&mut fb);
    }
    #[test]
    fn resolved_wait_remains_until_cursor_moves() {
        let mut s = scene();
        s.controller.filter = 3;
        let mut rows = s.sessions().to_vec();
        rows[0].state = State::Idle;
        s.set_sessions(&rows);
        assert!(s.controller_ids().contains(&"s0".into()));
        s.controller_move(true, false);
        assert!(!s.controller_ids().contains(&"s0".into()));
    }
    #[test]
    fn details_expire_without_changing_mode_or_cursor() {
        let mut s = scene();
        s.controller_click(450, 245);
        assert!(s.controller.details);
        s.tick(29999);
        assert!(s.controller.details);
        s.tick(1);
        assert!(!s.controller.details);
        assert!(s.controller.enabled);
        assert_eq!(s.selected_id(), Some("s0"));
    }
    #[test]
    fn unread_requires_changed_public_output_and_survives_browsing() {
        let mut s = scene();
        let d = |text: &str| SessionDetail {
            unread: None,
            id: "s2".into(),
            task: "Task".into(),
            target: None,
            output: Some(text.into()),
        };
        s.set_details(alloc::vec![d("first")]);
        assert!(!s.controller.unread.contains("s2"));
        s.set_details(alloc::vec![d("second")]);
        assert!(s.controller.unread.contains("s2"));
        s.controller_move(true, false);
        assert!(s.controller.unread.contains("s2"));
        s.set_details(alloc::vec![d("second")]);
        assert!(s.controller.unread.contains("s2"));
    }
    #[test]
    fn native_frame_is_fully_painted() {
        let s = scene();
        let mut fb = alloc::vec![0xdeadbeef;480*272];
        s.draw_controller(&mut fb);
        assert!(!fb.contains(&0xdeadbeef));
    }
    #[test]
    fn provider_unread_is_visible_at_startup_and_clears_when_desktop_reads() {
        let mut s = scene();
        let detail = |unread, output: &str| SessionDetail {
            id: "s2".into(),
            task: "Task".into(),
            target: None,
            output: Some(output.into()),
            unread: Some(unread),
        };
        s.set_details(alloc::vec![detail(true, "initial output")]);
        let mut frame = alloc::vec![0;480*272];
        s.draw_controller(&mut frame);
        assert!(frame.contains(&0x58a6ff));
        s.controller_move(true, false);
        assert!(s.controller.unread.contains("s2"));
        s.set_details(alloc::vec![detail(false, "new output already viewed")]);
        assert!(!s.controller.unread.contains("s2"));
        s.draw_controller(&mut frame);
        assert!(!frame.contains(&0x58a6ff));
    }
    #[test]
    fn status_order_prioritizes_attention_output_work_and_idle_with_stable_ties() {
        let mut s = scene();
        let mut rows = s.sessions().to_vec();
        rows[0].state = State::Idle;
        rows[1].state = State::Work;
        rows[2].state = State::Wait;
        rows[3].state = State::Idle;
        s.set_sessions(&rows);
        s.controller.unread.insert("s0".into());
        s.controller.unread.insert("s2".into());
        s.controller.status_order = true;
        assert_eq!(s.controller_ids()[..4], ["s2", "s0", "s1", "s3"]);
        s.controller.disconnected.push("s3".into());
        assert_eq!(s.controller_ids().last().unwrap(), "s3");
        s.controller.status_order = false;
        assert_eq!(s.controller_ids()[..4], ["s0", "s1", "s2", "s3"]);
    }
    #[test]
    fn sort_toggle_preserves_cursor_and_navigation_uses_sorted_order() {
        let mut s = scene();
        s.controller.unread.insert("s6".into());
        s.focus_patron("s1".into());
        assert_eq!(s.controller_click(320, 245), None);
        assert!(s.controller.status_order);
        assert_eq!(s.selected_id(), Some("s1"));
        assert_eq!(s.controller_move(true, false), Some("s6".into()));
    }
    #[test]
    fn changing_status_during_touch_changes_the_identity_target() {
        let mut s = scene();
        s.controller.status_order = true;
        let before = s.controller_hit(10, 55);
        let mut rows = s.sessions().to_vec();
        rows[0].state = State::Idle;
        s.set_sessions(&rows);
        assert_ne!(before, s.controller_hit(10, 55));
        // Client touch release requires the exact same Hit; such a gesture cancels.
    }
}
