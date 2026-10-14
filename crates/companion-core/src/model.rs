//! Atomic state replication. Mutations are published only after complete validation.
use crate::{
    crypto::{self, Error, Result},
    wire::*,
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec::Vec,
};
use sha2::{Digest, Sha256};
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub revision: u64,
    pub sessions: BTreeMap<String, Session>,
    pub view: View,
    pub usage: Usage,
    pub overflow_count: u32,
}
fn id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128
}
pub fn blob_valid(b: &BlobRef) -> bool {
    b.sha256.len() == 32 && b.byte_length <= 32768
}
pub fn validate(model: &Model) -> Result<()> {
    if model.revision == 0
        || model.sessions.len() > 64
        || model.usage.tokens_display.len() > 24
        || model.usage.cost_display.len() > 24
        || !model.usage.tokens_display.is_ascii()
        || !model.usage.cost_display.is_ascii()
    {
        return Err(Error::Invalid);
    }
    let mut seats = [false; 8];
    let mut requests = BTreeSet::new();
    for (key, s) in &model.sessions {
        if key != &s.id
            || !id(key)
            || !(1..=2).contains(&s.provider)
            || !(1..=4).contains(&s.status)
            || !(0..=3).contains(&s.waiting_reason)
            || s.title.len() > 192
            || s.display_name.len() > 192
            || s.branch.as_ref().is_some_and(|b| b.len() > 192)
        {
            return Err(Error::Invalid);
        }
        if s.seat != 255 {
            let n = s.seat as usize;
            if n >= 8 || seats[n] || (n % 4 < 2) != (s.provider == 1) {
                return Err(Error::Invalid);
            }
            seats[n] = true;
        }
        if s.task_blob.as_ref().is_some_and(|b| !blob_valid(b))
            || s.unlinked_review_blob.as_ref().is_some_and(|b| !blob_valid(b))
        {
            return Err(Error::Invalid);
        }
        if let Some(r) = &s.approval {
            if !id(&r.id)
                || !requests.insert(&r.id)
                || !matches!(r.state, 1 | 2 | 3 | 5 | 6 | 7 | 8)
                || r.title.len() > 192
                || r.tool_name.len() > 192
                || r.desktop_only_reason.len() > 192
                || r.digest.len() != 32
                || r.allowed_decisions.len() > 2
                || r.allowed_decisions.iter().any(|d| !matches!(d, 1 | 2))
                || r.allowed_decisions.windows(2).any(|p| p[0] >= p[1])
                || (r.allowed_decisions.is_empty() && r.desktop_only_reason.is_empty())
                || (!r.desktop_only_reason.is_empty() && !r.allowed_decisions.is_empty())
                || r.delivered_decision < 0
                || r.delivered_decision > 2
                || (r.state == 3 && r.delivered_decision == 0)
            {
                return Err(Error::Invalid);
            }
            let b = r.body_blob.as_ref().filter(|b| blob_valid(b)).ok_or(Error::Invalid)?;
            let decisions: Vec<u8> = r.allowed_decisions.iter().map(|d| *d as u8).collect();
            if crypto::request_digest(
                &r.id,
                &s.id,
                s.provider as u8,
                &r.title,
                &r.tool_name,
                &b.sha256.as_slice().try_into().unwrap(),
                b.byte_length,
                &decisions,
            )?
            .as_slice()
                != r.digest
            {
                return Err(Error::Invalid);
            }
        }
    }
    let v = &model.view;
    if !matches!(v.page, 1 | 2)
        || v.origin_device_id.len() > 64
        || !v.origin_device_id.is_ascii()
        || (!v.origin_connection_epoch.is_empty() && v.origin_connection_epoch.len() != 16)
    {
        return Err(Error::Invalid);
    }
    if v.selected_session_id.is_empty() {
        if v.page != 1 || v.task_anchor != 0 || v.review_anchor != 0 || !v.review_request_id.is_empty() {
            return Err(Error::Invalid);
        }
    } else {
        let s = model.sessions.get(&v.selected_session_id).ok_or(Error::Invalid)?;
        if v.task_anchor > s.task_blob.as_ref().map_or(0, |b| b.byte_length) {
            return Err(Error::Invalid);
        }
        if v.page == 2 {
            let b = v.review_blob.as_ref().filter(|b| blob_valid(b)).ok_or(Error::Invalid)?;
            if v.review_anchor > b.byte_length {
                return Err(Error::Invalid);
            }
            match v.review_kind {
                1 => {
                    let r = s.approval.as_ref().ok_or(Error::Invalid)?;
                    if v.review_request_id != r.id || r.body_blob.as_ref() != Some(b) {
                        return Err(Error::Invalid);
                    }
                }
                2 => {
                    if s.task_blob.as_ref() != Some(b) || !v.review_request_id.is_empty() {
                        return Err(Error::Invalid);
                    }
                }
                3 => {
                    if s.unlinked_review_blob.as_ref() != Some(b) || !v.review_request_id.is_empty() {
                        return Err(Error::Invalid);
                    }
                }
                _ => return Err(Error::Invalid),
            }
        }
    }
    Ok(())
}
impl Model {
    pub fn delta(&self, d: &Delta) -> Result<Self> {
        if d.base_revision != self.revision
            || d.revision <= self.revision
            || d.upsert_sessions.len() > 2
            || d.remove_session_ids.len() > 8
        {
            return Err(Error::Sequence);
        }
        let mut seen = BTreeSet::new();
        let mut next = self.clone();
        for id in &d.remove_session_ids {
            if !seen.insert(id) {
                return Err(Error::Invalid);
            }
            next.sessions.remove(id);
        }
        for s in &d.upsert_sessions {
            if !seen.insert(&s.id) {
                return Err(Error::Invalid);
            }
            next.sessions.insert(s.id.clone(), s.clone());
        }
        if let Some(v) = &d.view {
            next.view = v.clone();
        }
        if let Some(u) = &d.usage {
            next.usage = u.clone();
        }
        if let Some(n) = d.overflow_count {
            next.overflow_count = n;
        }
        next.revision = d.revision;
        validate(&next)?;
        Ok(next)
    }
}
struct Staging {
    model: Model,
    expected: usize,
    index: u32,
    hash: Sha256,
    started: u64,
}
pub struct Replica {
    pub model: Model,
    pub ready: bool,
    stage: Option<Staging>,
    resync: bool,
}
impl Default for Replica {
    fn default() -> Self {
        Self { model: Model::default(), ready: false, stage: None, resync: true }
    }
}
impl Replica {
    pub fn begin(&mut self, b: &SnapshotBegin, wire: &[u8], now: u64) -> Result<()> {
        if self.stage.is_some()
            || b.revision == 0
            || b.revision < self.model.revision
            || (!self.resync && b.revision == self.model.revision)
            || b.session_count > 64
        {
            return Err(Error::Sequence);
        }
        let model = Model {
            revision: b.revision,
            sessions: BTreeMap::new(),
            view: b.view.clone().ok_or(Error::Invalid)?,
            usage: b.usage.clone().unwrap_or_default(),
            overflow_count: b.overflow_count,
        };
        let mut hash = Sha256::new();
        hash.update((wire.len() as u32).to_le_bytes());
        hash.update(wire);
        self.stage = Some(Staging { model, expected: b.session_count as usize, index: 0, hash, started: now });
        self.ready = self.ready && !self.resync;
        Ok(())
    }
    pub fn part(&mut self, p: &SnapshotPart, wire: &[u8], now: u64) -> Result<()> {
        self.check_timeout(now)?;
        let s = self.stage.as_mut().ok_or(Error::Sequence)?;
        if p.revision != s.model.revision
            || p.index != s.index
            || p.sessions.is_empty()
            || p.sessions.len() > 2
            || s.model.sessions.len() + p.sessions.len() > s.expected
        {
            return Err(Error::Sequence);
        }
        for session in &p.sessions {
            if s.model.sessions.insert(session.id.clone(), session.clone()).is_some() {
                return Err(Error::Invalid);
            }
        }
        s.index += 1;
        s.hash.update((wire.len() as u32).to_le_bytes());
        s.hash.update(wire);
        Ok(())
    }
    pub fn commit(&mut self, c: &SnapshotCommit, now: u64) -> Result<()> {
        self.check_timeout(now)?;
        let s = self.stage.take().ok_or(Error::Sequence)?;
        if c.revision != s.model.revision
            || c.parts_count != s.index
            || s.model.sessions.len() != s.expected
            || s.hash.finalize().as_slice() != c.sha256
        {
            return Err(Error::Invalid);
        }
        validate(&s.model)?;
        self.model = s.model;
        self.ready = true;
        self.resync = false;
        Ok(())
    }
    pub fn delta(&mut self, d: &Delta) -> Result<bool> {
        if self.resync || self.stage.is_some() {
            return Ok(false);
        }
        self.model = self.model.delta(d)?;
        Ok(true)
    }
    pub fn resync(&mut self) {
        self.ready = false;
        self.resync = true;
        self.stage = None;
    }
    pub fn disconnect(&mut self) {
        self.ready = false;
        self.stage = None;
        self.resync = true;
    }
    pub fn new_authority(&mut self) {
        self.disconnect();
        self.model = Model::default();
    }
    pub fn check_timeout(&mut self, now: u64) -> Result<()> {
        if self.stage.as_ref().is_some_and(|s| now.saturating_sub(s.started) > 60000) {
            self.disconnect();
            Err(Error::Sequence)
        } else {
            Ok(())
        }
    }
}
/// Body cache never exposes a partial or unverified object to the renderer.
#[derive(Default)]
pub struct BlobCache {
    complete: BTreeMap<Vec<u8>, (Vec<u8>, u64)>,
    incoming: Option<(Vec<u8>, u32, Vec<u8>)>,
    clock: u64,
    pinned: Option<Vec<u8>>,
}
impl BlobCache {
    pub fn get(&mut self, sha: &[u8]) -> Option<&[u8]> {
        self.clock += 1;
        self.complete.get_mut(sha).map(|(b, t)| {
            *t = self.clock;
            b.as_slice()
        })
    }
    pub fn peek(&self, sha: &[u8]) -> Option<&[u8]> {
        self.complete.get(sha).map(|(b, _)| b.as_slice())
    }
    pub fn pin(&mut self, sha: Option<&[u8]>) {
        if self.pinned.as_deref() != sha {
            self.pinned = sha.map(|s| s.to_vec());
        }
    }
    pub fn cancel(&mut self) {
        self.incoming = None;
    }
    pub fn receive(&mut self, c: &BlobChunk) -> Result<bool> {
        if c.sha256.len() != 32 || c.total_length > 32768 || c.data.len() > 512 {
            return Err(Error::Limit);
        }
        if self.incoming.is_none() {
            if c.offset != 0 {
                return Err(Error::Sequence);
            }
            self.incoming = Some((c.sha256.clone(), c.total_length, Vec::with_capacity(c.total_length as usize)));
        }
        let (sha, total, data) = self.incoming.as_mut().unwrap();
        if *sha != c.sha256
            || *total != c.total_length
            || data.len() != c.offset as usize
            || data.len() + c.data.len() > *total as usize
            || c.r#final != (data.len() + c.data.len() == *total as usize)
        {
            self.cancel();
            return Err(Error::Sequence);
        }
        data.extend_from_slice(&c.data);
        if c.r#final {
            let (sha, _, data) = self.incoming.take().unwrap();
            if crypto::hash(&data).as_slice() != sha || core::str::from_utf8(&data).is_err() {
                return Err(Error::Invalid);
            }
            if !self.complete.contains_key(&sha) && self.complete.len() >= 4 {
                let oldest = self
                    .complete
                    .iter()
                    .filter(|(key, _)| self.pinned.as_ref() != Some(*key))
                    .min_by_key(|(_, (_, time))| *time)
                    .map(|(key, _)| key.clone())
                    .ok_or(Error::Limit)?;
                self.complete.remove(&oldest);
            }
            self.clock += 1;
            self.complete.insert(sha, (data, self.clock));
            return Ok(true);
        }
        Ok(false)
    }
}
/// Pure seat allocation independent of animation, preserving existing seats.
pub fn allocate(old: &Model, sessions: &mut [Session], arrival_order: &[String]) {
    let rank = |id: &str| arrival_order.iter().position(|x| x == id).unwrap_or(usize::MAX);
    for s in sessions.iter_mut() {
        s.seat = old.sessions.get(&s.id).filter(|o| o.provider == s.provider).map_or(255, |o| o.seat);
    }
    for provider in [1, 2] {
        let side = if provider == 1 { [0, 1, 4, 5] } else { [2, 3, 6, 7] };
        let mut outside: Vec<usize> = sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| s.provider == provider && s.seat == 255)
            .map(|(i, _)| i)
            .collect();
        outside
            .sort_by_key(|i| (!matches!(sessions[*i].status, 1 | 3), rank(&sessions[*i].id), sessions[*i].id.clone()));
        for i in outside {
            if let Some(seat) = side.iter().find(|seat| !sessions.iter().any(|s| s.seat == **seat)) {
                sessions[i].seat = *seat;
            } else if matches!(sessions[i].status, 1 | 3)
                && let Some(idle) = side
                    .iter()
                    .find_map(|seat| sessions.iter().position(|s| s.seat == *seat && matches!(s.status, 2 | 4)))
            {
                sessions[i].seat = sessions[idle].seat;
                sessions[idle].seat = 255;
            }
        }
        for target in side {
            let idle = sessions.iter().position(|s| s.seat == target && matches!(s.status, 2 | 4));
            let active = sessions
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    s.provider == provider
                        && matches!(s.status, 1 | 3)
                        && s.seat != 255
                        && side.iter().position(|v| *v == s.seat) > side.iter().position(|v| *v == target)
                })
                .min_by_key(|(_, s)| side.iter().position(|v| *v == s.seat))
                .map(|(i, _)| i);
            if let (Some(idle), Some(active)) = (idle, active) {
                let other = sessions[active].seat;
                sessions[active].seat = target;
                sessions[idle].seat = other;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    fn session(id: &str, seat: u32) -> Session {
        Session { id: id.to_string(), provider: 1, status: 2, seat, ..Default::default() }
    }
    fn model() -> Model {
        Model {
            revision: 1,
            sessions: BTreeMap::from([
                ("claude:a".into(), session("claude:a", 0)),
                ("claude:b".into(), session("claude:b", 1)),
            ]),
            view: View { page: 1, ..Default::default() },
            ..Default::default()
        }
    }
    #[test]
    fn invalid_delta_does_not_partially_mutate() {
        let m = model();
        validate(&m).unwrap();
        let d = Delta {
            base_revision: 1,
            revision: 2,
            upsert_sessions: alloc::vec![session("claude:a", 1)],
            ..Default::default()
        };
        assert!(m.delta(&d).is_err());
        assert_eq!(m.sessions["claude:a"].seat, 0);
    }
    #[test]
    fn atomic_swap_and_removal() {
        let m = model();
        let d = Delta {
            base_revision: 1,
            revision: 2,
            upsert_sessions: alloc::vec![session("claude:a", 1), session("claude:b", 0)],
            ..Default::default()
        };
        assert_eq!(m.delta(&d).unwrap().sessions["claude:a"].seat, 1);
    }
    #[test]
    fn blobs_are_verified_before_publication() {
        let data = b"test";
        let sha = crypto::hash(data).to_vec();
        let mut c = BlobCache::default();
        assert!(
            !c.receive(&BlobChunk {
                sha256: sha.clone(),
                total_length: 4,
                offset: 0,
                data: b"te".to_vec(),
                r#final: false
            })
            .unwrap()
        );
        assert!(c.peek(&sha).is_none());
        assert!(
            c.receive(&BlobChunk {
                sha256: sha.clone(),
                total_length: 4,
                offset: 2,
                data: b"st".to_vec(),
                r#final: true
            })
            .unwrap()
        );
        assert_eq!(c.peek(&sha), Some(data.as_slice()));
    }
    #[test]
    fn allocator_promotes_waiting_and_keeps_sides() {
        let mut m = model();
        m.sessions.get_mut("claude:b").unwrap().seat = 5;
        let mut ss: Vec<_> = m.sessions.values().cloned().collect();
        ss[1].status = 3;
        allocate(&m, &mut ss, &[]);
        assert_eq!(ss[1].seat, 0);
        assert_eq!(ss[0].seat, 5);
    }
}
