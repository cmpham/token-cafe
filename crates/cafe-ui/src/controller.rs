//! Native 480x272 session controller shared by desktop and board.
use crate::controller_design::{self as design, Rect, StatusIcon, Text, layout};
use crate::{
    gfx::Gfx,
    scene::{Kind, Scene, State},
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
    pub notice_age: u32,
    pub outputs: BTreeMap<String, String>,
    pub unread: BTreeSet<String>,
    pub filter: u32,
    pub details: bool,
    pub detail_age: u32,
    pub available: BTreeMap<String, bool>,
    pub disconnected: Vec<String>,
}
impl Controller {
    pub fn set_notice(&mut self, text: String) {
        self.notice = text;
        self.notice_age = 0;
    }
    pub fn notice_is_error(&self) -> bool {
        ["COULD NOT", "NO DESKTOP", "DETAILS ONLY", "OFFLINE", "DESKTOP NAVIGATION"]
            .iter()
            .any(|prefix| self.notice.starts_with(prefix))
    }
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
        if !Rect::new(0, 0, layout::WIDTH, layout::HEIGHT).contains(x, y) || !self.controller.enabled {
            return Hit::None;
        }
        if layout::MODE.contains(x, y) {
            return Hit::Mode;
        }
        for i in 0..4 {
            if layout::filter(i).contains(x, y) {
                return Hit::Filter(i as u32);
            }
        }
        if self.controller.details {
            return if y >= layout::FOOTER_Y { Hit::Details } else { Hit::None };
        }
        let ids = self.controller_ids();
        let page = self.selected_id().and_then(|id| ids.iter().position(|s| s == id)).unwrap_or(0) / layout::PAGE_SIZE;
        for (rect, hit, enabled) in [
            (layout::PREV, Hit::Page(false), page > 0),
            (layout::NEXT, Hit::Page(true), (page + 1) * layout::PAGE_SIZE < ids.len()),
            (layout::ORDER, Hit::Sort, true),
            (layout::DETAILS, Hit::Details, self.selected_id().is_some()),
        ] {
            if rect.contains(x, y) {
                return if enabled { hit } else { Hit::None };
            }
        }
        let ids = self.controller_ids();
        let page = self.selected_id().and_then(|id| ids.iter().position(|s| s == id)).unwrap_or(0) / layout::PAGE_SIZE;
        for i in 0..layout::PAGE_SIZE {
            if layout::tile(i).contains(x, y) {
                return ids.get(page * layout::PAGE_SIZE + i).cloned().map_or(Hit::None, Hit::Session);
            }
        }
        Hit::None
    }
    pub fn controller_page(&mut self, forward: bool) {
        let ids = self.controller_ids();
        if ids.is_empty() {
            return;
        }
        let page = self.selected_id().and_then(|id| ids.iter().position(|s| s == id)).unwrap_or(0) / layout::PAGE_SIZE;
        let target = if forward { (page + 1).min((ids.len() - 1) / layout::PAGE_SIZE) } else { page.saturating_sub(1) };
        if target == page {
            return;
        }
        self.focus_patron(ids[target * layout::PAGE_SIZE].clone());
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
                self.controller.set_notice(
                    if url.is_some() { "OPEN REQUESTED" } else { "DETAILS ONLY - NO DESKTOP TARGET" }.into(),
                );
                return url;
            }
            Hit::None => (),
        }
        None
    }
    pub fn draw_controller(&self, fb: &mut [u32]) {
        let mut g = Gfx::new(fb);
        let bg = design::color::CANVAS;
        let fg = design::color::TEXT;
        let muted = design::color::SECONDARY;
        let gold = design::color::FOCUS;
        Rect::new(0, 0, layout::WIDTH, layout::HEIGHT).paint(&mut g, bg);
        design::button(&mut g, layout::MODE, "< Cafe", true, false);
        for (i, label) in ["All", "Claude", "Codex", "Needs you"].iter().enumerate() {
            design::tab(&mut g, layout::filter(i), label, self.controller.filter == i as u32);
        }
        let ids = self.controller_ids();
        let pos = self.selected_id().and_then(|id| ids.iter().position(|s| s == id)).unwrap_or(0);
        let page = pos / layout::PAGE_SIZE;
        for (i, id) in ids.iter().skip(page * layout::PAGE_SIZE).take(layout::PAGE_SIZE).enumerate() {
            let bounds = layout::tile(i);
            let (x, y) = (bounds.x, bounds.y);
            design::tile(&mut g, bounds, self.selected_id() == Some(id.as_str()));
            let Some(s) = self.sessions().iter().find(|s| s.id == *id) else { continue };
            design::label(&mut g, &s.name, Rect::new(x + 7, y + 7, layout::TITLE_WIDTH, 22), Text::Title, fg);
            let app = if s.kind == Kind::Claude { "CLAUDE" } else { "CODEX" };
            design::label(
                &mut g,
                &format!("{} / {}", app, s.branch.as_deref().unwrap_or("SESSION")),
                Rect::new(x + 7, y + 30, layout::TITLE_WIDTH, 22),
                Text::Caption,
                muted,
            );
            if !self.controller_available(id) {
                design::label(
                    &mut g,
                    "DETAILS ONLY",
                    Rect::new(x + 7, y + 43, layout::TITLE_WIDTH, 22),
                    Text::Caption,
                    muted,
                );
            }
            StatusIcon::resolve(
                s.state,
                s.why,
                self.controller.unread.contains(id),
                self.controller.disconnected.iter().any(|offline| offline == id),
            )
            .draw_at(&mut g, x + 216, y + 19, self.animation_ms());
        }
        if ids.is_empty() {
            design::label(
                &mut g,
                "NO MATCHING SESSIONS",
                Rect::new(40, 120, layout::WIDTH - (40), 22),
                Text::Title,
                muted,
            );
        }
        design::page_button(&mut g, layout::PREV, false, page > 0);
        design::page_button(&mut g, layout::NEXT, true, (page + 1) * layout::PAGE_SIZE < ids.len());
        let waits = self.sessions().iter().filter(|s| s.state == State::Wait).count();
        let works = self.sessions().iter().filter(|s| s.state == State::Work).count();
        design::label(
            &mut g,
            &format!("All: {} need you / {} working", waits, works),
            Rect::new(120, 234, layout::SUMMARY.w - 12, 22),
            Text::Caption,
            fg,
        );
        design::label(
            &mut g,
            &format!("Page {}/{}", page + 1, ids.len().div_ceil(layout::PAGE_SIZE).max(1)),
            Rect::new(120, 251, layout::NOTICE_WIDTH, 22),
            Text::Caption,
            muted,
        );
        design::option(
            &mut g,
            layout::ORDER,
            if self.controller.status_order { "Status order" } else { "Stable order" },
        );
        design::button(&mut g, layout::DETAILS, "Details", self.selected_id().is_some(), false);
        if !self.controller.notice.is_empty() {
            design::notice(&mut g, &self.controller.notice);
        }
        if self.controller.details {
            layout::DETAIL_BODY.paint(&mut g, bg);
            if let Some(id) = self.selected_id() {
                if let Some(s) = self.sessions().iter().find(|s| s.id == id) {
                    design::label(&mut g, &s.name, Rect::new(12, 56, 450, 22), Text::Title, fg);
                    design::label(
                        &mut g,
                        StatusIcon::resolve(
                            s.state,
                            s.why,
                            self.controller.unread.contains(id),
                            self.controller.disconnected.iter().any(|offline| offline == id),
                        )
                        .label(),
                        Rect::new(12, 82, layout::WIDTH - (12), 22),
                        Text::Control,
                        gold,
                    );
                }
                if let Some(d) = self.details().iter().find(|d| d.id == id) {
                    let wrapped = design::wrap(&d.task, 450, Text::Control, 7);
                    for (i, line) in wrapped.lines.iter().enumerate() {
                        design::label(&mut g, line, Rect::new(12, 104 + i as i32 * 16, 450, 22), Text::Control, fg);
                    }
                    if wrapped.truncated {
                        design::label(&mut g, "More in desktop app", Rect::new(280, 82, 188, 22), Text::Caption, muted);
                    }
                }
            }
            design::button(&mut g, Rect::new(0, layout::FOOTER_Y, 480, 44), "Back to sessions", true, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{ForcedPose, Session, SessionDetail, SessionTarget, Why};
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
    #[test]
    fn disabled_page_and_details_controls_are_inert() {
        let mut s = scene();
        s.focus_patron("s3".into());
        assert_eq!(s.controller_hit(20, 245), Hit::None);
        s.controller_page(false);
        assert_eq!(s.selected_id(), Some("s3"));
        s.controller_page(true);
        s.focus_patron("s7".into());
        assert_eq!(s.controller_hit(75, 245), Hit::None);
        s.controller_page(true);
        assert_eq!(s.selected_id(), Some("s7"));
        s.clear_selection();
        assert_eq!(s.controller_hit(450, 245), Hit::None);
    }
    #[test]
    fn requested_notices_expire_but_failures_wait_for_navigation() {
        let mut s = scene();
        s.controller.set_notice("OPEN REQUESTED".into());
        s.tick(4999);
        assert!(!s.controller.notice.is_empty());
        s.tick(1);
        assert!(s.controller.notice.is_empty());
        s.controller.set_notice("COULD NOT OPEN - RETRY".into());
        s.tick(60000);
        assert!(!s.controller.notice.is_empty());
        s.controller_click(320, 245);
        assert!(s.controller.notice.is_empty());
    }
}
