//! OLED presentation policy. Transport and pixel rendering live outside this module.
use companion_core::model::Model;
use std::collections::BTreeSet;

type Alert = (String, String, Vec<u8>);
const IDLE_MS: u64 = 5_000;
#[derive(Default)]
pub(super) struct Oled {
    acknowledged: BTreeSet<Alert>,
    last_input: Option<u64>,
    observed: BTreeSet<Alert>,
    alert_generation: u64,
}
fn alert(s: &companion_core::wire::Session) -> Option<Alert> {
    if let Some(a) = &s.approval {
        return (a.state == 1).then(|| (s.id.clone(), a.id.clone(), a.digest.clone()));
    }
    // Questions and permission waits both require the user's attention.
    (s.status == 3).then(|| {
        (
            s.id.clone(),
            format!("wait:{}", s.waiting_reason),
            s.task_blob.as_ref().map_or_else(Vec::new, |b| b.sha256.clone()),
        )
    })
}
fn row(text: &str) -> [u8; 16] {
    let mut out = [b' '; 16];
    for (slot, c) in out.iter_mut().zip(text.chars()) {
        *slot = if c.is_ascii_graphic() || c == ' ' { c as u8 } else { b'?' };
    }
    out
}
impl Oled {
    pub(super) fn input(&mut self, model: &Model, now: u64) {
        self.last_input = Some(now);
        self.acknowledged = model.sessions.values().filter_map(alert).collect();
    }
    pub(super) fn rows(
        &mut self,
        model: &Model,
        now: u64,
        remaining: [Option<u8>; 3],
        dual_claude: bool,
    ) -> [[u8; 16]; 4] {
        if model.view.controller_mode {
            let filter =
                ["ALL", "CLAUDE", "CODEX", "WAIT"].get(model.view.controller_filter as usize).unwrap_or(&"ALL");
            let header = format!("CTRL {}{}", filter, if model.view.controller_status_order { "*" } else { "" });
            if self.last_input.is_some_and(|at| now.saturating_sub(at) < IDLE_MS)
                && let Some(s) = model.sessions.get(&model.view.selected_session_id)
            {
                let label = match (s.status, s.waiting_reason) {
                    (1, _) => "WORKING",
                    (3, 1) => "APPROVAL NEEDED",
                    (3, _) => "WAITING INPUT",
                    (4, _) => "DISCONNECTED",
                    _ => "IDLE",
                };
                return [row(&header), row(&s.title), row(s.branch.as_deref().unwrap_or("")), row(label)];
            }
            let count = |p| model.sessions.values().filter(|s| s.provider == p && matches!(s.status, 1 | 3)).count();
            let pct = |p: Option<u8>| p.map_or_else(|| "--".into(), |p| format!("{p}%"));
            return [
                row(&header),
                row(&format!("CLAUDE {} CODEX {}", count(1), count(2))),
                row(&format!("{} / {}", pct(remaining[0]), pct(remaining[1]))),
                row(&format!("CODEX {}", pct(remaining[2]))),
            ];
        }
        let current: BTreeSet<_> = model.sessions.values().filter_map(alert).collect();
        self.acknowledged.retain(|a| current.contains(a));
        if current != self.observed {
            self.alert_generation = self.alert_generation.wrapping_add(1);
            self.observed = current;
        }
        let mut mask = 0;
        for s in model.sessions.values() {
            if alert(s).is_some_and(|a| !self.acknowledged.contains(&a)) {
                mask |= match s.provider {
                    1 => 1,
                    2 => 2,
                    _ => 0,
                };
            }
        }
        if self.last_input.is_some_and(|at| now.saturating_sub(at) < IDLE_MS)
            && let Some(s) = model.sessions.get(&model.view.selected_session_id)
        {
            let name = if s.display_name.is_empty() { &s.title } else { &s.display_name };
            let second: String = name.chars().skip(16).take(16).collect();
            return [row("@TC2N"), row(if s.provider == 1 { "CLAUDE" } else { "CODEX" }), row(name), row(&second)];
        }
        if mask != 0 {
            return [row(&format!("@TC2A{mask}")), row(&self.alert_generation.to_string()), row(""), row("")];
        }
        let count =
            |provider| model.sessions.values().filter(|s| s.provider == provider && matches!(s.status, 1 | 3)).count();
        {
            let percent = |value: Option<u8>| value.map_or_else(|| "--".into(), |v| format!("{v}%"));
            if dual_claude || remaining[1].is_some() {
                return [
                    row("@TC4I"),
                    row(&count(1).to_string()),
                    row(&count(2).to_string()),
                    row(&format!(
                        "{:<4}{:<4}{:<4}",
                        percent(remaining[0]),
                        percent(remaining[1]),
                        percent(remaining[2])
                    )),
                ];
            }
            [
                row("@TC3I"),
                row(&count(1).to_string()),
                row(&count(2).to_string()),
                row(&format!("{:<8}{:<8}", percent(remaining[0]), percent(remaining[2]))),
            ]
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use companion_core::wire::{Request, Session};
    fn model() -> Model {
        let mut m = Model::default();
        for (id, provider, status) in [("a", 1, 1), ("b", 2, 2), ("c", 2, 4)] {
            m.sessions.insert(
                id.into(),
                Session {
                    id: id.into(),
                    provider,
                    status,
                    display_name: "abcdefghijklmnopSECOND".into(),
                    ..Default::default()
                },
            );
        }
        m.view.selected_session_id = "a".into();
        m
    }
    #[test]
    fn controller_mode_keeps_filter_and_dual_quotas_visible() {
        let mut m = model();
        m.view.controller_mode = true;
        m.view.controller_filter = 2;
        let mut o = Oled::default();
        let rows = o.rows(&m, 100, [Some(29), Some(64), Some(42)], true);
        assert_eq!(rows[0], row("CTRL CODEX"));
        assert_eq!(rows[2], row("29% / 64%"));
        assert_eq!(rows[3], row("CODEX 42%"));
        o.input(&m, 200);
        let nav = o.rows(&m, 201, [None; 3], true);
        assert_eq!(nav[0], row("CTRL CODEX"));
        let idle = o.rows(&m, 6000, [Some(29), Some(64), Some(42)], true);
        assert_eq!(idle[2], row("29% / 64%"));
    }
    #[test]
    fn idle_counts_navigation_and_timeout() {
        let m = model();
        let mut o = Oled::default();
        let r = o.rows(&m, 0, [None; 3], false);
        assert_eq!(r[0], row("@TC3I"));
        assert_eq!(r[1], row("1"));
        assert_eq!(r[2], row("0"));
        o.input(&m, 100);
        let r = o.rows(&m, 101, [None; 3], false);
        assert_eq!(r[0], row("@TC2N"));
        assert_eq!(r[2], row("abcdefghijklmnop"));
        assert_eq!(r[3], row("SECOND"));
        assert_eq!(o.rows(&m, 5100, [None; 3], false)[0], row("@TC3I"));
    }
    #[test]
    fn idle_weekly_values_fit_both_columns() {
        let mut o = Oled::default();
        let rows = o.rows(&model(), 0, [Some(100), None, Some(29)], false);
        assert_eq!(rows[3], row("100%    29%     "));
        assert_eq!(o.rows(&model(), 0, [None, None, Some(0)], false)[3], row("--      0%      "));
    }
    #[test]
    fn separate_claude_percentages_have_fixed_slots() {
        let mut oled = Oled::default();
        let rows = oled.rows(&model(), 0, [Some(95), Some(100), Some(29)], false);
        assert_eq!(rows[0], row("@TC4I"));
        assert_eq!(rows[3], row("95% 100%29% "));
        assert_eq!(oled.rows(&model(), 0, [None, Some(0), None], false)[3], row("--  0%  --  "));
        assert_eq!(oled.rows(&model(), 0, [Some(95), None, Some(29)], false)[0], row("@TC3I"));
    }
    #[test]
    fn known_dual_account_layout_survives_unavailable_percentages() {
        let mut oled = Oled::default();
        let rows = oled.rows(&model(), 0, [None, None, Some(29)], true);
        assert_eq!(rows[0], row("@TC4I"));
        assert_eq!(rows[3], row("--  --  29% "));
    }
    #[test]
    fn new_alert_cannot_hide_selection_while_navigating() {
        let mut m = model();
        let mut oled = Oled::default();
        oled.input(&m, 100);
        m.sessions.get_mut("b").unwrap().status = 3;
        assert_eq!(oled.rows(&m, 101, [None; 3], false)[0], row("@TC2N"));
        assert_eq!(oled.rows(&m, 5099, [None; 3], false)[2], row("abcdefghijklmnop"));
        assert_eq!(oled.rows(&m, 5100, [None; 3], false)[0], row("@TC2A2"));
        oled.input(&m, 5101);
        assert_eq!(oled.rows(&m, 5102, [None; 3], false)[0], row("@TC2N"));
    }

    #[test]
    fn counts_working_and_waiting_for_each_provider_including_unseated() {
        let mut m = Model::default();
        for provider in [1, 2] {
            for status in 1..=4 {
                let id = format!("{provider}:{status}");
                m.sessions.insert(id.clone(), Session { id, provider, status, seat: 255, ..Default::default() });
            }
        }
        let mut o = Oled::default();
        o.input(&m, 0); // Silence waiting alerts before checking idle totals.
        assert_eq!(o.rows(&m, 6000, [None; 3], false)[1], row("2"));
        assert_eq!(o.rows(&m, 6000, [None; 3], false)[2], row("2"));
        m.sessions.get_mut("1:1").unwrap().status = 2;
        m.sessions.get_mut("2:3").unwrap().status = 2;
        let rows = o.rows(&m, 6001, [None; 3], false);
        assert_eq!(rows[1], row("1"));
        assert_eq!(rows[2], row("1"));
    }
    #[test]
    fn alerts_acknowledge_all_existing_but_rearm_for_new_requests() {
        let mut m = model();
        let mut o = Oled::default();
        for id in ["a", "b"] {
            m.sessions.get_mut(id).unwrap().approval =
                Some(Request { id: id.into(), state: 1, digest: vec![1], ..Default::default() });
        }
        assert_eq!(o.rows(&m, 0, [None; 3], false)[0], row("@TC2A3"));
        o.input(&m, 10);
        assert_eq!(o.rows(&m, 11, [None; 3], false)[0], row("@TC2N"));
        assert_eq!(o.rows(&m, 6000, [None; 3], false)[0], row("@TC3I"));
        m.sessions.get_mut("b").unwrap().approval.as_mut().unwrap().digest = vec![2];
        assert_eq!(o.rows(&m, 6001, [None; 3], false)[0], row("@TC2A2"));
        m.sessions.get_mut("b").unwrap().approval.as_mut().unwrap().state = 3;
        assert_eq!(o.rows(&m, 6002, [None; 3], false)[0], row("@TC3I"));
        m.sessions.get_mut("a").unwrap().approval = None;
        o.rows(&m, 6003, [None; 3], false);
        let a = m.sessions.get_mut("a").unwrap();
        a.status = 3;
        a.waiting_reason = 2;
        assert_eq!(o.rows(&m, 6004, [None; 3], false)[0], row("@TC2A1"));
        o.input(&m, 6004);
        m.sessions.get_mut("a").unwrap().waiting_reason = 1;
        assert_eq!(o.rows(&m, 6005, [None; 3], false)[0], row("@TC2N"));
        assert_eq!(o.rows(&m, 11004, [None; 3], false)[0], row("@TC2A1"));
    }
}
