//! Board-local UI, optimistic gestures and verified review gating. No platform IO.
use crate::{
    authority::Intents,
    codec,
    crypto::{Error, Result},
    model::{BlobCache, Replica},
    wire::{envelope::Payload, *},
};
use alloc::{
    collections::VecDeque,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use cafe_ui::{
    approval::Request as UiRequest,
    review::Review,
    scene::{self, Scene},
};
#[derive(Clone, Debug, PartialEq)]
pub struct Receipt {
    pub authority: Vec<u8>,
    pub session: String,
    pub request: String,
    pub digest: Vec<u8>,
    pub decision: i32,
}
#[derive(Clone, Debug, PartialEq)]
enum Target {
    Patron(String),
    Body(String),
    Rail(bool),
    Back,
    Desktop(String),
    Decision(i32, Vec<u8>),
    Dismiss,
    None,
}
struct Contact {
    target: Target,
    x: i32,
    y: i32,
    down: u64,
    cancelled: bool,
    armed: bool,
}
pub struct Client {
    pub replica: Replica,
    pub blobs: BlobCache,
    pub scene: Scene,
    pub intents: Intents,
    pub connection: Vec<u8>,
    pub authority: Vec<u8>,
    pub device: String,
    pub connected: bool,
    outgoing: VecDeque<Envelope>,
    next_action: u64,
    hello: bool,
    can_decide: bool,
    fetching: Option<Vec<u8>>,
    fetching_meta: Option<(String, BlobRef, bool)>,
    shown_task: Option<(String, BlobRef, u64)>,
    refresh_at: Option<u64>,
    unavailable: Option<Vec<u8>>,
    last_tick: u64,
    last_heartbeat: u64,
    contact: Option<Contact>,
    last_tap: Option<(Target, u64, i32, i32)>,
    last_touch: u64,
    rendered_identity: Vec<u8>,
    review_identity: Vec<u8>,
    displayed_at: Option<u64>,
    local_review: bool,
    pub sleeping: bool,
    wake_contact: bool,
    pub receipts: Vec<Receipt>,
    receipt_wait: Option<u64>,
}
impl Client {
    pub fn new(device: String, connection: Vec<u8>, injection: bool, now: u64) -> Self {
        let mut c = Self {
            replica: Replica::default(),
            blobs: BlobCache::default(),
            scene: Scene::new(),
            intents: Intents::default(),
            connection,
            authority: Vec::new(),
            device,
            outgoing: VecDeque::new(),
            next_action: 1,
            hello: false,
            can_decide: false,
            connected: true,
            fetching: None,
            fetching_meta: None,
            shown_task: None,
            refresh_at: None,
            unavailable: None,
            last_tick: now,
            last_heartbeat: now,
            contact: None,
            last_tap: None,
            last_touch: now,
            rendered_identity: Vec::new(),
            review_identity: Vec::new(),
            displayed_at: None,
            local_review: false,
            sleeping: false,
            wake_contact: false,
            receipts: vec![],
            receipt_wait: None,
        };
        c.emit(Payload::Hello(Hello {
            device_id: c.device.clone(),
            minor: 0,
            asset_digest: cafe_ui::ASSET_DIGEST.to_vec(),
            max_envelope_bytes: 4096,
            max_blob_bytes: 32768,
            capabilities: vec![1, 2, 3, 4, 5, 6],
            required_capabilities: vec![1, 2, 6],
            input_injection_enabled: injection,
        }))
        .unwrap();
        c
    }
    fn emit(&mut self, p: Payload) -> Result<()> {
        if self.outgoing.len() >= 32 {
            return Err(Error::Limit);
        }
        let e = Envelope {
            major: 2,
            connection_epoch: self.connection.clone(),
            authority_epoch: self.authority.clone(),
            payload: Some(p),
        };
        codec::encode(&e)?;
        self.outgoing.push_back(e);
        Ok(())
    }
    pub fn outgoing(&mut self) -> Option<Envelope> {
        self.outgoing.pop_front()
    }
    pub fn disconnect(&mut self) {
        self.connected = false;
        self.replica.disconnect();
        self.intents.clear();
        self.outgoing.clear();
        self.contact = None;
        self.last_tap = None;
        self.can_decide = false;
        self.displayed_at = None;
        self.local_review = false;
        if let Some(r) = &mut self.scene.review {
            r.actionable = false;
            r.status = "DISCONNECTED - CHECK DESKTOP".into();
        }
    }
    pub fn receive(&mut self, e: Envelope, raw: &[u8], now: u64) -> Result<()> {
        let result = self.receive_inner(e, raw, now);
        if result.is_err() {
            self.disconnect();
        }
        result
    }
    fn receive_inner(&mut self, e: Envelope, raw: &[u8], now: u64) -> Result<()> {
        if !self.connected || e.connection_epoch != self.connection {
            return Err(Error::Sequence);
        }
        if let Some(Payload::HelloAck(a)) = &e.payload {
            if self.hello
                || a.minor != 0
                || a.asset_digest != cafe_ui::ASSET_DIGEST
                || a.heartbeat_ms != 1000
                || a.session_limit != 64
                || a.blob_limit != 32768
                || a.max_envelope_bytes != 4096
                || !a.required_capabilities.iter().all(|v| [1, 2, 3, 4, 5, 6].contains(v))
                || ![1, 2, 6].iter().all(|v| a.capabilities.contains(v))
                || e.authority_epoch.len() != 16
            {
                return Err(Error::Invalid);
            }
            self.authority = e.authority_epoch;
            self.hello = true;
            self.can_decide = a.capabilities.contains(&3) || a.capabilities.contains(&4);
            self.emit(Payload::Resync(Resync { applied_revision: 0, reason: 1 }))?;
            return Ok(());
        }
        if !self.hello || e.authority_epoch != self.authority {
            return Err(Error::Sequence);
        }
        match e.payload.ok_or(Error::Invalid)? {
            Payload::SnapshotBegin(b) => self.replica.begin(&b, raw, now)?,
            Payload::SnapshotPart(p) => self.replica.part(&p, raw, now)?,
            Payload::SnapshotCommit(c) => {
                self.replica.commit(&c, now)?;
                self.emit(Payload::Applied(Applied { revision: self.replica.model.revision }))?;
                self.sync(now)?;
            }
            Payload::Delta(d) => match self.replica.delta(&d) {
                Ok(true) => {
                    self.emit(Payload::Applied(Applied { revision: self.replica.model.revision }))?;
                    self.sync(now)?;
                }
                Ok(false) => (),
                Err(_) => {
                    self.replica.resync();
                    self.intents.clear();
                    self.displayed_at = None;
                    self.emit(Payload::Resync(Resync { applied_revision: self.replica.model.revision, reason: 3 }))?;
                }
            },
            Payload::BlobChunk(b) => {
                if self.fetching.as_ref() != Some(&b.sha256) {
                    return Err(Error::Sequence);
                }
                if self.fetching_meta.as_ref().is_none_or(|(_, r, _)| r.byte_length != b.total_length) {
                    return Err(Error::Invalid);
                }
                if self.blobs.receive(&b)? {
                    if let Some((id, reference, true)) = self.fetching_meta.take() {
                        self.shown_task = Some((id, reference, now));
                    }
                    self.fetching = None;
                    self.sync(now)?;
                }
            }
            Payload::BlobUnavailable(b) => {
                if self.fetching.as_ref() != Some(&b.sha256) {
                    return Err(Error::Sequence);
                }
                self.unavailable = Some(b.sha256);
                self.fetching = None;
                self.blobs.cancel();
                self.sync(now)?;
            }
            Payload::ActionResult(r) => {
                if !(1..=4).contains(&r.status) || !(0..=12).contains(&r.reason) || !self.intents.result(r) {
                    return Err(Error::Limit);
                }
                self.sync(now)?;
            }
            Payload::Heartbeat(_) => (),
            Payload::Error(_) => return Err(Error::Invalid),
            _ => return Err(Error::Sequence),
        }
        Ok(())
    }
    fn issue(&mut self, mut a: Action, now: u64) -> Result<()> {
        if !self.connected || !self.replica.ready {
            return Err(Error::Sequence);
        }
        a.id = self.next_action;
        a.observed_revision = self.replica.model.revision;
        if self.intents.len() >= 32 || self.outgoing.len() >= 32 {
            return Err(Error::Limit);
        }
        self.next_action = self.next_action.checked_add(1).ok_or(Error::Limit)?;
        self.emit(Payload::Action(a.clone()))?;
        self.intents.push(a);
        self.sync(now)
    }
    pub fn view(&self) -> View {
        self.intents.projected(&self.replica.model)
    }
    fn text(&self, b: Option<&BlobRef>) -> String {
        b.and_then(|b| self.blobs.peek(&b.sha256))
            .and_then(|v| core::str::from_utf8(v).ok())
            .unwrap_or("LOADING...")
            .to_string()
    }
    fn sync(&mut self, now: u64) -> Result<()> {
        if !self.replica.ready {
            return Ok(());
        }
        self.intents.reconcile(self.replica.model.revision);
        let v = self.view();
        let m = &self.replica.model;
        let sessions: Vec<_> = m
            .sessions
            .values()
            .map(|s| scene::Session {
                id: s.id.clone(),
                name: if s.display_name.is_empty() { s.title.clone() } else { s.display_name.clone() },
                kind: if s.provider == 1 { scene::Kind::Claude } else { scene::Kind::Codex },
                state: match s.status {
                    1 => scene::State::Work,
                    3 => scene::State::Wait,
                    _ => scene::State::Idle,
                },
                why: match s.waiting_reason {
                    1 => scene::Why::Perm,
                    2 | 3 => scene::Why::Ask,
                    _ => scene::Why::None,
                },
                pose: scene::ForcedPose::Auto,
                age_s: (s.state_age_ms / 1000).min(u32::MAX as u64) as u32,
                branch: s.branch.clone(),
                active_subagents: s.active_subagents.map(|n| n as usize),
            })
            .collect();
        let seats: Vec<_> = m.sessions.values().map(|s| (s.id.clone(), s.seat as usize)).collect();
        self.scene.set_authoritative_sessions(&sessions, &seats);
        for s in m.sessions.values() {
            self.scene.set_appearance(&s.id, s.appearance_seed);
        }
        let details = m
            .sessions
            .values()
            .map(|s| scene::SessionDetail {
                id: s.id.clone(),
                target: None,
                task: {
                    let current = s.approval.as_ref().and_then(|r| r.body_blob.as_ref()).or(s.task_blob.as_ref());
                    let fallback = self
                        .shown_task
                        .as_ref()
                        .filter(|(id, _, _)| id == &s.id && s.id == v.selected_session_id && s.approval.is_none())
                        .map(|(_, b, _)| b);
                    self.text(current.filter(|b| self.blobs.peek(&b.sha256).is_some()).or(fallback).or(current))
                },
            })
            .collect();
        self.scene.set_details(details);
        self.scene.set_usage_text(m.usage.tokens_display.clone(), m.usage.cost_display.clone());
        if v.selected_session_id.is_empty() {
            self.scene.clear_selection();
        } else {
            self.scene.focus_patron(v.selected_session_id.clone());
        }
        self.scene.set_task_anchor(v.task_anchor as usize);
        let wanted = if v.page == 2 {
            v.review_blob.clone()
        } else {
            m.sessions
                .get(&v.selected_session_id)
                .and_then(|s| s.approval.as_ref().and_then(|r| r.body_blob.clone()).or(s.task_blob.clone()))
        };
        self.blobs.pin(if v.page == 2 {
            wanted.as_ref().map(|b| b.sha256.as_slice())
        } else {
            self.shown_task
                .as_ref()
                .filter(|(id, _, _)| id == &v.selected_session_id)
                .map(|(_, b, _)| b.sha256.as_slice())
        });
        if v.page == 2 {
            let s = m.sessions.get(&v.selected_session_id).ok_or(Error::Invalid)?;
            let r = if v.review_kind == 1 { s.approval.as_ref() } else { None };
            let request = UiRequest {
                id: r.map_or_else(String::new, |r| r.id.clone()),
                session: s.id.clone(),
                title: r.map_or_else(|| s.title.clone(), |r| r.title.clone()),
                tool_name: r.map_or_else(String::new, |r| r.tool_name.clone()),
                text: self.text(wanted.as_ref()),
            };
            let verified = wanted.as_ref().is_some_and(|b| self.blobs.peek(&b.sha256).is_some());
            let mut identity = vec![];
            identity.extend_from_slice(s.id.as_bytes());
            identity.push(0);
            identity.extend_from_slice(&wanted.as_ref().map_or_else(Vec::new, |b| b.sha256.clone()));
            if let Some(r) = r {
                identity.extend_from_slice(&r.digest);
                identity.push(r.state as u8);
            }
            if identity != self.review_identity {
                self.review_identity = identity;
                self.displayed_at = None;
                self.rendered_identity.clear();
                self.contact = None;
            }
            let mut review = Review::new(request, false);
            review.allow_approve = r.is_some_and(|r| r.allowed_decisions.contains(&1));
            review.allow_deny = r.is_some_and(|r| r.allowed_decisions.contains(&2));
            let receipt = r.is_some_and(|r| self.receipts.iter().any(|x| x.request == r.id && x.digest == r.digest));
            let eligible = verified
                && self.can_decide
                && r.is_some_and(|r| r.state == 1 && r.desktop_only_reason.is_empty())
                && Review::identity_fits(&review.request)
                && !receipt;
            review.actionable = eligible && self.receipts.len() < 8;
            review.dismiss_pending = (receipt || self.receipts.len() >= 8)
                && self.receipt_wait.is_none_or(|t| now.saturating_sub(t) >= 5000);
            review.dismiss_all = self.receipts.len() >= 8;
            review.status = if !verified {
                if self.unavailable == wanted.as_ref().map(|b| b.sha256.clone()) {
                    "TEXT UNAVAILABLE - OPEN DESKTOP"
                } else {
                    "LOADING VERIFIED TEXT..."
                }
                .into()
            } else if receipt {
                "SENT - CHECK DESKTOP".into()
            } else if let Some(r) = r {
                match r.state {
                    2 => "SENDING DECISION...",
                    3 => "SENT TO CLAUDE",
                    5 => "REQUEST EXPIRED",
                    6 => "REQUEST CANCELLED",
                    7 => "DELIVERY FAILED",
                    8 => "OUTCOME UNKNOWN - CHECK DESKTOP",
                    _ => {
                        if !eligible {
                            "ANSWER IN DESKTOP"
                        } else if !self.local_review {
                            "TAP TEXT TO ENABLE OPTIONS"
                        } else if !self.armed(now) {
                            "PLEASE WAIT..."
                        } else {
                            ""
                        }
                    }
                }
                .into()
            } else {
                "READ ONLY - OPEN DESKTOP".into()
            };
            review.set_anchor(v.review_anchor as usize);
            self.scene.review = Some(review);
        } else {
            self.scene.review = None;
            self.review_identity.clear();
            self.rendered_identity.clear();
            self.displayed_at = None;
            self.local_review = false;
        }
        if self.fetching.is_none()
            && let Some(b) =
                wanted.filter(|b| self.blobs.peek(&b.sha256).is_none() && self.unavailable.as_ref() != Some(&b.sha256))
        {
            let task = v.page == 1
                && self.replica.model.sessions.get(&v.selected_session_id).is_some_and(|s| s.approval.is_none());
            if task
                && let Some((id, _, at)) = &self.shown_task
                && id == &v.selected_session_id
                && now.saturating_sub(*at) < 5000
            {
                self.refresh_at = Some(*at + 5000);
                return Ok(());
            }
            self.fetching_meta = Some((v.selected_session_id.clone(), b.clone(), task));
            self.fetching = Some(b.sha256.clone());
            self.emit(Payload::BlobGet(BlobGet { sha256: b.sha256 }))?;
        }
        Ok(())
    }
    pub fn reconcile_receipts(&mut self, persist: &mut impl FnMut(&[Receipt]) -> bool) {
        if !self.connected || !self.replica.ready {
            return;
        }
        let remaining: Vec<_> = self
            .receipts
            .iter()
            .filter(|receipt| {
                !self.replica.model.sessions.values().any(|s| {
                    s.approval.as_ref().is_some_and(|r| {
                        r.id == receipt.request && r.digest == receipt.digest && matches!(r.state, 3 | 5 | 6 | 7)
                    })
                })
            })
            .cloned()
            .collect();
        if remaining.len() != self.receipts.len() && persist(&remaining) {
            self.receipts = remaining;
        }
    }
    pub fn tick(&mut self, now: u64) -> Result<()> {
        if self.refresh_at.is_some_and(|at| now >= at) {
            self.refresh_at = None;
            self.sync(now)?;
        }
        let dt = now.saturating_sub(self.last_tick).min(100) as u32;
        self.last_tick = now;
        self.scene.tick(dt);
        if self.connected {
            self.replica.check_timeout(now)?;
            if now.saturating_sub(self.last_heartbeat) >= 1000 && self.hello {
                self.emit(Payload::Heartbeat(Heartbeat {
                    applied_revision: self.replica.model.revision,
                    monotonic_ms: now,
                }))?;
                self.last_heartbeat = now;
            }
        }
        if self.contact.is_none() && now.saturating_sub(self.last_touch) >= 600000 {
            self.sleeping = true;
        }
        if self.receipt_wait.is_some_and(|t| now.saturating_sub(t) >= 5000) {
            self.receipt_wait = None;
            self.sync(now)?;
        }
        // Re-evaluate arming without replacing scene state every frame.
        if self.armed(now)
            && let Some(r) = &mut self.scene.review
            && r.actionable
            && r.status == "PLEASE WAIT..."
        {
            r.status.clear();
        }
        Ok(())
    }
    pub fn render(&mut self, frame: &mut [u32], now: u64) {
        self.scene.render(
            frame,
            chrono::NaiveTime::from_num_seconds_from_midnight_opt(((now / 1000) % (24 * 3600)) as u32, 0).unwrap(),
        );
        self.feedback(|x, y| frame[y as usize * 480 + x as usize] = 0xffd95f);
    }
    /// Paint only the pressed marker over an already presented frame. This can
    /// bypass a full scene redraw and never arms a decision or changes state.
    pub fn feedback(&self, mut pixel: impl FnMut(i32, i32)) -> bool {
        if let Some(c) = &self.contact
            && !c.cancelled
            && !matches!(c.target, Target::None)
        {
            for (sx, sy) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
                for n in 0..5 {
                    for (x, y) in [(c.x + sx * 10, c.y + sy * (10 - n)), (c.x + sx * (10 - n), c.y + sy * 10)] {
                        if (0..480).contains(&x) && (0..272).contains(&y) {
                            pixel(x, y);
                        }
                    }
                }
            }
            return true;
        }
        false
    }
    /// Platform calls only after the panel finishes presenting this exact render.
    pub fn presented(&mut self, now: u64) {
        if self.scene.review.as_ref().is_some_and(|r| r.actionable) && self.rendered_identity != self.review_identity {
            self.rendered_identity = self.review_identity.clone();
            self.displayed_at = Some(now);
        }
    }
    pub fn armed(&self, now: u64) -> bool {
        self.connected
            && self.replica.ready
            && self.local_review
            && !self.sleeping
            && self.contact.is_none()
            && self.displayed_at.is_some_and(|t| now.saturating_sub(t) >= 500)
            && now.saturating_sub(self.last_touch) >= 500
            && self.rendered_identity == self.review_identity
    }
    fn target(&self, x: i32, y: i32) -> Target {
        if !(0..480).contains(&x) || !(0..272).contains(&y) {
            return Target::None;
        }
        if let Some(r) = &self.scene.review {
            if y < 48 {
                return if x < 120 {
                    Target::Back
                } else if x >= 360 {
                    Target::Desktop(r.request.session.clone())
                } else {
                    Target::None
                };
            }
            if y >= 220 && r.dismiss_pending && (12..432).contains(&x) {
                return Target::Dismiss;
            }
            if y >= 220 {
                return if r.actionable && r.status.is_empty() {
                    if (12..156).contains(&x) && r.allow_deny {
                        Target::Decision(2, self.review_identity.clone())
                    } else if (168..432).contains(&x) && r.allow_approve {
                        Target::Decision(1, self.review_identity.clone())
                    } else {
                        Target::None
                    }
                } else {
                    Target::None
                };
            }
            if x >= 436 {
                return Target::Rail(y >= 142);
            }
            return Target::Body(r.request.session.clone());
        }
        if y >= cafe_ui::assets::WIN_Y {
            if x >= 436 {
                return Target::Rail(y >= (cafe_ui::assets::WIN_Y + 272) / 2);
            }
            return self.scene.selected_id().map_or(Target::None, |s| Target::Body(s.into()));
        }
        self.scene.visible_patron_at(x, y).map_or(Target::None, |s| Target::Patron(s.into()))
    }
    /// Raw contact samples: pressed=1, released=0, cancelled/multiple=2.
    pub fn touch(
        &mut self,
        pressed: u8,
        x: i32,
        y: i32,
        now: u64,
        persist: &mut impl FnMut(&[Receipt]) -> bool,
    ) -> Result<()> {
        if pressed == 2 {
            self.contact = None;
            self.last_tap = None;
            self.last_touch = now;
            return Ok(());
        }
        let was_armed = self.armed(now);
        self.last_touch = now;
        if self.sleeping {
            self.sleeping = false;
            self.wake_contact = true;
            self.contact = None;
            self.last_tap = None;
        }
        if self.wake_contact {
            if pressed == 0 {
                self.wake_contact = false;
            }
            return Ok(());
        }
        if !self.connected || !self.replica.ready {
            return Ok(());
        }
        if pressed == 1 {
            if let Some(c) = &mut self.contact {
                if (x - c.x).abs() > 12 || (y - c.y).abs() > 12 {
                    c.cancelled = true;
                }
                return Ok(());
            }
            self.contact =
                Some(Contact { target: self.target(x, y), x, y, down: now, cancelled: false, armed: was_armed });
            return Ok(());
        }
        let Some(c) = self.contact.take() else { return Ok(()) };
        if c.cancelled
            || now.saturating_sub(c.down) > 800
            || (x - c.x).abs() > 12
            || (y - c.y).abs() > 12
            || c.target != self.target(x, y)
        {
            self.last_tap = None;
            return Ok(());
        }
        let double = self.last_tap.as_ref().is_some_and(|(t, time, px, py)| {
            *t == c.target && now.saturating_sub(*time) <= 450 && (x - px).abs() <= 20 && (y - py).abs() <= 20
        });
        self.last_tap = if double { None } else { Some((c.target.clone(), now, x, y)) };
        match c.target {
            Target::Patron(id) => {
                if double {
                    self.issue(Action { kind: 6, session_id: id, ..Default::default() }, now)?;
                } else {
                    let selected = self.view().selected_session_id;
                    self.issue(
                        Action {
                            kind: 1,
                            session_id: if selected == id { String::new() } else { id },
                            ..Default::default()
                        },
                        now,
                    )?;
                }
            }
            Target::Body(id) => {
                if self.view().page == 2 {
                    if double {
                        self.issue(Action { kind: 6, session_id: id, ..Default::default() }, now)?;
                    } else {
                        self.local_review = true;
                        self.displayed_at = None;
                        self.rendered_identity.clear();
                        self.sync(now)?;
                    }
                } else if double {
                    let s = &self.replica.model.sessions[&id];
                    let (kind, request, b) = if let Some(r) = &s.approval {
                        (1, r.id.clone(), r.body_blob.clone())
                    } else if s.unlinked_review_blob.is_some() {
                        (3, String::new(), s.unlinked_review_blob.clone())
                    } else {
                        (2, String::new(), s.task_blob.clone())
                    };
                    if let Some(b) = b {
                        self.issue(
                            Action {
                                kind: 2,
                                session_id: id,
                                request_id: request,
                                blob_id: b.sha256,
                                review_kind: kind,
                                ..Default::default()
                            },
                            now,
                        )?;
                        self.local_review = true;
                        self.sync(now)?;
                    }
                }
            }
            Target::Back => {
                self.issue(Action { kind: 3, ..Default::default() }, now)?;
            }
            Target::Desktop(id) => {
                self.issue(Action { kind: 6, session_id: id, ..Default::default() }, now)?;
            }
            Target::Rail(forward) => {
                let v = self.view();
                if v.page == 2 {
                    if let Some(r) = &mut self.scene.review {
                        r.page(forward);
                        let anchor = r.anchor();
                        self.issue(
                            Action {
                                kind: 5,
                                session_id: v.selected_session_id,
                                request_id: v.review_request_id,
                                blob_id: v.review_blob.unwrap().sha256,
                                anchor: Some(anchor as u32),
                                ..Default::default()
                            },
                            now,
                        )?;
                    }
                } else {
                    self.scene.page_task(forward);
                    let anchor = self.scene.task_anchor();
                    if let Some(b) =
                        self.replica.model.sessions.get(&v.selected_session_id).and_then(|s| s.task_blob.as_ref())
                    {
                        self.issue(
                            Action {
                                kind: 4,
                                session_id: v.selected_session_id,
                                blob_id: b.sha256.clone(),
                                anchor: Some(anchor as u32),
                                ..Default::default()
                            },
                            now,
                        )?;
                    }
                }
            }
            Target::Dismiss => {
                let current = self.scene.review.as_ref().map(|r| r.request.id.clone()).unwrap_or_default();
                let next = if self.receipts.len() >= 8 {
                    Vec::new()
                } else {
                    self.receipts.iter().filter(|r| r.request != current).cloned().collect()
                };
                if persist(&next) {
                    self.receipts = next;
                    self.displayed_at = None;
                    self.rendered_identity.clear();
                    self.last_tap = None;
                    self.sync(now)?;
                }
            }
            Target::Decision(decision, identity) => {
                if c.armed && identity == self.review_identity && self.receipts.len() < 8 {
                    let v = self.view();
                    let s = &self.replica.model.sessions[&v.selected_session_id];
                    let r = s.approval.as_ref().ok_or(Error::Invalid)?;
                    let a = Action {
                        kind: 7,
                        session_id: s.id.clone(),
                        request_id: r.id.clone(),
                        request_digest: r.digest.clone(),
                        displayed_body_sha256: r.body_blob.as_ref().unwrap().sha256.clone(),
                        decision,
                        ..Default::default()
                    };
                    let receipt = Receipt {
                        authority: self.authority.clone(),
                        session: s.id.clone(),
                        request: r.id.clone(),
                        digest: r.digest.clone(),
                        decision,
                    };
                    if self.intents.len() >= 32 || self.outgoing.len() >= 32 {
                        return Err(Error::Limit);
                    }
                    let mut next = self.receipts.clone();
                    next.push(receipt);
                    if persist(&next) {
                        self.receipts = next;
                        self.receipt_wait = Some(now);
                        self.issue(a, now)?;
                    } else if let Some(r) = &mut self.scene.review {
                        r.status = "STORAGE ERROR - OPEN DESKTOP".into();
                        r.actionable = false;
                    }
                }
            }
            Target::None => (),
        }
        Ok(())
    }
}
