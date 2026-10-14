//! Desktop-to-wire model mapping. Provider session URLs never cross the radio.
use crate::{
    approval::{Broker, Decision, Outcome},
    scene::{self, Scene},
};
use companion_core::{
    authority::Authority,
    crypto,
    model::{Model, allocate},
    sync,
    wire::{BlobRef, Request, Session, View},
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
pub struct Store {
    pub authority: Authority,
    pub epoch: [u8; 16],
    pub broker: Option<Broker>,
    pub targets: BTreeMap<String, String>,
    blobs: BTreeMap<Vec<u8>, (Vec<u8>, Instant)>,
    arrival: Vec<String>,
    terminal: BTreeMap<String, (Request, Instant)>,
    pub last_desktop: Option<View>,
}
fn bounded(s: &str, max: usize) -> String {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].into()
}
pub fn wire_id(s: &str) -> String {
    if s.len() <= 128 { s.into() } else { format!("id:{}", crypto::hex(&crypto::hash(s.as_bytes()))) }
}
fn appearance(s: &str) -> u32 {
    s.bytes().fold(2166136261u32, |h, c| (h ^ (c as u32)).wrapping_mul(16777619))
}
/// Move a submitting request to its final wire state; true when the broker had an outcome.
fn settle(r: &mut Request, outcome: Option<Outcome>) -> bool {
    let (state, decision) = match outcome {
        Some(Outcome::Delivered(d)) => (3, if d == Decision::Allow { 1 } else { 2 }),
        Some(Outcome::Failed(_)) => (7, 0),
        Some(Outcome::Closed) => (6, 0),
        None => return false,
    };
    r.state = state;
    r.delivered_decision = decision;
    true
}
impl Store {
    pub fn new(epoch: [u8; 16]) -> Self {
        Self {
            authority: Authority {
                model: Model { revision: 1, view: View { page: 1, ..Default::default() }, ..Default::default() },
            },
            epoch,
            broker: None,
            targets: BTreeMap::new(),
            blobs: BTreeMap::new(),
            arrival: vec![],
            terminal: BTreeMap::new(),
            last_desktop: None,
        }
    }
    pub fn body(&self, sha: &[u8]) -> Option<&[u8]> {
        self.blobs.get(sha).map(|(b, _)| b.as_slice())
    }
    fn blob(&mut self, text: &str) -> BlobRef {
        let text = bounded(text, 32768);
        let b = sync::blob(text.as_bytes()).expect("bounded UTF8");
        self.blobs.insert(b.sha256.clone(), (text.into_bytes(), Instant::now()));
        b
    }
    pub fn keep_terminal(&mut self, session: &str) {
        if let Some(r) = self.authority.model.sessions.get(session).and_then(|s| s.approval.as_ref()) {
            self.terminal.insert(session.into(), (r.clone(), Instant::now()));
        }
    }
    pub fn ingest(&mut self, scene: &mut Scene, broker: Option<&Broker>) {
        self.broker = broker.cloned();
        let old = self.authority.model.clone();
        let mut next = old.clone();
        let mut sessions = vec![];
        self.targets.clear();
        if let Some(u) = scene.usage() {
            next.usage = companion_core::wire::Usage {
                tokens_display: scene::fmt_tokens(u.tokens),
                cost_display: scene::fmt_usd(u.usd),
            };
        }
        let input = scene.sessions().to_vec();
        let details = scene.details().to_vec();
        let requests = scene.requests().to_vec();
        self.terminal.retain(|_, (r, t)| t.elapsed() < Duration::from_secs(if r.state == 2 { 30 } else { 5 }));
        // Desktop decisions bypass the authority; the broker's record finishes them.
        for (r, at) in self.terminal.values_mut() {
            let outcome = self.broker.as_ref().and_then(|b| b.outcome(&r.id));
            if r.state == 2 && settle(r, outcome) {
                *at = Instant::now();
            }
        }
        for s in input.iter().take(64) {
            let id = wire_id(&s.id);
            if !self.arrival.contains(&id) {
                self.arrival.push(id.clone());
            }
            let detail = details.iter().find(|d| d.id == s.id);
            let target = detail.and_then(|d| d.target.as_ref()).and_then(scene::SessionTarget::url);
            if let Some(url) = &target {
                self.targets.insert(id.clone(), url.clone());
            }
            let task = detail.map(|d| self.blob(&d.task));
            let live = requests.iter().find(|r| r.session == s.id);
            let settled = self.terminal.get(&id).map(|(r, _)| r.clone());
            let approval = match (settled, live) {
                // A finished request yields to the next live one only after boards could see it
                // finish (receipts reconcile on that state); a submitting request never yields.
                (Some(t), live)
                    if t.state == 2
                        || live.is_none_or(|r| r.id == t.id)
                        || !old
                            .sessions
                            .get(&id)
                            .and_then(|o| o.approval.as_ref())
                            .is_some_and(|p| p.id == t.id && p.state == t.state) =>
                {
                    Some(t)
                }
                (_, None) => {
                    old.sessions.get(&id).and_then(|o| o.approval.clone()).filter(|r| matches!(r.state, 1 | 2)).map(
                        |mut r| {
                            r.state = 2;
                            let outcome = self.broker.as_ref().and_then(|b| b.outcome(&r.id));
                            settle(&mut r, outcome);
                            self.terminal.insert(id.clone(), (r.clone(), Instant::now()));
                            r
                        },
                    )
                }
                (_, Some(r)) => Some({
                    let body = self.blob(&r.text);
                    let title = bounded(&r.title, 192);
                    let tool_name = bounded(&r.tool_name, 192);
                    let reason =
                        if r.title.len() > 192 || r.tool_name.len() > 192 || !cafe_ui::review::Review::identity_fits(r)
                        {
                            "REQUEST IDENTITY TOO LONG"
                        } else if r.text.len() > 32768 {
                            "REQUEST TOO LONG"
                        } else {
                            ""
                        };
                    let decisions = if reason.is_empty() { vec![1, 2] } else { vec![] };
                    let digest = crypto::request_digest(
                        &r.id,
                        &id,
                        1,
                        &title,
                        &tool_name,
                        &body.sha256.as_slice().try_into().unwrap(),
                        body.byte_length,
                        &decisions.iter().map(|x| *x as u8).collect::<Vec<_>>(),
                    )
                    .unwrap()
                    .to_vec();
                    Request {
                        id: r.id.clone(),
                        digest,
                        title,
                        tool_name,
                        body_blob: Some(body),
                        state: 1,
                        allowed_decisions: decisions,
                        desktop_only_reason: reason.into(),
                        delivered_decision: 0,
                    }
                }),
            };
            let unlinked = if approval.is_none() && s.state == scene::State::Wait {
                Some(self.blob(&format!("{}\n\nThis session needs your input in its desktop app. No linked approval request is available. Open Desktop to read and answer the exact request.",s.name)))
            } else {
                None
            };
            let status = match s.state {
                scene::State::Work => 1,
                scene::State::Idle => 2,
                scene::State::Wait => 3,
            };
            let age = s.age_s as u64 * 1000;
            // Existing clients advance their own animation clock. Refresh age
            // periodically for newly connected clients, not every collector tick.
            let state_age_ms = old
                .sessions
                .get(&id)
                .filter(|previous| {
                    previous.status == status && age >= previous.state_age_ms && age - previous.state_age_ms < 30_000
                })
                .map_or(age, |previous| previous.state_age_ms);
            sessions.push(Session {
                id,
                provider: if s.kind == scene::Kind::Claude { 1 } else { 2 },
                title: bounded(&s.name, 192),
                display_name: bounded(&s.name, 192),
                status: match s.state {
                    scene::State::Work => 1,
                    scene::State::Idle => 2,
                    scene::State::Wait => 3,
                },
                waiting_reason: match s.why {
                    scene::Why::None => 0,
                    scene::Why::Perm => 1,
                    scene::Why::Ask => 2,
                },
                seat: 255,
                appearance_seed: appearance(&s.id),
                state_age_ms,
                task_blob: task,
                task_truncated: detail.is_some_and(|d| d.task.len() > 32768),
                navigation_available: cfg!(target_os = "macos") && target.is_some(),
                approval,
                branch: s.branch.as_deref().map(|s| bounded(s, 192)),
                active_subagents: s.active_subagents.map(|n| n as u32),
                unlinked_review_blob: unlinked,
            });
        }
        self.arrival.retain(|id| sessions.iter().any(|s| &s.id == id));
        allocate(&old, &mut sessions, &self.arrival);
        next.overflow_count = (input.len().saturating_sub(8)) as u32;
        next.sessions = sessions.into_iter().map(|s| (s.id.clone(), s)).collect();
        // Desktop interactions become authoritative view updates only if the UI
        // actually changed since the previous synchronization.
        let mut desktop = View {
            selected_session_id: scene.selected_id().map(wire_id).unwrap_or_default(),
            page: 1,
            task_anchor: scene.task_anchor() as u32,
            ..Default::default()
        };
        if let Some(r) = &scene.review
            && let Some(s) = next.sessions.get(&wire_id(&r.request.session))
        {
            let req = s.approval.as_ref().filter(|a| a.id == r.request.id);
            let (kind, b) = if let Some(req) = req {
                (1, req.body_blob.clone())
            } else if s.unlinked_review_blob.is_some() {
                (3, s.unlinked_review_blob.clone())
            } else {
                (2, s.task_blob.clone())
            };
            desktop.selected_session_id = s.id.clone();
            desktop.page = 2;
            desktop.review_kind = kind;
            desktop.review_request_id = req.map_or_else(String::new, |r| r.id.clone());
            desktop.review_blob = b;
            desktop.review_anchor = r.anchor() as u32;
        }
        if self.last_desktop.as_ref().is_some_and(|last| *last != desktop) {
            next.view = desktop.clone();
            next.view.generation = old.view.generation + 1;
            next.view.origin_device_id = "desktop".into();
        }
        if !next.view.selected_session_id.is_empty() && !next.sessions.contains_key(&next.view.selected_session_id) {
            next.view = View { page: 1, generation: old.view.generation + 1, ..Default::default() };
        }
        if next.view.page == 2 {
            let s = next.sessions.get(&next.view.selected_session_id);
            let valid = s.is_some_and(|s| match next.view.review_kind {
                1 => s
                    .approval
                    .as_ref()
                    .is_some_and(|r| r.id == next.view.review_request_id && r.body_blob == next.view.review_blob),
                2 => s.task_blob == next.view.review_blob,
                3 => s.unlinked_review_blob == next.view.review_blob,
                _ => false,
            });
            if !valid {
                next.view.page = 1;
                next.view.review_kind = 0;
                next.view.review_request_id.clear();
                next.view.review_blob = None;
                next.view.review_anchor = 0;
            }
        }
        if let Some(s) = next.sessions.get(&next.view.selected_session_id)
            && old.sessions.get(&s.id).is_some_and(|o| o.task_blob != s.task_blob)
        {
            next.view.task_anchor = 0;
        }
        if next != old {
            next.revision = old.revision + 1;
            if companion_core::model::validate(&next).is_ok() {
                self.authority.model = next;
            } else {
                eprintln!("BLE v2: model rejected; retaining previous state");
            }
        }
        self.apply(scene);
        self.last_desktop = Some(self.desktop_view(scene));
        let now = Instant::now();
        let refs: Vec<Vec<u8>> = self
            .authority
            .model
            .sessions
            .values()
            .flat_map(|s| {
                [
                    s.task_blob.as_ref(),
                    s.unlinked_review_blob.as_ref(),
                    s.approval.as_ref().and_then(|r| r.body_blob.as_ref()),
                ]
            })
            .flatten()
            .map(|b| b.sha256.clone())
            .collect();
        // Bodies the published model still references stay fetchable (a settled request's text).
        self.blobs.retain(|k, (_, t)| refs.contains(k) || now.duration_since(*t) < Duration::from_secs(30));
        // Bound cached historical bodies; live references remain retained.
        while self.blobs.values().map(|(b, _)| b.len()).sum::<usize>() > 512 * 1024 {
            let oldest = self
                .blobs
                .iter()
                .filter(|(k, _)| !refs.contains(k))
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                self.blobs.remove(&k);
            } else {
                break;
            }
        }
    }
    fn desktop_view(&self, scene: &Scene) -> View {
        let mut v = self.authority.model.view.clone();
        v.generation = 0;
        v.origin_device_id.clear();
        v.origin_connection_epoch.clear();
        v.applied_action_id = 0;
        v.selected_session_id = scene.selected_id().map(wire_id).unwrap_or_default();
        v.task_anchor = scene.task_anchor() as u32;
        if let Some(r) = &scene.review {
            v.review_anchor = r.anchor() as u32;
        }
        v
    }
    fn apply(&self, scene: &mut Scene) {
        let m = &self.authority.model;
        let ids: Vec<_> = scene.sessions().iter().map(|s| (wire_id(&s.id), s.id.clone())).collect();
        let real = |id: &str| ids.iter().find(|(wire, _)| wire == id).map(|(_, r)| r.clone());
        let ss = scene.sessions().to_vec();
        let seats = m.sessions.values().filter_map(|s| real(&s.id).map(|id| (id, s.seat as usize))).collect::<Vec<_>>();
        scene.set_authoritative_sessions(&ss, &seats);
        let v = &m.view;
        if let Some(id) = real(&v.selected_session_id) {
            scene.focus_patron(id);
        } else {
            scene.clear_selection();
        }
        scene.set_task_anchor(v.task_anchor as usize);
        if v.page == 2 {
            if let Some(s) = m.sessions.get(&v.selected_session_id) {
                let r = s.approval.as_ref().filter(|_| v.review_kind == 1);
                let text = v
                    .review_blob
                    .as_ref()
                    .and_then(|b| self.body(&b.sha256))
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or("TEXT UNAVAILABLE - OPEN DESKTOP");
                let request = crate::approval::Request {
                    id: r.map_or_else(String::new, |r| r.id.clone()),
                    session: real(&s.id).unwrap_or_else(|| s.id.clone()),
                    title: r.map_or_else(|| s.title.clone(), |r| r.title.clone()),
                    tool_name: r.map_or_else(String::new, |r| r.tool_name.clone()),
                    text: text.into(),
                };
                if scene.review.as_ref().is_none_or(|old| old.request != request) {
                    scene.review = Some(crate::review::Review::new(request, false));
                }
                if let Some(review) = &mut scene.review {
                    review.set_anchor(v.review_anchor as usize);
                    review.actionable = r.is_some_and(|r| r.state == 1 && r.desktop_only_reason.is_empty());
                    review.status = r
                        .map_or("READ ONLY - OPEN DESKTOP", |r| match r.state {
                            1 => {
                                if review.actionable {
                                    ""
                                } else {
                                    "ANSWER IN DESKTOP"
                                }
                            }
                            2 => "SENDING DECISION...",
                            3 => "SENT TO CLAUDE",
                            _ => "CHECK DESKTOP",
                        })
                        .into();
                }
            }
        } else {
            scene.review = None;
        }
    }
    pub fn decide(&self, request: &str, decision: i32) -> bool {
        self.broker
            .as_ref()
            .is_some_and(|b| b.decide(request, if decision == 1 { Decision::Allow } else { Decision::Deny }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use companion_core::authority::Peer;
    fn scene() -> Scene {
        let mut scene = Scene::new();
        scene.set_sessions(&[scene::Session {
            id: "claude:a".into(),
            name: "Alpha".into(),
            kind: scene::Kind::Claude,
            state: scene::State::Work,
            why: scene::Why::None,
            pose: scene::ForcedPose::Auto,
            age_s: 1,
            branch: None,
            active_subagents: None,
        }]);
        scene.set_details(vec![scene::SessionDetail { id: "claude:a".into(), target: None, task: "Full task".into() }]);
        scene
    }
    #[test]
    fn remote_selection_survives_desktop_collection_tick() {
        let mut scene = scene();
        let mut s = Store::new([1; 16]);
        s.ingest(&mut scene, None);
        let mut p = Peer::new("qa".into(), vec![2; 16], false, false);
        let (r, _) = s
            .authority
            .action(
                &mut p,
                &companion_core::wire::Action { id: 1, kind: 1, session_id: "claude:a".into(), ..Default::default() },
                0,
                None,
            )
            .unwrap();
        assert_eq!(r.status, 1);
        s.ingest(&mut scene, None);
        assert_eq!(scene.selected_id(), Some("claude:a"));
        s.ingest(&mut scene, None);
        assert_eq!(s.authority.model.view.selected_session_id, "claude:a");
        scene.clear_selection();
        s.ingest(&mut scene, None);
        assert!(s.authority.model.view.selected_session_id.is_empty());
    }
    #[test]
    fn oversize_request_is_read_only_and_oldest_request_wins() {
        let mut scene = scene();
        let request = crate::approval::Request {
            id: "first".into(),
            session: "claude:a".into(),
            title: "ALLOW Bash?".into(),
            tool_name: "Bash".into(),
            text: "x".repeat(32769),
        };
        let mut second = request.clone();
        second.id = "second".into();
        scene.set_requests(vec![request, second]);
        let mut s = Store::new([1; 16]);
        s.ingest(&mut scene, None);
        let r = s.authority.model.sessions["claude:a"].approval.as_ref().unwrap();
        assert_eq!(r.id, "first");
        assert_eq!(r.desktop_only_reason, "REQUEST TOO LONG");
        assert!(r.allowed_decisions.is_empty());
    }
    #[test]
    fn age_ticks_do_not_flood_the_wire() {
        let mut scene = scene();
        let mut store = Store::new([1; 16]);
        store.ingest(&mut scene, None);
        let revision = store.authority.model.revision;
        let mut sessions = scene.sessions().to_vec();
        sessions[0].age_s = 2;
        scene.set_sessions(&sessions);
        store.ingest(&mut scene, None);
        assert_eq!(store.authority.model.revision, revision);
        sessions[0].age_s = 31;
        scene.set_sessions(&sessions);
        store.ingest(&mut scene, None);
        assert_eq!(store.authority.model.sessions["claude:a"].state_age_ms, 31000);
        assert!(store.authority.model.revision > revision);
        sessions[0].state = scene::State::Wait;
        sessions[0].age_s = 0;
        scene.set_sessions(&sessions);
        store.ingest(&mut scene, None);
        assert_eq!(store.authority.model.sessions["claude:a"].state_age_ms, 0);
    }
    #[test]
    fn desktop_decision_settles_request_and_then_yields_to_the_next_one() {
        let request = |id: &str| crate::approval::Request {
            id: id.into(),
            session: "claude:a".into(),
            title: "ALLOW Bash?".into(),
            tool_name: "Bash".into(),
            text: "ls".into(),
        };
        let mut scene = scene();
        scene.set_requests(vec![request("first")]);
        let broker = Broker::default();
        let mut s = Store::new([1; 16]);
        s.ingest(&mut scene, Some(&broker));
        let revision = s.authority.model.revision;
        assert_eq!(s.authority.model.sessions["claude:a"].approval.as_ref().unwrap().state, 1);
        // The desktop answered: the broker dropped the request before the hook reply landed.
        scene.set_requests(vec![]);
        s.ingest(&mut scene, Some(&broker));
        let r = s.authority.model.sessions["claude:a"].approval.clone().unwrap();
        assert_eq!((r.id.as_str(), r.state), ("first", 2));
        scene.set_requests(vec![request("second")]);
        s.ingest(&mut scene, Some(&broker));
        assert_eq!(s.authority.model.sessions["claude:a"].approval.as_ref().unwrap().id, "first");
        broker.record("first".into(), Outcome::Delivered(Decision::Allow));
        s.ingest(&mut scene, Some(&broker));
        let r = s.authority.model.sessions["claude:a"].approval.clone().unwrap();
        assert_eq!((r.id.as_str(), r.state, r.delivered_decision), ("first", 3, 1));
        assert!(s.authority.model.revision > revision);
        assert!(s.body(&r.body_blob.unwrap().sha256).is_some());
        s.ingest(&mut scene, Some(&broker));
        let r = s.authority.model.sessions["claude:a"].approval.clone().unwrap();
        assert_eq!((r.id.as_str(), r.state), ("second", 1));
    }
    #[test]
    fn appearance_uses_renderer_utf8_hash() {
        let text = "claude:café";
        let expected = text.bytes().fold(2166136261u32, |h, b| (h ^ u32::from(b)).wrapping_mul(16777619));
        assert_eq!(appearance(text), expected);
    }
}
