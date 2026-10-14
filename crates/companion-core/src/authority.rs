//! Authoritative action ordering. Provider side effects are returned, never executed here.
use crate::{
    crypto,
    model::{Model, validate},
    wire::*,
};
use alloc::{collections::VecDeque, string::String, vec::Vec};
use prost::Message;
#[derive(Clone, Debug, PartialEq)]
pub enum Dispatch {
    Desktop(String),
    Decision { session: String, request: String, decision: i32 },
}
#[derive(Clone)]
struct Cached {
    id: u64,
    fingerprint: [u8; 32],
    result: ActionResult,
}
pub struct Peer {
    pub device: String,
    pub connection: Vec<u8>,
    pub can_decide: bool,
    pub can_navigate: bool,
    high: u64,
    recent: VecDeque<Cached>,
    last_navigation: Option<u64>,
}
impl Peer {
    pub fn new(device: String, connection: Vec<u8>, can_decide: bool, can_navigate: bool) -> Self {
        Self { device, connection, can_decide, can_navigate, high: 0, recent: VecDeque::new(), last_navigation: None }
    }
    pub fn complete(&mut self, id: u64, result: ActionResult) {
        if let Some(c) = self.recent.iter_mut().find(|c| c.id == id) {
            c.result = result;
        }
    }
}
pub struct Authority {
    pub model: Model,
}
impl Authority {
    fn result(&self, id: u64, status: i32, reason: i32) -> ActionResult {
        ActionResult {
            action_id: id,
            status,
            reason,
            applied_revision: self.model.revision,
            view_generation: self.model.view.generation,
        }
    }
    pub fn action(
        &mut self,
        peer: &mut Peer,
        a: &Action,
        now: u64,
        body: Option<&[u8]>,
    ) -> core::result::Result<(ActionResult, Option<Dispatch>), crypto::Error> {
        let fingerprint = crypto::hash(&a.encode_to_vec());
        if let Some(old) = peer.recent.iter().find(|c| c.id == a.id) {
            return if old.fingerprint == fingerprint { Ok((old.result, None)) } else { Err(crypto::Error::Sequence) };
        }
        if a.id == 0 || a.id <= peer.high {
            return Ok((self.result(a.id, 3, 7), None));
        }
        // Never evict pending dispatches, and do not advance the high-water mark
        // when capacity cannot safely retain this new action.
        if peer.recent.len() >= 64 {
            let Some(pos) = peer.recent.iter().position(|c| c.result.status != 2) else {
                return Ok((self.result(a.id, 3, 6), None));
            };
            peer.recent.remove(pos);
        }
        peer.high = a.id;
        let before = self.model.clone();
        let mut status = 1;
        let mut reason = 0;
        let mut dispatch = None;
        if a.observed_revision > self.model.revision || !(1..=7).contains(&a.kind) {
            status = 3;
            reason = 7;
        } else {
            let session = self.model.sessions.get(&a.session_id).cloned();
            match a.kind {
                1 => {
                    if !a.session_id.is_empty() && session.is_none() {
                        status = 3;
                        reason = 1;
                    } else {
                        self.model.view.selected_session_id = a.session_id.clone();
                        self.model.view.task_anchor = 0;
                        self.close_review();
                    }
                }
                2 => {
                    if let Some(s) = session {
                        let b = match a.review_kind {
                            1 => s.approval.as_ref().filter(|r| r.id == a.request_id).and_then(|r| r.body_blob.clone()),
                            2 => s.task_blob,
                            3 => s.unlinked_review_blob,
                            _ => None,
                        };
                        if let Some(b) = b.filter(|b| b.sha256 == a.blob_id) {
                            self.model.view.selected_session_id = a.session_id.clone();
                            self.model.view.page = 2;
                            self.model.view.review_kind = a.review_kind;
                            self.model.view.review_request_id =
                                if a.review_kind == 1 { a.request_id.clone() } else { String::new() };
                            self.model.view.review_blob = Some(b);
                            self.model.view.review_anchor = 0;
                        } else {
                            status = 3;
                            reason = 3;
                        }
                    } else {
                        status = 3;
                        reason = 1;
                    }
                }
                3 => self.close_review(),
                4 | 5 => {
                    let expected = if a.kind == 4 {
                        session.as_ref().and_then(|s| s.task_blob.as_ref())
                    } else {
                        self.model.view.review_blob.as_ref()
                    };
                    let request_ok =
                        a.kind == 4 || self.model.view.page == 2 && self.model.view.review_request_id == a.request_id;
                    let anchor = a.anchor.and_then(|offset| {
                        body.filter(|bytes| crypto::hash(bytes).as_slice() == a.blob_id)
                            .and_then(|bytes| core::str::from_utf8(bytes).ok())
                            .filter(|text| text.is_char_boundary(offset as usize))
                            .map(|_| offset)
                    });
                    if a.session_id != self.model.view.selected_session_id
                        || !request_ok
                        || expected.is_none_or(|b| b.sha256 != a.blob_id)
                        || anchor.is_none()
                    {
                        status = 3;
                        reason = 3;
                    } else if let Some(anchor) = anchor {
                        if a.kind == 4 {
                            self.model.view.task_anchor = anchor;
                        } else {
                            self.model.view.review_anchor = anchor;
                        }
                    }
                }
                6 => {
                    if !peer.can_navigate || session.as_ref().is_none_or(|s| !s.navigation_available) {
                        status = 3;
                        reason = 4;
                    } else if peer.last_navigation.is_some_and(|t| now.saturating_sub(t) < 1000) {
                        status = 3;
                        reason = 6;
                    } else {
                        peer.last_navigation = Some(now);
                        self.model.view.selected_session_id = a.session_id.clone();
                        self.model.view.task_anchor = 0;
                        self.close_review();
                        status = 2;
                        dispatch = Some(Dispatch::Desktop(a.session_id.clone()));
                    }
                }
                7 => {
                    if !peer.can_decide {
                        status = 3;
                        reason = 4;
                    } else if let Some(s) = session {
                        if let Some(r) = s.approval {
                            if r.id != a.request_id
                                || r.digest != a.request_digest
                                || r.body_blob.as_ref().is_none_or(|b| b.sha256 != a.displayed_body_sha256)
                            {
                                status = 3;
                                reason = 2;
                            } else if r.state != 1 {
                                status = 3;
                                reason = 12;
                            } else if s.provider != 1
                                || !r.desktop_only_reason.is_empty()
                                || !r.allowed_decisions.contains(&a.decision)
                            {
                                status = 3;
                                reason = 4;
                            } else {
                                self.model.sessions.get_mut(&a.session_id).unwrap().approval.as_mut().unwrap().state =
                                    2;
                                status = 2;
                                dispatch = Some(Dispatch::Decision {
                                    session: a.session_id.clone(),
                                    request: a.request_id.clone(),
                                    decision: a.decision,
                                });
                            }
                        } else {
                            status = 3;
                            reason = 2;
                        }
                    } else {
                        status = 3;
                        reason = 1;
                    }
                }
                _ => unreachable!(),
            }
        }
        if status != 3 && self.model != before {
            if self.model.revision == u64::MAX || self.model.view.generation == u64::MAX {
                self.model = before;
                return Err(crypto::Error::Limit);
            }
            self.model.revision += 1;
            if self.model.view != before.view {
                self.model.view.generation = self.model.view.generation.checked_add(1).ok_or(crypto::Error::Limit)?;
                self.model.view.origin_device_id = peer.device.clone();
                self.model.view.origin_connection_epoch = peer.connection.clone();
                self.model.view.applied_action_id = a.id;
            }
            if validate(&self.model).is_err() {
                self.model = before;
                status = 3;
                reason = 7;
                dispatch = None;
            }
        }
        let result = self.result(a.id, status, reason);
        peer.recent.push_back(Cached { id: a.id, fingerprint, result });
        Ok((result, dispatch))
    }
    fn close_review(&mut self) {
        let v = &mut self.model.view;
        v.page = 1;
        v.review_kind = 0;
        v.review_request_id.clear();
        v.review_blob = None;
        v.review_anchor = 0;
    }
    pub fn finish(&mut self, peer: &mut Peer, a: &Action, delivered: bool) -> ActionResult {
        if !peer
            .recent
            .iter()
            .any(|r| r.id == a.id && r.result.status == 2 && r.fingerprint == crypto::hash(&a.encode_to_vec()))
        {
            return self.result(a.id, 3, 7);
        }
        if self.model.revision == u64::MAX {
            return self.result(a.id, 4, 11);
        }
        if a.kind == 7
            && let Some(r) = self
                .model
                .sessions
                .get_mut(&a.session_id)
                .and_then(|s| s.approval.as_mut())
                .filter(|r| r.id == a.request_id && r.state == 2)
        {
            r.state = if delivered { 3 } else { 8 };
            r.delivered_decision = if delivered { a.decision } else { 0 };
            self.model.revision += 1;
        }
        let result = self.result(
            a.id,
            if delivered {
                1
            } else if a.kind == 6 {
                3
            } else {
                4
            },
            if delivered {
                0
            } else if a.kind == 6 {
                9
            } else {
                11
            },
        );
        peer.complete(a.id, result);
        result
    }
}
/// Pending optimistic inputs are identified by their connection, never just ID.
#[derive(Default)]
pub struct Intents {
    pending: VecDeque<Action>,
    results: VecDeque<ActionResult>,
}
impl Intents {
    pub fn push(&mut self, a: Action) -> bool {
        if self.pending.len() >= 32 {
            return false;
        }
        self.pending.push_back(a);
        true
    }
    pub fn result(&mut self, r: ActionResult) -> bool {
        if self.results.len() >= 32 {
            return false;
        }
        self.results.push_back(r);
        true
    }
    pub fn reconcile(&mut self, revision: u64) {
        let mut waiting = VecDeque::new();
        while let Some(r) = self.results.pop_front() {
            if r.applied_revision > revision {
                waiting.push_back(r);
            } else if r.status != 2 {
                self.pending.retain(|a| a.id != r.action_id);
            }
        }
        self.results = waiting;
    }
    pub fn projected(&self, base: &Model) -> View {
        let mut v = base.view.clone();
        for a in &self.pending {
            match a.kind {
                1 => {
                    v.selected_session_id = a.session_id.clone();
                    v.task_anchor = 0;
                    v.page = 1;
                    v.review_request_id.clear();
                    v.review_kind = 0;
                    v.review_blob = None;
                    v.review_anchor = 0;
                }
                2 => {
                    if let Some(s) = base.sessions.get(&a.session_id) {
                        let b = match a.review_kind {
                            1 => s.approval.as_ref().filter(|r| r.id == a.request_id).and_then(|r| r.body_blob.clone()),
                            2 => s.task_blob.clone(),
                            3 => s.unlinked_review_blob.clone(),
                            _ => None,
                        };
                        if let Some(b) = b.filter(|b| b.sha256 == a.blob_id) {
                            v.selected_session_id = a.session_id.clone();
                            v.page = 2;
                            v.review_kind = a.review_kind;
                            v.review_request_id = a.request_id.clone();
                            v.review_blob = Some(b);
                            v.review_anchor = 0;
                        }
                    }
                }
                3 => {
                    v.page = 1;
                    v.review_request_id.clear();
                    v.review_kind = 0;
                    v.review_blob = None;
                    v.review_anchor = 0;
                }
                4 => {
                    if a.session_id == v.selected_session_id {
                        v.task_anchor = a.anchor.unwrap_or(0);
                    }
                }
                5 => {
                    if a.session_id == v.selected_session_id && a.request_id == v.review_request_id {
                        v.review_anchor = a.anchor.unwrap_or(0);
                    }
                }
                6 => {
                    v.selected_session_id = a.session_id.clone();
                    v.page = 1;
                    v.task_anchor = 0;
                    v.review_request_id.clear();
                    v.review_kind = 0;
                    v.review_blob = None;
                    v.review_anchor = 0;
                }
                _ => (),
            }
        }
        v
    }
    pub fn clear(&mut self) {
        self.pending.clear();
        self.results.clear();
    }
    pub fn len(&self) -> usize {
        self.pending.len()
    }
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;
    fn authority() -> Authority {
        Authority {
            model: Model {
                revision: 1,
                sessions: BTreeMap::from([(
                    "claude:a".into(),
                    Session { id: "claude:a".into(), provider: 1, status: 2, seat: 0, ..Default::default() },
                )]),
                view: View { page: 1, ..Default::default() },
                ..Default::default()
            },
        }
    }
    #[test]
    fn retries_are_idempotent_and_conflicting_payload_fails() {
        let mut h = authority();
        let mut p = Peer::new("test".into(), alloc::vec![1;16], false, true);
        let a = Action { id: 1, kind: 1, session_id: "claude:a".into(), ..Default::default() };
        let (r, _) = h.action(&mut p, &a, 0, None).unwrap();
        assert_eq!(r.status, 1);
        let rev = h.model.revision;
        assert_eq!(h.action(&mut p, &a, 0, None).unwrap().0, r);
        assert_eq!(h.model.revision, rev);
        let mut different = a;
        different.session_id.clear();
        assert!(h.action(&mut p, &different, 0, None).is_err());
    }
    #[test]
    fn rejected_intent_waits_for_its_revision() {
        let mut q = Intents::default();
        q.push(Action { id: 1, kind: 1, session_id: "gone".into(), ..Default::default() });
        q.result(ActionResult { action_id: 1, status: 3, applied_revision: 4, ..Default::default() });
        q.reconcile(3);
        assert_eq!(q.len(), 1);
        q.reconcile(4);
        assert!(q.is_empty());
    }
    #[test]
    fn qa_peer_cannot_dispatch() {
        let mut h = authority();
        let mut p = Peer::new("qa".into(), alloc::vec![1;16], false, true);
        let (r, d) = h.action(&mut p, &Action { id: 1, kind: 7, ..Default::default() }, 0, None).unwrap();
        assert_eq!(r.reason, 4);
        assert!(d.is_none());
    }
}
