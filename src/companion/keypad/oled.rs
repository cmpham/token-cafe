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
    pub(super) fn rows(&mut self, model: &Model, now: u64) -> [[u8; 16]; 4] {
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
        if mask != 0 {
            return [row(&format!("@TC2A{mask}")), row(&self.alert_generation.to_string()), row(""), row("")];
        }
        if self.last_input.is_some_and(|at| now.saturating_sub(at) < IDLE_MS)
            && let Some(s) = model.sessions.get(&model.view.selected_session_id)
        {
            let name = if s.display_name.is_empty() { &s.title } else { &s.display_name };
            let second: String = name.chars().skip(16).take(16).collect();
            return [row("@TC2N"), row(if s.provider == 1 { "CLAUDE" } else { "CODEX" }), row(name), row(&second)];
        }
        let count =
            |provider| model.sessions.values().filter(|s| s.provider == provider && matches!(s.status, 1 | 3)).count();
        [row("@TC2I"), row(&count(1).to_string()), row(&count(2).to_string()), row("")]
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
    fn idle_counts_navigation_and_timeout() {
        let m = model();
        let mut o = Oled::default();
        let r = o.rows(&m, 0);
        assert_eq!(r[0], row("@TC2I"));
        assert_eq!(r[1], row("1"));
        assert_eq!(r[2], row("0"));
        o.input(&m, 100);
        let r = o.rows(&m, 101);
        assert_eq!(r[0], row("@TC2N"));
        assert_eq!(r[2], row("abcdefghijklmnop"));
        assert_eq!(r[3], row("SECOND"));
        assert_eq!(o.rows(&m, 5100)[0], row("@TC2I"));
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
        assert_eq!(o.rows(&m, 6000)[1], row("2"));
        assert_eq!(o.rows(&m, 6000)[2], row("2"));
        m.sessions.get_mut("1:1").unwrap().status = 2;
        m.sessions.get_mut("2:3").unwrap().status = 2;
        let rows = o.rows(&m, 6001);
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
        assert_eq!(o.rows(&m, 0)[0], row("@TC2A3"));
        o.input(&m, 10);
        assert_eq!(o.rows(&m, 11)[0], row("@TC2N"));
        assert_eq!(o.rows(&m, 6000)[0], row("@TC2I"));
        m.sessions.get_mut("b").unwrap().approval.as_mut().unwrap().digest = vec![2];
        assert_eq!(o.rows(&m, 6001)[0], row("@TC2A2"));
        m.sessions.get_mut("b").unwrap().approval.as_mut().unwrap().state = 3;
        assert_eq!(o.rows(&m, 6002)[0], row("@TC2I"));
        m.sessions.get_mut("a").unwrap().approval = None;
        o.rows(&m, 6003);
        let a = m.sessions.get_mut("a").unwrap();
        a.status = 3;
        a.waiting_reason = 2;
        assert_eq!(o.rows(&m, 6004)[0], row("@TC2A1"));
        o.input(&m, 6004);
        m.sessions.get_mut("a").unwrap().waiting_reason = 1;
        assert_eq!(o.rows(&m, 6005)[0], row("@TC2A1"));
    }
}
