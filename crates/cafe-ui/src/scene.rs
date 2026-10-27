//! The café: seats, patrons, walking, poses, particles and the message
//! window, behaving as on the Token Café design page (design/cafe.js). The
//! collector only says which sessions exist and what state each is in;
//! everything that moves is decided here.

#[cfg(not(feature = "std"))]
use crate::FloatMath;
use alloc::collections::BTreeMap as HashMap;
use alloc::{
    borrow::ToOwned,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::f64::consts::PI;

use chrono::{NaiveTime, Timelike};

use crate::assets::{self as a, Pal, Sprite, base, c, slot};
use crate::gfx::{Gfx, text_width};
#[cfg(feature = "std")]
use crate::patrons::Animation;

// Compact immutable runs live in flash. Decoding writes the frame sequentially,
// avoiding both a 510 KiB heap copy and a full-frame flash read every draw.
static BACKGROUND_RUNS: &[u32] = &include!(concat!(env!("OUT_DIR"), "/background_runs.rs"));
fn paint_background(mut frame: &mut [u32]) {
    if frame.len() == a::W * a::H * 4 {
        let mut offset = 0;
        for &run in BACKGROUND_RUNS {
            for _ in 0..(run >> 24) {
                let x = offset % a::W * 2;
                let y = offset / a::W * 2;
                for dy in 0..2 {
                    for dx in 0..2 {
                        frame[(y + dy) * a::W * 2 + x + dx] = run & 0xffffff;
                    }
                }
                offset += 1;
            }
        }
        return;
    }
    for &run in BACKGROUND_RUNS {
        let (pixels, rest) = frame.split_at_mut((run >> 24) as usize);
        pixels.fill(run & 0xffffff);
        frame = rest;
    }
    debug_assert!(frame.is_empty());
}

pub const SEATS: usize = 8;
const TASK_LINE_LEN: usize = 32; // between the portrait and scrollbar touch target
const PLATE_LEN: usize = 13; // shown on the desk nameplate

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Claude,
    Codex,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Work,
    Idle,
    Wait,
}

/// Why a waiting session waits: a permission prompt ("!") or anything else ("?").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Why {
    None,
    Perm,
    Ask,
}

/// A forced pose, for the demo; Auto lets the scene choose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ForcedPose {
    Auto,
    Think,
    Stretch,
    Sip,
    Sleep,
}

#[derive(Clone, Debug)]
pub struct Session {
    /// Stable per session: the patron's looks are a hash of it.
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub state: State,
    pub why: Why,
    pub pose: ForcedPose,
    /// How long the session has been in this state, so a restarted app
    /// still knows who has been idle long enough to doze off.
    pub age_s: u32,
    pub branch: Option<String>,
    /// Active direct children observed in open rollout files; None when unavailable.
    pub active_subagents: Option<usize>,
}

/// Navigation to an existing desktop chat or an identified terminal pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionTarget {
    Claude(String),
    Codex(String),
    ITerm(String),
}

impl SessionTarget {
    pub fn url(&self) -> Option<String> {
        if let Self::ITerm(id) = self {
            // The reveal handler only selects an existing session. Do not allow
            // query parameters, other handlers, or arbitrary URLs from the environment.
            if id.is_empty()
                || id.len() > 128
                || !id.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b':'))
            {
                return None;
            }
            return Some(format!("iterm2:reveal?sessionid={id}"));
        }
        let (id, prefix) = match self {
            Self::Claude(id) => (id.strip_prefix("local_")?, "claude://code/continue?session=local_"),
            Self::Codex(id) => (id.as_str(), "codex://threads/"),
            Self::ITerm(_) => unreachable!(),
        };
        // Session IDs are data, never arbitrary URLs or shell arguments.
        if id.is_empty() || id.len() > 64 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
            return None;
        }
        Some(format!("{prefix}{id}"))
    }
}

/// Short, evidence-backed display fields, separate from the animated session state.
#[derive(Clone, Debug)]
pub struct SessionDetail {
    pub target: Option<SessionTarget>,
    pub id: String,
    pub task: String,
}

/// This month's totals for the tab on the wall, Claude and Codex summed.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Usage {
    pub tokens: f64,
    pub usd: f64,
}

#[derive(Clone, PartialEq, Debug)]
pub struct UsageRow {
    pub label: String,
    pub tokens: f64,
    pub usd: f64,
}

/* ---- layout, as in design/cafe.js --------------------------------------- */

const COLS: i32 = 4;
const STATION_W: i32 = 120;
const STATION_H: i32 = 82;
const FLOOR_Y: i32 = 48;
const SIT_X: i32 = 32; // seated base box inside a station
const SIT_Y: i32 = 12;
const CHAIR_X: i32 = 30;
const CHAIR_Y: i32 = 2;
const DESK_X: i32 = -4;
const DESK_Y: i32 = 42;
const DESK_W: i32 = 88;
const LID_X: i32 = 34;
const LID_Y: i32 = 36;
const BX: i32 = 3; // base box offset inside character sprites
const BY: i32 = 6;
const WALK_SPEED: f64 = 60.0; // px per second
const THINK_EVERY: i64 = 11000;
const THINK_FOR: i64 = 2600;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Seated,
    Arriving,
    Swapping,
    Leaving,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pose {
    Type,
    Think,
    Raise,
    Stretch,
    Sip,
    Sleep,
}

struct Agent {
    id: String,
    name: String,
    codex: bool,
    branch: Option<String>,
    active_subagents: Option<usize>,
    state: State,
    why: Why,
    force: ForcedPose,
    pal: Pal,
    style: usize,
    variant: u32,
    phase: i64,
    seat: usize,
    mode: Mode,
    x: f64,
    y: f64,
    dist: f64,
    dir: Dir,
    path: Vec<[f64; 2]>,
    path_i: usize,
    since: i64,
    zacc: i64,
    seen: bool,
    gone: bool,
}

impl Agent {
    fn remaining(&self) -> usize {
        self.path.len() - self.path_i
    }

    fn start_walk(&mut self, path: &[[f64; 2]], mode: Mode) {
        self.mode = mode;
        self.x = path[0][0];
        self.y = path[0][1];
        self.dist = 0.0;
        self.path = path[1..].to_vec();
        self.path_i = 0;
    }
}

#[derive(Clone, Copy)]
enum PKind {
    Z,
    Steam,
    Key,
}

#[derive(Clone, Copy)]
struct Particle {
    kind: PKind,
    x: f64,
    y: f64,
    age: i64,
    life: i64,
    seat: usize,
    glyph: usize,
    color: u32,
}

impl Particle {
    fn new(kind: PKind, x: f64, y: f64, life: i64) -> Self {
        Particle { kind, x, y, age: 0, life, seat: 0, glyph: 0, color: 0 }
    }
}

pub struct Scene {
    #[cfg(feature = "std")]
    pub patrons: crate::patrons::Patrons,
    #[cfg(feature = "std")]
    pub selected_seat: Option<usize>,
    external_seats: Option<HashMap<String, usize>>,
    /// Milliseconds of animation so far.
    t: i64,
    /// In arrival order.
    agents: Vec<Agent>,
    particles: Vec<Particle>,
    focus: usize,
    attention: HashMap<String, Option<i64>>,
    automatic_attention: Option<String>,
    resolved_attention: HashMap<String, (i64, Option<String>)>,
    appearance_seeds: HashMap<String, u32>,
    overflow: usize,
    /// No list yet: the first one seats everyone at once.
    first: bool,
    usage: Option<Usage>,
    usage_text: Option<(String, String)>,
    usage_rows: Vec<UsageRow>,
    usage_account_rows: Vec<UsageRow>,
    usage_accounts: bool,
    usage_page: usize,
    usage_open: bool,
    sessions: Vec<Session>,
    details: Vec<SessionDetail>,
    pages: HashMap<String, Vec<String>>,
    unavailable: [String; 1],
    selected: Option<String>,
    task_page: usize,
    requests: Vec<crate::approval::Request>,
    pub review: Option<crate::review::Review>,
    pub handoff_request: Option<String>,
    footer_origin: Option<String>,
}

struct Geom {
    sx: i32,
    sy: i32,
    col: i32,
}

fn seat_geom(i: usize) -> Geom {
    let (col, row) = (i as i32 % COLS, i as i32 / COLS);
    Geom { sx: col * STATION_W + 20, sy: FLOOR_Y + row * STATION_H, col }
}

/// Feet positions from the door (under the message window) to a chair.
fn path_to(i: usize) -> [[f64; 2]; 5] {
    let g = seat_geom(i);
    let gap = ((g.col + i32::from(g.col < COLS / 2)) * STATION_W) as f64;
    let aisle = (g.sy + 76) as f64;
    let (sx, sy) = (g.sx as f64, g.sy as f64);
    [[240.0, 294.0], [240.0, aisle], [gap, aisle], [gap, sy + 42.0], [sx + 40.0, sy + 42.0]]
}

/// Two offset walking lanes keep a swap readable. Feet leave each chair via
/// the gap between columns, travel below the desks, then approach the new chair.
fn swap_path(from: usize, to: usize, lane: f64) -> [[f64; 2]; 6] {
    let a = seat_geom(from);
    let b = seat_geom(to);
    let aisle = if a.col < 2 { 120.0 } else { 360.0 };
    let x = aisle + lane;
    [
        [(a.sx + 40) as f64, (a.sy + 42) as f64],
        [x, (a.sy + 42) as f64],
        [x, (a.sy + 72) as f64],
        [x, (b.sy + 72) as f64],
        [x, (b.sy + 42) as f64],
        [(b.sx + 40) as f64, (b.sy + 42) as f64],
    ]
}

fn seat_props(i: usize) -> &'static [(&'static Sprite, i32, i32)] {
    const P: [&[(&Sprite, i32, i32)]; SEATS] = [
        &[(&a::PROP_PLANT, 15, 25)],
        &[(&a::PROP_DUCK, 56, 29)],
        &[(&a::PROP_NOODLES, 16, 29), (&a::PROP_CAN, 61, 29)],
        &[(&a::PROP_BOOKS, 54, 30)],
        &[(&a::PROP_CAN, 16, 29)],
        &[(&a::PROP_PLANT, 57, 25)],
        &[(&a::PROP_DUCK, 15, 29)],
        &[(&a::PROP_NOODLES, 56, 29)],
    ];
    P[i]
}

/* ---- looks -------------------------------------------------------------- */

fn fnv1a(s: &str) -> u32 {
    s.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619))
}

struct Look {
    pal: Pal,
    style: usize,
    variant: u32,
    phase: i64,
}

/// Skin, hair colour and style, shirt and idle preference: a hash of the
/// session id, so a patron looks the same every time.
pub(crate) fn review_appearance(id: &str) -> (Pal, usize) {
    let look = look_for(id);
    (look.pal, look.style)
}

fn look_for(id: &str) -> Look {
    look_seed(fnv1a(id))
}
fn look_seed(h: u32) -> Look {
    let sk = a::SKINS[h as usize % a::SKINS.len()];
    let hr = a::HAIRS[(h >> 3) as usize % a::HAIRS.len()];
    let sh = a::SHIRTS[(h >> 7) as usize % a::SHIRTS.len()];
    let mut pal = Pal::base();
    pal.set(slot::S, sk[0]);
    pal.set(slot::s, sk[1]);
    pal.set(slot::H, hr[0]);
    pal.set(slot::h, hr[1]);
    pal.set(slot::J, hr[2]);
    pal.set(slot::C, sh[0]);
    pal.set(slot::c, sh[1]);
    pal.set(slot::V, a::LOOK_V);
    pal.set(slot::P, a::PANTS[(h >> 11) as usize % a::PANTS.len()]);
    pal.set(slot::F, a::LOOK_F);
    Look { pal, style: (h >> 20) as usize % a::POSES.len(), variant: (h >> 13) % 3, phase: ((h >> 16) % 9000) as i64 }
}

/// 'mithrandir-1c' -> 'mithr-1c' for n=8: keep the session suffix.
pub fn shorten(name: &str, n: usize) -> String {
    if name.chars().count() <= n {
        return name.to_string();
    }
    if let Some((base, suf)) = name.rsplit_once('-') {
        let sl = suf.chars().count();
        if !base.is_empty() && (1..=3).contains(&sl) && n > sl + 1 {
            let cut: String = base.chars().take(n - 1 - sl).collect();
            return format!("{}-{suf}", cut.trim_end_matches(['-', '_', ' ']));
        }
    }
    name.chars().take(n).collect()
}

fn jround(v: f64) -> i32 {
    (v + 0.5).floor() as i32 // Math.round
}

fn pose_of(t: i64, a: &Agent) -> Pose {
    match a.state {
        State::Wait => return Pose::Raise,
        State::Work => {
            let ph = (t + a.phase) % THINK_EVERY;
            return if a.force == ForcedPose::Think || ph < THINK_FOR { Pose::Think } else { Pose::Type };
        }
        State::Idle => {}
    }
    match a.force {
        ForcedPose::Think => return Pose::Think,
        ForcedPose::Stretch => return Pose::Stretch,
        ForcedPose::Sip => return Pose::Sip,
        ForcedPose::Sleep => return Pose::Sleep,
        ForcedPose::Auto => {}
    }
    // idle: the longer the human leaves them, the deeper the slack
    let idle = t - a.since;
    if idle > 10 * 60000 {
        Pose::Sleep
    } else if idle > 2 * 60000 || a.variant == 2 {
        Pose::Sip
    } else {
        Pose::Stretch
    }
}

/* ---- the tab: this month's tokens and dollars, both tools summed -------- */

const TAB_X: i32 = 397;
const TAB_Y: i32 = 5;
const TAB_W: i32 = 74;

/// At most 5 characters (6 for "$10.0K"); floor() rounding as in cafe.js.
fn tenths(v: f64, unit: char) -> String {
    let t = (v + 0.5).floor() as i64;
    format!("{}.{}{}", t / 10, t % 10, unit)
}

pub fn fmt_tokens(v: f64) -> String {
    if v < 1e3 {
        format!("{}", v.floor() as i64)
    } else if v < 1e6 {
        format!("{}K", (v / 1e3 + 0.5).floor() as i64)
    } else if v < 1e7 {
        tenths(v / 1e5, 'M')
    } else if v < 1e9 {
        format!("{}M", (v / 1e6 + 0.5).floor() as i64)
    } else if v < 1e11 {
        tenths(v / 1e8, 'B')
    } else {
        format!("{}B", (v / 1e9 + 0.5).floor() as i64)
    }
}

pub fn fmt_usd(v: f64) -> String {
    if v < 999.5 {
        format!("${}", (v + 0.5).floor() as i64)
    } else if v < 9999.5 {
        format!("${}", tenths(v / 100.0, 'K'))
    } else {
        format!("${}K", (v / 1000.0 + 0.5).floor() as i64)
    }
}

enum Item {
    Chair(usize),
    Desk(usize),
    Agent(usize),
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

fn ellipsize(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_owned()
    } else {
        format!("{}...", text.chars().take(width.saturating_sub(3)).collect::<String>())
    }
}

/// Word-wrap in character cells; never scroll or cut a multi-byte UTF-8 character.
fn task_pages(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec!["TASK UNAVAILABLE".into()];
    }
    let lines: Vec<_> = crate::text::layout(text, TASK_LINE_LEN).into_iter().map(|line| line.text).collect();
    lines.chunks(2).map(|lines| lines.join("\n")).collect()
}

impl Scene {
    pub fn new() -> Self {
        Scene {
            external_seats: None,
            #[cfg(feature = "std")]
            patrons: crate::patrons::Patrons::default(),
            #[cfg(feature = "std")]
            selected_seat: None,
            t: 0,
            agents: Vec::new(),
            particles: Vec::new(),
            focus: 0,
            attention: HashMap::new(),
            automatic_attention: None,
            resolved_attention: HashMap::new(),
            appearance_seeds: HashMap::new(),
            overflow: 0,
            first: true,
            usage: None,
            usage_text: None,
            usage_rows: Vec::new(),
            usage_account_rows: Vec::new(),
            usage_accounts: false,
            usage_page: 0,
            usage_open: false,
            sessions: Vec::new(),
            details: Vec::new(),
            pages: HashMap::new(),
            unavailable: ["TASK UNAVAILABLE".into()],
            selected: None,
            task_page: 0,
            requests: Vec::new(),
            review: None,
            handoff_request: None,
            footer_origin: None,
        }
    }

    fn seat_free(&self, seat: usize) -> bool {
        !self.agents.iter().any(|a| a.seat == seat)
    }

    /// Reconcile with the collector's full session list. The first list
    /// seats everyone at once; later newcomers walk in and leavers walk out.
    /// Whoever finds no free seat waits outside.
    pub fn set_sessions(&mut self, list: &[Session]) {
        for session in list {
            if session.state == State::Wait
                && !self
                    .sessions
                    .iter()
                    .any(|old| old.id == session.id && old.state == State::Wait && old.why == session.why)
            {
                self.notify_attention(&session.id);
            }
        }
        let ended: Vec<_> = self
            .sessions
            .iter()
            .filter(|old| {
                old.state == State::Wait && list.iter().any(|new| new.id == old.id && new.state != State::Wait)
            })
            .map(|s| s.id.clone())
            .collect();
        for id in ended {
            self.mark_resolved(&id);
        }
        self.resolved_attention.retain(|id, _| list.iter().any(|s| &s.id == id));
        self.appearance_seeds.retain(|id, _| list.iter().any(|s| &s.id == id));
        self.sessions = list.to_vec();
        self.reconcile_attention();
        if self.selected.as_ref().is_some_and(|id| !list.iter().any(|s| &s.id == id)) {
            self.clear_selection();
        }
        let instant = core::mem::replace(&mut self.first, false);
        for a in &mut self.agents {
            a.seen = false;
        }
        let mut overflow = 0;
        for s in list {
            if self.external_seats.as_ref().is_some_and(|seats| seats.get(&s.id).is_none_or(|seat| *seat >= SEATS)) {
                overflow += 1;
                continue;
            }
            let mut fresh = false;
            let k = match self.agents.iter().position(|a| a.id == s.id) {
                Some(k) => k,
                None => {
                    // Keep the two providers on opposite sides of the aisle.
                    // Never spill into the other provider's seats.
                    let right = s.kind == Kind::Codex;
                    let Some(seat) = self
                        .external_seats
                        .as_ref()
                        .and_then(|seats| seats.get(&s.id).copied())
                        .or_else(|| (0..SEATS).find(|&i| (i % COLS as usize >= 2) == right && self.seat_free(i)))
                    else {
                        overflow += 1;
                        continue;
                    };
                    let look = look_for(&s.id);
                    let mut a = Agent {
                        id: s.id.clone(),
                        name: String::new(),
                        codex: false,
                        branch: None,
                        active_subagents: None,
                        state: State::Work,
                        why: Why::None,
                        force: ForcedPose::Auto,
                        pal: look.pal,
                        style: look.style,
                        variant: look.variant,
                        phase: look.phase,
                        seat,
                        mode: Mode::Seated,
                        x: 0.0,
                        y: 0.0,
                        dist: 0.0,
                        dir: Dir::Up,
                        path: Vec::new(),
                        path_i: 0,
                        since: 0,
                        zacc: 0,
                        seen: false,
                        gone: false,
                    };
                    if !instant {
                        a.start_walk(&path_to(seat), Mode::Arriving);
                    }
                    fresh = true;
                    self.agents.push(a);
                    self.agents.len() - 1
                }
            };
            let a = &mut self.agents[k];
            // Discovery can briefly omit a live process. Restore its occupied
            // seat if it reappears before departure completes.
            if a.mode == Mode::Leaving {
                a.mode = Mode::Seated;
                a.path.clear();
                a.path_i = 0;
                a.gone = false;
            }
            a.seen = true;
            if fresh || a.state != s.state || a.why != s.why {
                a.since = self.t - s.age_s as i64 * 1000;
            }
            a.name = s.name.clone();
            a.branch = s.branch.clone();
            a.active_subagents = s.active_subagents;
            a.codex = s.kind == Kind::Codex;
            a.state = s.state;
            a.why = s.why;
            a.force = s.pose;
        }
        for a in &mut self.agents {
            if a.seen || a.mode == Mode::Leaving {
                continue;
            }
            let full = path_to(a.seat);
            let back: Vec<[f64; 2]> = if a.mode == Mode::Swapping {
                // A session can close mid-swap. Exit from its current position,
                // rather than indexing the unrelated five-point arrival path.
                vec![[a.x, a.y], [240.0, a.y], [240.0, 294.0]]
            } else if a.mode == Mode::Arriving {
                // turn round where they stand
                let k = full.len() - 1 - a.remaining();
                core::iter::once([a.x, a.y]).chain(full[..=k].iter().rev().copied()).collect()
            } else {
                full.iter().rev().copied().collect()
            };
            a.start_walk(&back, Mode::Leaving);
        }
        self.overflow = overflow;
        self.promote_active();
    }

    fn promote_active(&mut self) {
        if self.external_seats.is_some() {
            return;
        }
        let mut resumed: Vec<_> = self
            .agents
            .iter()
            .filter(|a| a.state != State::Idle && a.seen && a.mode == Mode::Seated)
            .map(|a| a.id.clone())
            .collect();
        // Maintain the invariant even on startup, after arrival, or after another
        // swap finishes: no seated active patron trails a seated idle patron.
        resumed.sort_by_key(|id| self.agents.iter().find(|a| &a.id == id).map(|a| a.seat));
        for id in resumed {
            let Some(k) = self.agents.iter().position(|a| a.id == id && a.mode == Mode::Seated) else { continue };
            let from = self.agents[k].seat;
            let right = self.agents[k].codex;
            let candidate = self
                .agents
                .iter()
                .enumerate()
                .filter(|(_, a)| {
                    a.seen && a.codex == right && a.state == State::Idle && a.mode == Mode::Seated && a.seat < from
                })
                .min_by_key(|(_, a)| a.seat)
                .map(|(i, _)| i);
            if let Some(other) = candidate {
                let to = self.agents[other].seat;
                self.agents[k].seat = to;
                self.agents[other].seat = from;
                self.agents[k].start_walk(&swap_path(from, to, -6.0), Mode::Swapping);
                self.agents[other].start_walk(&swap_path(to, from, 6.0), Mode::Swapping);
            }
        }
    }

    /// The authority assigns seats; both renderers animate the same assignment.
    pub fn set_authoritative_sessions(&mut self, list: &[Session], seats: &[(String, usize)]) {
        self.external_seats = Some(seats.iter().cloned().collect());
        self.set_sessions(list);
        for ag in &mut self.agents {
            let Some((_, to)) = seats.iter().find(|(id, seat)| id == &ag.id && *seat < SEATS) else { continue };
            if ag.seat == *to || ag.mode == Mode::Leaving {
                continue;
            }
            let from = ag.seat;
            let mut path = swap_path(from, *to, if from < *to { 6.0 } else { -6.0 });
            if ag.mode != Mode::Seated {
                path[0] = [ag.x, ag.y];
            }
            ag.seat = *to;
            ag.start_walk(&path, Mode::Swapping);
        }
    }
    pub fn set_appearance(&mut self, id: &str, seed: u32) {
        self.appearance_seeds.insert(id.into(), seed);
        if let Some(a) = self.agents.iter_mut().find(|a| a.id == id) {
            let l = look_seed(seed);
            a.pal = l.pal;
            a.style = l.style;
            a.variant = l.variant;
            a.phase = l.phase;
        }
    }
    pub fn set_usage_text(&mut self, tokens: String, cost: String) {
        self.usage_text = Some((tokens, cost));
    }
    pub fn set_usage_rows(&mut self, rows: Vec<UsageRow>) {
        self.usage_rows = rows;
    }
    pub fn set_usage_accounts(&mut self, rows: Vec<UsageRow>) {
        self.usage_account_rows = rows;
        self.usage_page = self.usage_page.min(self.usage_page_count() - 1);
    }
    pub fn toggle_usage_accounts(&mut self) {
        if self.usage_open {
            self.usage_accounts = !self.usage_accounts;
            self.usage_page = 0;
        }
    }
    fn usage_page_count(&self) -> usize {
        let rows = if self.usage_accounts { &self.usage_account_rows } else { &self.usage_rows };
        rows.len().div_ceil(4).max(1)
    }
    fn page_usage(&mut self, forward: bool) {
        let pages = self.usage_page_count();
        self.usage_page = if forward { (self.usage_page + 1) % pages } else { (self.usage_page + pages - 1) % pages };
    }
    pub fn toggle_usage(&mut self) {
        if !self.usage_rows.is_empty() && self.review.is_none() {
            self.usage_open = !self.usage_open;
        }
    }
    pub fn usage_is_open(&self) -> bool {
        self.usage_open
    }
    pub fn selected_id(&self) -> Option<&str> {
        self.selected.as_deref()
    }
    pub fn footer_id(&self) -> Option<&str> {
        self.footer_origin.as_deref().or(self.selected.as_deref())
    }
    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }
    pub fn details(&self) -> &[SessionDetail] {
        &self.details
    }
    pub fn requests(&self) -> &[crate::approval::Request] {
        &self.requests
    }
    pub fn usage(&self) -> Option<Usage> {
        self.usage
    }
    pub fn task_anchor(&self) -> usize {
        let text =
            self.details.iter().find(|d| Some(d.id.as_str()) == self.selected_id()).map_or("", |d| d.task.as_str());
        crate::text::layout(text, TASK_LINE_LEN).get(self.task_page * 2).map_or(0, |l| l.offset)
    }
    pub fn set_task_anchor(&mut self, anchor: usize) {
        let text =
            self.details.iter().find(|d| Some(d.id.as_str()) == self.selected_id()).map_or("", |d| d.task.as_str());
        self.task_page = crate::text::index(&crate::text::layout(text, TASK_LINE_LEN), anchor) / 2;
    }
    pub fn visible_patron_at(&self, x: i32, y: i32) -> Option<&str> {
        if self.review.is_some() {
            return None;
        }
        // Moving sprites get first refusal; never redirect their tap to a seat.
        if let Some(ag) = self
            .agents
            .iter()
            .filter(|ag| ag.seen && matches!(ag.mode, Mode::Arriving | Mode::Swapping))
            .filter(|ag| (x - jround(ag.x)).abs() <= 18 && y >= jround(ag.y) - 46 && y <= jround(ag.y) + 2)
            .max_by_key(|ag| jround(ag.y))
        {
            return Some(&ag.id);
        }
        if !(0..480).contains(&x) || !(FLOOR_Y..a::WIN_Y).contains(&y) {
            return None;
        }
        let seat = ((y - FLOOR_Y) / STATION_H * COLS + x / STATION_W) as usize;
        self.agents.iter().find(|ag| ag.seat == seat && ag.seen && ag.mode == Mode::Seated).map(|ag| ag.id.as_str())
    }

    /// New identities alert once; selecting or synchronizing a view never re-arms them.
    pub fn notify_attention(&mut self, id: &str) {
        self.resolved_attention.remove(id);
        self.attention.insert(id.into(), Some(self.t));
        if self.automatic_attention.is_none() {
            self.automatic_attention = Some(id.into());
        }
    }

    fn mark_resolved(&mut self, id: &str) {
        let task = self.details.iter().find(|d| d.id == id).map(|d| d.task.clone());
        self.resolved_attention.entry(id.into()).or_insert((self.t, task));
    }

    fn reconcile_attention(&mut self) {
        self.attention.retain(|id, _| {
            self.sessions.iter().any(|s| &s.id == id && s.state == State::Wait)
                || self.requests.iter().any(|r| &r.session == id)
        });
        if self.automatic_attention.as_ref().is_none_or(|id| !self.attention.contains_key(id)) {
            self.automatic_attention = self.attention.keys().next().cloned();
        }
    }

    pub fn acknowledge_attention(&mut self, id: &str) {
        if let Some(started) = self.attention.get_mut(id) {
            *started = None;
        }
    }

    pub fn attention_id(&self) -> Option<&str> {
        if let Some(id) = &self.selected {
            return self.attention.contains_key(id).then_some(id.as_str());
        }
        self.automatic_attention.as_deref()
    }

    pub fn next_attention_id(&self) -> Option<&str> {
        let ids: Vec<_> = self.attention.keys().collect();
        if ids.len() < 2 {
            return None;
        }
        let current = self.attention_id();
        let next = ids.iter().position(|id| Some(id.as_str()) == current).map_or(0, |i| (i + 1) % ids.len());
        Some(ids[next].as_str())
    }

    fn attention_bright(&self) -> bool {
        self.attention.values().any(|start| {
            start.is_some_and(|at| {
                let elapsed = self.t - at;
                (0..30_000).contains(&elapsed) && (elapsed / 600) % 2 == 0
            })
        })
    }

    pub fn set_requests(&mut self, requests: Vec<crate::approval::Request>) {
        if let Some(review) = &mut self.review
            && review.actionable
            && !requests.iter().any(|r| r == review.request())
        {
            review.actionable = false;
            if review.status.is_empty() {
                review.status = "REQUEST NO LONGER PENDING".into();
            }
        }
        let ended: Vec<_> = self
            .requests
            .iter()
            .filter(|old| {
                !requests.iter().any(|new| new.session == old.session)
                    && !self.sessions.iter().any(|s| s.id == old.session && s.state == State::Wait)
            })
            .map(|r| r.session.clone())
            .collect();
        for id in ended {
            self.mark_resolved(&id);
        }
        for request in &requests {
            if !self.requests.iter().any(|old| old.id == request.id && old.session == request.session) {
                self.notify_attention(&request.session);
            }
        }
        self.requests = requests;
        self.reconcile_attention();
    }

    pub fn interaction_id(&self, x: i32, y: i32) -> Option<String> {
        if let Some(review) = &self.review {
            return Some(format!("review:{}", review.request().id));
        }
        if y >= a::WIN_Y {
            if x >= 436 && self.attention_id().is_some() {
                return None;
            }
            return Some("footer".into());
        }
        self.patron_at(x, y).map(str::to_owned)
    }

    pub fn review_input(&mut self, double: bool, x: i32, y: i32, can_decide: bool) -> Option<String> {
        if let Some(review) = &mut self.review {
            if y < 48 && x < 100 {
                self.review = None;
                return None;
            }
            if let Some(n) = review.choice_at(x, y) {
                if !double {
                    review.selected_choice = Some(n);
                }
                return None;
            }
            if (y < 48 && x >= 380) || (double && (72..212).contains(&y) && x < 436) {
                let id = review.request().session.clone();
                self.handoff_request = Some(review.request().id.clone());
                self.review = None;
                return self.session_target(&id).and_then(SessionTarget::url);
            }
            if x >= 436 && (72..212).contains(&y) {
                review.page(y >= 142);
            }
            if !double && can_decide && (220..264).contains(&y) && review.actionable && review.status.is_empty() {
                review.decision = if review.request().choices > 0 {
                    if (168..432).contains(&x) {
                        review.selected_choice.map(crate::approval::Decision::Choice)
                    } else {
                        None
                    }
                } else if (12..156).contains(&x) {
                    Some(crate::approval::Decision::Deny)
                } else if (168..432).contains(&x) {
                    Some(crate::approval::Decision::Allow)
                } else {
                    None
                };
                if review.decision.is_some() {
                    review.status = "SENDING DECISION...".into();
                }
            }
            return None;
        }
        if double && y >= a::WIN_Y && x < 436 {
            let id = self
                .footer_origin
                .take()
                .filter(|id| {
                    self.sessions.iter().any(|s| &s.id == id) || self.requests.iter().any(|r| &r.session == id)
                })
                .or_else(|| self.selected.clone())?;
            self.selected = Some(id.clone());
            let request = self.requests.iter().find(|r| r.session == id).cloned();
            let actionable = request.is_some();
            let permission = self.sessions.iter().any(|s| s.id == id && s.state == State::Wait && s.why == Why::Perm);
            let request = request.unwrap_or_else(|| crate::approval::Request { choices: 0,
                tool_name: String::new(),
                id: String::new(),
                session: id.clone(),
                title: if permission { "APPROVAL ON DESKTOP" } else { "QUESTION / TASK" }.into(),
                text: if permission {
                    "Claude or Codex is waiting for permission.\n\nThis request is not linked to board approval.\nCheck the original session on your computer to read and answer the exact request.".into()
                } else { self
                    .details
                    .iter()
                    .find(|d| d.id == id)
                    .map_or("QUESTION UNAVAILABLE".into(), |d| d.task.clone()) },
            });
            self.review = Some(crate::review::Review::new(request, actionable));
        }
        None
    }

    pub fn set_details(&mut self, details: Vec<SessionDetail>) {
        let old = self.details.iter().find(|d| Some(&d.id) == self.selected.as_ref()).map(|d| &d.task);
        let new = details.iter().find(|d| Some(&d.id) == self.selected.as_ref()).map(|d| &d.task);
        if old != new {
            self.task_page = 0;
        }
        self.pages.retain(|id, _| details.iter().any(|d| &d.id == id));
        for detail in &details {
            if !self.details.iter().any(|old| old.id == detail.id && old.task == detail.task) {
                self.pages.insert(detail.id.clone(), task_pages(&detail.task));
            }
        }
        self.details = details;
    }

    pub fn clear_selection(&mut self) {
        self.usage_open = false;
        self.footer_origin = None;
        self.review = None;
        self.selected = None;
        self.task_page = 0;
    }

    pub fn select_next(&mut self) {
        self.select_patron(true);
    }

    pub fn select_patron(&mut self, forward: bool) {
        if self.review.is_some() || self.usage_open {
            return;
        }
        if self.sessions.is_empty() {
            return;
        }
        let next = self
            .sessions
            .iter()
            .position(|s| Some(&s.id) == self.selected.as_ref())
            .map_or(if forward { 0 } else { self.sessions.len() - 1 }, |i| {
                (i + if forward { 1 } else { self.sessions.len() - 1 }) % self.sessions.len()
            });
        let id = self.sessions[next].id.clone();
        self.acknowledge_attention(&id);
        self.selected = Some(id);
        self.task_page = 0;
    }

    /// Arrow navigation opens focus first and stops at document boundaries.
    pub fn page_task_key(&mut self, forward: bool) {
        if self.usage_open {
            self.page_usage(forward);
            return;
        }
        if let Some(review) = &mut self.review {
            review.page(forward);
            return;
        }
        if self.selected.is_none() {
            self.select_next();
            return;
        }
        let last = self.selected_task_pages().len() - 1;
        self.task_page = if forward {
            self.task_page.saturating_add(1).min(last)
        } else {
            self.task_page.saturating_sub(1).min(last)
        };
    }

    pub fn page_task(&mut self, forward: bool) {
        let n = self.selected_task_pages().len();
        self.task_page = if forward { (self.task_page + 1) % n } else { (self.task_page + n - 1) % n };
    }

    fn selected_task_pages(&self) -> &[String] {
        self.selected.as_ref().and_then(|id| self.pages.get(id)).map_or(&self.unavailable, Vec::as_slice)
    }

    pub fn scrollbar_hit(&self, x: i32, y: i32) -> bool {
        !self.usage_open && self.attention_id().is_none() && self.selected.is_some() && x >= 436 && y >= a::WIN_Y
    }

    pub fn scroll_task_to(&mut self, y: i32) {
        let n = self.selected_task_pages().len();
        let thumb_h = ((a::H as i32 - a::WIN_Y - 12) / n as i32).max(8);
        let travel = a::H as i32 - a::WIN_Y - 12 - thumb_h;
        if travel > 0 {
            let position = (y - a::WIN_Y - 6 - thumb_h / 2).clamp(0, travel);
            self.task_page = ((position * (n - 1) as i32 + travel / 2) / travel) as usize;
        }
    }

    pub fn patron_at(&self, x: i32, y: i32) -> Option<&str> {
        if self.usage_open {
            return None;
        }
        if self.external_seats.is_some() {
            return self.visible_patron_at(x, y);
        }
        if self.review.is_some() {
            return None;
        }
        if !(0..480).contains(&x) || !(FLOOR_Y..a::WIN_Y).contains(&y) {
            return None;
        }
        let seat = ((y - FLOOR_Y) / STATION_H * COLS + x / STATION_W) as usize;
        self.agents
            .iter()
            .find(|ag| ag.seat == seat && ag.seen && !matches!(ag.mode, Mode::Leaving | Mode::Swapping))
            .map(|ag| ag.id.as_str())
    }

    pub fn session_target(&self, id: &str) -> Option<&SessionTarget> {
        self.details.iter().find(|d| d.id == id).and_then(|d| d.target.as_ref())
    }

    /// Resolve a patron activation shared by mouse double-click and companion double-tap.
    pub fn activate_patron(&mut self, x: i32, y: i32) -> Option<String> {
        let id = self.patron_at(x, y)?.to_owned();
        self.focus_patron(id.clone());
        self.session_target(&id).and_then(SessionTarget::url)
    }

    /// A single click on an identified CLI patron reveals its existing pane.
    /// Desktop chats retain their separate selection/double-click behavior.
    pub fn activate_terminal_patron(&mut self, x: i32, y: i32) -> Option<String> {
        let id = self.patron_at(x, y)?.to_owned();
        let target = self.session_target(&id)?;
        if !matches!(target, SessionTarget::ITerm(_)) {
            return None;
        }
        let url = target.url()?;
        self.acknowledge_attention(&id);
        self.focus_patron(id);
        Some(url)
    }

    pub fn focus_patron(&mut self, id: String) {
        self.footer_origin = None;
        if self.selected.as_ref() != Some(&id) {
            self.task_page = 0;
        }
        self.selected = Some(id);
    }

    /// Coordinates are in the original 480x272 frame, after removing letterboxing.
    pub fn click(&mut self, x: i32, y: i32) {
        if self.review.is_some() {
            self.review_input(false, x, y, false);
            return;
        }
        if self.usage_open {
            if x >= 432 && y < 44 {
                self.usage_open = false;
            } else if (42..60).contains(&y) {
                self.usage_accounts = x >= 164;
                self.usage_page = 0;
            } else if (220..240).contains(&y) {
                self.page_usage(x >= 360);
            }
            return;
        }
        if (TAB_X..TAB_X + TAB_W).contains(&x) && (TAB_Y..TAB_Y + 28).contains(&y) {
            self.toggle_usage();
            return;
        }
        if y >= a::WIN_Y {
            if let Some(id) = self.attention_id().map(str::to_owned) {
                if x >= 436 {
                    if let Some(next) = self.next_attention_id().map(str::to_owned) {
                        self.acknowledge_attention(&next);
                        self.focus_patron(next);
                    }
                } else {
                    self.acknowledge_attention(&id);
                    self.focus_patron(id.clone());
                    self.footer_origin = Some(id);
                    if y >= a::WIN_Y + 38 && (50..190).contains(&x) {
                        self.review_input(true, x, y, false);
                    }
                }
                return;
            }
            self.footer_origin = self.selected.clone();
            if self.selected.as_ref().is_some_and(|id| {
                self.requests.iter().any(|r| &r.session == id)
                    || self.sessions.iter().any(|s| &s.id == id && s.state == State::Wait)
            }) && x < 436
            {
                return;
            }
            if self.selected.is_none() || x < 436 {
                self.select_next();
            } else {
                self.scroll_task_to(y);
            }
            return;
        }
        if y < FLOOR_Y {
            return;
        }
        let seat = ((y - FLOOR_Y) / STATION_H * COLS + x / STATION_W) as usize;
        if let Some(ag) = self
            .agents
            .iter()
            .find(|ag| ag.seat == seat && ag.seen && !matches!(ag.mode, Mode::Leaving | Mode::Swapping))
        {
            let id = ag.id.clone();
            if self.selected.as_ref() == Some(&ag.id) {
                self.clear_selection();
            } else {
                self.selected = Some(ag.id.clone());
                self.task_page = 0;
            }
            self.acknowledge_attention(&id);
        } else {
            self.clear_selection();
        }
    }

    pub fn set_usage(&mut self, usage: Usage) {
        self.usage = Some(usage);
    }

    /// Seated agents waiting for the human, longest wait first.
    fn waiting_list(&self) -> Vec<usize> {
        let mut w: Vec<usize> = (0..self.agents.len())
            .filter(|&i| self.agents[i].mode == Mode::Seated && self.agents[i].state == State::Wait)
            .collect();
        w.sort_by_key(|&i| self.agents[i].since);
        w
    }

    pub fn tick(&mut self, dt: u32) {
        self.t += dt as i64;
        let t = self.t;
        let blocked: Vec<usize> = self
            .agents
            .iter()
            .filter(|ag| ag.mode == Mode::Leaving)
            .filter(|ag| {
                let geo = seat_geom(ag.seat);
                (ag.x - (geo.sx + 40) as f64).abs() < 30.0 && (ag.y - (geo.sy + 42) as f64).abs() < 30.0
            })
            .map(|ag| ag.seat)
            .collect();
        for a in &mut self.agents {
            if self.external_seats.is_some() && a.mode == Mode::Arriving && a.path_i == 0 && blocked.contains(&a.seat) {
                continue;
            }
            if a.mode == Mode::Seated {
                continue;
            }
            let mut step = WALK_SPEED * dt as f64 / 1000.0;
            while step > 0.0 && a.remaining() > 0 {
                let [tx, ty] = a.path[a.path_i];
                let (dx, dy) = (tx - a.x, ty - a.y);
                let d = dx.hypot(dy);
                if d > 0.0 {
                    a.dir = if dx.abs() > dy.abs() {
                        if dx > 0.0 { Dir::Right } else { Dir::Left }
                    } else if dy > 0.0 {
                        Dir::Down
                    } else {
                        Dir::Up
                    };
                }
                if d <= step {
                    a.x = tx;
                    a.y = ty;
                    step -= d;
                    a.dist += d;
                    a.path_i += 1;
                } else {
                    a.x += (dx / d) * step;
                    a.y += (dy / d) * step;
                    a.dist += step;
                    step = 0.0;
                }
            }
            if a.remaining() == 0 {
                if matches!(a.mode, Mode::Arriving | Mode::Swapping) {
                    a.mode = Mode::Seated;
                    a.since = t;
                } else {
                    a.gone = true;
                }
            }
        }
        self.agents.retain(|a| !a.gone);
        self.promote_active();
        for p in &mut self.particles {
            p.age += dt as i64;
        }
        self.particles.retain(|p| p.age < p.life);
        for a in &mut self.agents {
            if a.mode != Mode::Seated {
                continue;
            }
            let pose = pose_of(t, a);
            let g = seat_geom(a.seat);
            a.zacc += dt as i64;
            if pose == Pose::Sleep && a.zacc > 900 {
                a.zacc = 0;
                self.particles.push(Particle::new(PKind::Z, (g.sx + 46) as f64, (g.sy + 12) as f64, 2200));
            }
            if pose == Pose::Sip && a.zacc > 500 {
                a.zacc = 0;
                let up = ((t + a.phase) / 1600) % 2 == 0;
                let mut p = Particle::new(
                    PKind::Steam,
                    (g.sx + SIT_X + if up { 10 } else { 11 }) as f64,
                    (g.sy + SIT_Y + if up { 6 } else { 11 }) as f64,
                    1400,
                );
                p.seat = a.seat;
                self.particles.push(p);
            }
            if pose == Pose::Type && a.zacc > 1300 + a.phase % 700 {
                a.zacc = 0;
                let x = (g.sx as i64 + 33 + (t / 7) % 12) as f64;
                let mut p = Particle::new(PKind::Key, x, (g.sy + LID_Y - 4) as f64, 1100);
                p.glyph = ((t / 13) % 5) as usize;
                p.color = a::LAPTOP[a.codex as usize][3];
                self.particles.push(p);
            }
        }
        let nw = self.waiting_list().len();
        if self.focus >= nw {
            self.focus = 0;
        }
    }

    /* ---- render ---------------------------------------------------------- */

    fn draw_stars(&self, g: &mut Gfx) {
        const STARS: [(i32, i32); 7] = [(14, 9), (22, 14), (31, 8), (40, 12), (17, 18), (46, 7), (27, 19)];
        for (n, &(x, y)) in STARS.iter().enumerate() {
            let v = (self.t as f64 / 700.0 + (n * 3) as f64) as i32;
            g.px(x, y, if v % 5 == 0 { c::STAR_DIM } else { c::STAR });
        }
    }

    fn draw_sign(&self, g: &mut Gfx) {
        const SIGN: &str = "TOKEN CAFE";
        let k = 3;
        let x0 = (a::W as i32 - text_width(SIGN, k)) >> 1;
        let y0 = 3;
        // one letter flickers now and then
        let cycle = self.t % 9000;
        let dead = if cycle > 7200 && cycle < 7700 && (cycle / 90) % 2 == 0 { 7 } else { usize::MAX };
        for pass in 0..2 {
            for (n, ch) in SIGN.bytes().enumerate() {
                let gl = &a::FONT[(ch - 32) as usize];
                let off = n == dead;
                for (j, bits) in gl.iter().enumerate() {
                    for i in 0..5 {
                        if (bits >> (4 - i)) & 1 == 0 {
                            continue;
                        }
                        let (x, y) = (x0 + (n as i32 * 6 + i) * k, y0 + j as i32 * k);
                        if pass == 0 {
                            if !off {
                                g.rect(x - 1, y - 1, k + 2, k + 2, c::NEON_GLOW);
                            }
                        } else {
                            let col = if off {
                                c::NEON_OFF
                            } else if (i + j as i32) % 4 == 0 {
                                c::NEON_CORE
                            } else {
                                c::NEON
                            };
                            g.rect(x, y, k, k, col);
                        }
                    }
                }
            }
        }
        // A tiny enamel sign, with a steady light and crisp lettering.
        let sx = (a::W as i32 - 68) >> 1;
        g.rect(sx, 25, 68, 10, c::CYAN_GLOW);
        g.rect(sx + 1, 26, 66, 8, base::K);
        g.rect(sx + 4, 29, 2, 2, c::CYAN);
        g.text("OPEN", sx + 9, 26, c::CYAN, 1, None);
        g.rect(sx + 36, 28, 1, 4, c::CYAN_GLOW);
        g.text("24H", sx + 42, 26, c::CHALK_HI, 1, None);
    }

    /// The chalkboard and its labels are in the background; only the values move.
    fn draw_tab(&self, g: &mut Gfx) {
        let (tok, usd) = self.usage_text.clone().unwrap_or_else(|| match self.usage {
            Some(u) => (fmt_tokens(u.tokens), fmt_usd(u.usd)),
            None => ("--".into(), "--".into()),
        });
        g.text(&tok, TAB_X + TAB_W - 3 - text_width(&tok, 1), TAB_Y + 5, c::CHALK_HI, 1, None);
        g.text(&usd, TAB_X + TAB_W - 3 - text_width(&usd, 1), TAB_Y + 14, c::CHALK_GOLD, 1, None);
        if !self.usage_rows.is_empty() {
            g.text("+", TAB_X - 8, TAB_Y + 8, c::CHALK_HI, 1, None);
        }
    }

    fn draw_usage(&self, fb: &mut [u32]) {
        fb.fill(0x101423);
        let mut g = Gfx::new(fb);
        g.rect(8, 8, 464, 256, 0x303b5c);
        g.rect(10, 10, 460, 252, 0x171e31);
        g.text("THIS MONTH / USAGE", 24, 24, c::GOLD, 2, None);
        g.text("X", 444, 24, c::TEXT, 2, None);
        g.text("APP TOTALS", 24, 47, if self.usage_accounts { c::TEXT_DIM } else { c::GOLD }, 1, None);
        g.text("RECORDED ORG", 164, 47, if self.usage_accounts { c::GOLD } else { c::TEXT_DIM }, 1, None);
        g.text("APP / ACCOUNT GROUP", 24, 65, c::TEXT_DIM, 1, None);
        g.text("TOKENS", 304, 65, c::TEXT_DIM, 1, None);
        g.text("API EST.", 402, 65, c::TEXT_DIM, 1, None);
        g.rect(24, 75, 432, 1, 0x52617f);
        let rows = if self.usage_accounts { &self.usage_account_rows } else { &self.usage_rows };
        for (index, row) in rows.iter().skip(self.usage_page * 4).take(4).enumerate() {
            let y = 87 + index as i32 * 23;
            g.text(&row.label, 24, y, c::TEXT, 1, None);
            let tokens = fmt_tokens(row.tokens);
            let usd = fmt_usd(row.usd);
            g.text(&tokens, 340 - text_width(&tokens, 1), y, c::CHALK_HI, 1, None);
            g.text(&usd, 450 - text_width(&usd, 1), y, c::CHALK_GOLD, 1, None);
        }
        g.rect(24, 174, 432, 1, 0x52617f);
        g.text("ALL PROVIDERS", 24, 188, c::GOLD, 1, None);
        if let Some(usage) = self.usage {
            let tokens = fmt_tokens(usage.tokens);
            let usd = fmt_usd(usage.usd);
            g.text(&tokens, 340 - text_width(&tokens, 1), 188, c::GOLD, 1, None);
            g.text(&usd, 450 - text_width(&usd, 1), 188, c::GOLD, 1, None);
        }
        g.text("SAVED APP / RECORDED SESSION OWNER; NOT A BILL", 24, 212, c::TEXT_DIM, 1, None);
        g.text("A: SWITCH VIEW", 24, 230, c::TEXT, 1, None);
        let page = format!("< {}/{} >", self.usage_page + 1, self.usage_page_count());
        g.text(&page, 450 - text_width(&page, 1), 230, c::TEXT, 1, None);
        g.text("U / ESC TO CLOSE", 24, 246, c::TEXT, 1, None);
    }

    fn draw_clock(&self, g: &mut Gfx, cx: i32, cy: i32, now: NaiveTime) {
        for j in -6..=6 {
            for i in -6..=6 {
                let d = i * i + j * j;
                if d <= 36 {
                    g.px(cx + i, cy + j, if d > 25 { c::CLOCK_RIM } else { c::CLOCK_FACE });
                }
            }
        }
        g.px(cx, cy - 4, base::K);
        g.px(cx + 4, cy, base::K);
        g.px(cx, cy + 4, base::K);
        g.px(cx - 4, cy, base::K);
        let (h, m) = (now.hour(), now.minute());
        let hm = ((h % 12) as f64 + m as f64 / 60.0) / 12.0 * PI * 2.0;
        let mm = m as f64 / 60.0 * PI * 2.0;
        for r in 0..=4 {
            g.px(cx + jround(mm.sin() * r as f64), cy - jround(mm.cos() * r as f64), base::K);
        }
        for r in 0..=2 {
            g.px(cx + jround(hm.sin() * r as f64), cy - jround(hm.cos() * r as f64), base::X);
        }
    }

    fn draw_chair(&self, g: &mut Gfx, i: usize) {
        let geo = seat_geom(i);
        let mut p = Pal::base();
        let ch = a::CHAIRS[i % a::CHAIRS.len()];
        p.set(slot::A, ch[0]);
        p.set(slot::a, ch[1]);
        g.blit_large(&a::CHAIR, geo.sx + CHAIR_X - 5, geo.sy + CHAIR_Y, Some(&p), false);
    }

    fn draw_closed_laptop(&self, g: &mut Gfx, ag: &Agent, x: i32, y: i32) {
        let l = a::LAPTOP[ag.codex as usize];
        g.rect(x, y, 14, 4, base::K);
        g.rect(x + 1, y + 1, 12, 1, l[0]);
        g.rect(x + 1, y + 2, 12, 1, l[1]);
    }

    /// Claude's lid shows its asterisk spinner, Codex's a clockwise highlight around the ChatGPT knot.
    fn draw_laptop(&self, g: &mut Gfx, ag: &Agent, x: i32, y: i32) {
        let t = self.t;
        let l = a::LAPTOP[ag.codex as usize];
        let mut p = Pal::base();
        p.set(slot::L, l[0]);
        p.set(slot::l, l[1]);
        p.set(slot::A, l[2]);
        p.set(slot::a, l[3]);
        g.blit(&a::LID, x, y, Some(&p), false);
        g.rect(x - 1, y + 11, 15, 2, base::K);
        g.rect(x, y + 11, 13, 1, l[1]);
        let logo = if ag.codex {
            if ag.state == State::Work { &a::CHATGPT_WORK[((t / 260) % 6) as usize] } else { &a::CHATGPT }
        } else if ag.state == State::Work {
            const SEQ: [usize; 6] = [0, 1, 2, 3, 2, 1];
            &a::SPARK[SEQ[((t / 130) % 6) as usize]]
        } else {
            &a::SPARK[3]
        };
        if ag.state == State::Wait && (t / 300) % 2 != 0 {
            p.set(slot::A, base::Y);
            p.set(slot::a, base::W);
        }
        g.blit(logo, x + if ag.codex { 1 } else { 3 }, y + if ag.codex { 1 } else { 2 }, Some(&p), false);
    }

    fn draw_plate(&self, g: &mut Gfx, ag: Option<&Agent>, cx: i32, y: i32) {
        let Some(ag) = ag else {
            // an empty seat keeps a blank brass plate
            g.rect(cx - 12, y + 2, 24, 7, base::K);
            g.rect(cx - 11, y + 3, 22, 5, c::DESK_FRONT_LO);
            return;
        };
        let name = shorten(&ag.name, PLATE_LEN).to_ascii_uppercase();
        let w = text_width(&name, 1) + 6;
        let x = cx - (w >> 1);
        let waiting = self.attention.contains_key(&ag.id);
        g.rect(x, y, w, 11, base::K);
        let face = if waiting {
            base::Y
        } else if ag.codex {
            c::PLATE_CODEX
        } else {
            c::PLATE_CLAUDE
        };
        g.rect(x + 1, y + 1, w - 2, 9, face);
        g.rect(x + 1, y + 9, w - 2, 1, c::PLATE_LO);
        g.text(&name, x + 3, y + 2, if ag.state == State::Idle { a::IDLE_TEXT } else { base::K }, 1, None);
    }

    fn draw_desk(&self, g: &mut Gfx, i: usize) {
        let geo = seat_geom(i);
        let (x, y, w) = (geo.sx + DESK_X, geo.sy + DESK_Y, DESK_W);
        g.darken(x + 2, y + 21, w - 2, 3, 0.55);
        g.rect(x, y, w, 9, base::K);
        g.rect(x + 1, y + 1, w - 2, 7, c::DESK_TOP);
        g.rect(x + 1, y + 1, w - 2, 1, c::DESK_HI);
        for n in 0..3 {
            g.rect(x + 7 + n * 16, y + 3 + (n & 1) * 2, 8, 1, c::DESK_GRAIN);
        }
        g.rect(x + 1, y + 7, w - 2, 1, c::DESK_EDGE);
        g.rect(x, y + 8, w, 13, base::K);
        g.rect(x + 1, y + 8, w - 2, 12, c::DESK_FRONT);
        g.rect(x + 1, y + 18, w - 2, 2, c::DESK_FRONT_LO);
        for &(s, px, py) in seat_props(i) {
            g.blit(s, geo.sx + px, geo.sy + py + 12, None, false);
        }
        let ag = self.agents.iter().find(|a| a.seat == i);
        if let Some(ag) = ag.filter(|a| a.mode == Mode::Seated) {
            #[cfg(feature = "std")]
            if self.patrons.is_creature(i) {
                // Desktop contact stays behind the laptop lid and its logo.
                g.clip(geo.sx + 29, geo.sy + DESK_Y, geo.sx + 55, geo.sy + DESK_Y + 2);
                self.draw_creature(g, ag);
                g.clip(0, 0, a::W as i32, a::WIN_Y);
            }
            if pose_of(self.t, ag) == Pose::Sleep {
                self.draw_closed_laptop(g, ag, geo.sx + LID_X - 1, geo.sy + DESK_Y + 2);
            } else {
                self.draw_laptop(g, ag, geo.sx + LID_X, geo.sy + LID_Y);
            }
            #[cfg(feature = "std")]
            if self.patrons.is_creature(i) {
                if pose_of(self.t, ag) == Pose::Sip {
                    self.patrons.cup(g, i, (geo.sx + 40, geo.sy + DESK_Y + 1), (self.t + ag.phase) as u64);
                }
                if pose_of(self.t, ag) == Pose::Type {
                    self.patrons.keyboard_paws(g, i, (geo.sx + 40, geo.sy + DESK_Y + 1), (self.t + ag.phase) as u64);
                }
            }
        }
        self.draw_plate(g, ag, geo.sx + 40, y + 9);
    }

    #[cfg(feature = "std")]
    fn draw_creature(&self, g: &mut Gfx, ag: &Agent) -> bool {
        let geo = seat_geom(ag.seat);
        let animation = match pose_of(self.t, ag) {
            Pose::Type => Animation::Type,
            Pose::Think => Animation::Think,
            Pose::Raise => Animation::Wave,
            Pose::Sip => Animation::Sip,
            Pose::Sleep => Animation::Sleep,
            Pose::Stretch => Animation::Stretch,
        };
        self.patrons.draw(g, ag.seat, (geo.sx + 40, geo.sy + DESK_Y + 1), animation, (self.t + ag.phase) as u64)
    }

    fn draw_seated(&self, g: &mut Gfx, ag: &Agent) {
        #[cfg(feature = "std")]
        if self.draw_creature(g, ag) {
            return;
        }
        let geo = seat_geom(ag.seat);
        let p = &a::POSES[ag.style];
        let t = self.t + ag.phase;
        let spr = match pose_of(self.t, ag) {
            Pose::Type => &p.typing[((t / 170) % 2) as usize],
            Pose::Think => &p.think,
            Pose::Raise => &p.raise[((t / 320) % 2) as usize],
            Pose::Stretch => &p.stretch,
            Pose::Sip => &p.sip[((t / 1600) % 2) as usize],
            Pose::Sleep => &p.sleep[((t / 1100) % 2) as usize],
        };
        g.blit_large(spr, geo.sx + SIT_X - BX - 6, geo.sy, Some(&ag.pal), false);
    }

    fn draw_walker(&self, g: &mut Gfx, ag: &Agent) {
        #[cfg(feature = "std")]
        {
            let animation = match ag.dir {
                Dir::Up => Animation::WalkUp,
                Dir::Down => Animation::WalkDown,
                Dir::Left => Animation::WalkLeft,
                Dir::Right => Animation::WalkRight,
            };
            if self.patrons.draw(g, ag.seat, (jround(ag.x), jround(ag.y)), animation, ag.dist as u64) {
                return;
            }
        }
        let p = &a::POSES[ag.style];
        let f = (ag.dist / 7.0) as i32 % 4;
        let mut flip = false;
        let spr = match ag.dir {
            Dir::Up => {
                if f % 2 == 0 {
                    &p.stand_u
                } else {
                    &p.walk_u[if f == 1 { 0 } else { 1 }]
                }
            }
            Dir::Down => {
                if f % 2 == 0 {
                    &p.stand_d
                } else {
                    &p.walk_d[if f == 1 { 0 } else { 1 }]
                }
            }
            Dir::Left | Dir::Right => {
                flip = ag.dir == Dir::Left;
                if f % 2 == 0 { &p.stand_s } else { &p.walk_s[0] }
            }
        };
        let (x, y) = (jround(ag.x) - 13, jround(ag.y) - 39);
        g.blit_large(spr, x - BX, y - BY + if f % 2 != 0 { -1 } else { 0 }, Some(&ag.pal), flip);
    }

    fn draw_overlays(&self, g: &mut Gfx) {
        #[cfg(feature = "std")]
        if let Some(seat) = self.selected_seat.filter(|&s| s < SEATS) {
            let geo = seat_geom(seat);
            g.rect(geo.sx + DESK_X, geo.sy + DESK_Y + 20, DESK_W, 1, c::GOLD);
        }
        let t = self.t;
        let focused = self
            .selected
            .as_deref()
            .or_else(|| self.attention_id())
            .and_then(|id| self.agents.iter().position(|ag| ag.id == id && ag.seen));
        for (k, ag) in self.agents.iter().enumerate() {
            if ag.mode != Mode::Seated {
                continue;
            }
            let geo = seat_geom(ag.seat);
            if self.attention.contains_key(&ag.id) {
                let (x, y, w, h) = (geo.sx - 6, geo.sy + 5, 92, 57);
                if focused == Some(k) {
                    g.rect(x, y, w + 1, 2, c::GOLD);
                    g.rect(x, y + h - 1, w + 1, 2, c::GOLD);
                    g.rect(x, y, 2, h + 1, c::GOLD);
                    g.rect(x + w - 1, y, 2, h + 1, c::GOLD);
                } else {
                    for (cx, cy, dx, dy) in [(x, y, 1, 1), (x + w, y, -1, 1), (x, y + h, 1, -1), (x + w, y + h, -1, -1)]
                    {
                        g.rect(cx - if dx < 0 { 6 } else { 0 }, cy, 7, 1, c::CHALK_DIM);
                        g.rect(cx, cy - if dy < 0 { 6 } else { 0 }, 1, 7, c::CHALK_DIM);
                    }
                }
            }
            match pose_of(t, ag) {
                Pose::Raise => {
                    let b = if ag.why == Why::Ask { &a::BUBBLE_ASK } else { &a::BUBBLE_BANG };
                    g.blit(b, geo.sx + 29, geo.sy + SIT_Y - 14, None, false);
                }
                Pose::Think => g.blit(&a::BUBBLE_THINK, geo.sx + 45, geo.sy + SIT_Y - 11, None, false),
                _ => {}
            }
            if focused == Some(k) && ag.state != State::Wait {
                g.blit(&a::CURSOR, geo.sx + 7, geo.sy + SIT_Y + 6, None, false);
            }
        }
        for p in &self.particles {
            let k = p.age as f64 / p.life as f64;
            match p.kind {
                PKind::Z => {
                    let x = jround(p.x + k * 10.0 + (k * 6.0).sin() * 2.0);
                    let y = jround(p.y - k * 20.0);
                    g.blit(if k < 0.45 { &a::ZSMALL } else { &a::ZBIG }, x, y, None, false);
                }
                PKind::Steam => {
                    let x = jround(p.x + (k * 7.0 + p.seat as f64).sin() * 1.5);
                    let y = jround(p.y - k * 9.0);
                    g.px(x, y, if k < 0.5 { base::W } else { base::w });
                }
                PKind::Key => {
                    let mut pal = Pal::base();
                    pal.set(slot::A, p.color);
                    g.blit(&a::KEYS[p.glyph], jround(p.x), jround(p.y - k * 10.0), Some(&pal), false);
                }
            }
        }
    }

    fn patron_look(&self, id: &str) -> (Pal, usize) {
        if let Some(agent) = self.agents.iter().find(|ag| ag.id == id && ag.seen) {
            (agent.pal, agent.style)
        } else {
            let look = self.appearance_seeds.get(id).map_or_else(|| look_for(id), |seed| look_seed(*seed));
            (look.pal, look.style)
        }
    }

    fn draw_patron_portrait(&self, g: &mut Gfx, id: &str, x: i32, y: i32) {
        #[cfg(feature = "std")]
        if let Some(ag) = self.agents.iter().find(|ag| ag.id == id && ag.seen && self.patrons.is_creature(ag.seat)) {
            g.rect(x, y, 34, 36, c::GOLD);
            g.rect(x + 1, y + 1, 32, 34, c::WIN1);
            self.patrons.portrait(g, ag.seat, (x + 17, y + 34), (self.t + ag.phase) as u64);
            return;
        }
        let (pal, style) = self.patron_look(id);
        crate::review::draw_patron_portrait(g, &pal, style, x, y);
    }

    /// Shared portrait and text layout for automatic alerts and manual focus.
    fn draw_dialogue(&self, g: &mut Gfx, id: &str, _state: State, title: &str, text: &str) {
        self.draw_patron_portrait(g, id, 8, a::WIN_Y + 13);
        g.text(&ellipsize(title, 66), 50, a::WIN_Y + 6, c::GOLD, 1, Some(c::TEXT_SHADOW));
        for (row, line) in text.lines().take(2).enumerate() {
            g.text(line, 50, a::WIN_Y + 19 + row as i32 * 19, c::TEXT, 2, Some(c::TEXT_SHADOW));
        }
    }

    /// The message window: whoever waits for the human (portrait, name and
    /// what they want), else the party status. Focus never rotates on a timer.
    fn draw_message(&self, g: &mut Gfx, now: NaiveTime) {
        let t = self.t;
        let (x, y, w, h) = (0, a::WIN_Y, a::W as i32, a::H as i32 - a::WIN_Y);
        // Pulse the full message surface: a thin edge is easy to miss on the LCD.
        // Paint before text and controls so their foreground never flashes off.
        if self.attention_bright() {
            g.rect(x + 3, y + 3, w - 6, h - 6, 0x70451c);
        }
        if let Some(id) = self.attention_id() {
            let session = self.sessions.iter().find(|s| s.id == id);
            let request = self.requests.iter().position(|r| r.session == id);
            let name = session.map_or(id, |s| s.name.as_str());
            let permission =
                request.is_some_and(|i| self.requests[i].choices == 0) || session.is_some_and(|s| s.why == Why::Perm);
            let status = if permission { "! APPROVAL NEEDED" } else { "? ANSWER NEEDED" };
            let text = request
                .map(|i| self.requests[i].text.as_str())
                .or_else(|| {
                    (!permission).then(|| self.details.iter().find(|d| d.id == id).map(|d| d.task.as_str())).flatten()
                })
                .filter(|text| !text.is_empty())
                .unwrap_or(if permission {
                    "OPEN REQUEST FOR PERMISSION DETAILS"
                } else {
                    "OPEN REQUEST TO READ THE QUESTION"
                });
            // The main question gets a 2x font; status and controls remain compact.
            self.draw_patron_portrait(g, id, 8, y + 15);
            g.text(&ellipsize(name, 55), 50, y + 5, c::GOLD, 1, None);
            g.text(status, 50, y + 13, c::GOLD, 1, None);
            let preview = crate::text::layout(text, 60);
            if let Some(line) = preview.first() {
                let line = ellipsize(&line.text, if self.next_attention_id().is_some() { 31 } else { 35 });
                g.text(&line, 50, y + 22, c::TEXT, 2, Some(c::TEXT_SHADOW));
            }
            let viewed = self.attention.get(id).is_some_and(Option::is_none);
            crate::review::draw_button(g, 50, y + 38, 140, 20, "VIEW REQUEST >");
            g.text(
                if viewed { "SEEN - STILL PENDING" } else { "TAP MESSAGE TO ACKNOWLEDGE" },
                200,
                y + 45,
                c::GOLD,
                1,
                None,
            );
            if self.next_attention_id().is_some() {
                g.rect(435, y + 4, 1, h - 8, c::WIN_BORDER_LO);
                let count = format!(
                    "{}/{}",
                    self.attention.keys().position(|key| key == id).unwrap_or(0) + 1,
                    self.attention.len()
                );
                g.text(&count, 440, y + 9, c::GOLD, 1, None);
                crate::review::draw_button(g, 439, y + 38, 35, 20, "NEXT");
            }
            g.rect(2, y + 1, w - 4, 2, if self.attention_bright() { base::W } else { c::GOLD });
            return;
        }
        let resolved =
            self.selected.as_deref().and_then(|id| self.resolved_attention.get(id).map(|r| (id, r))).or_else(|| {
                if self.selected.is_none() && self.attention.is_empty() {
                    self.resolved_attention.iter().max_by_key(|(_, (at, _))| at).map(|(id, r)| (id.as_str(), r))
                } else {
                    None
                }
            });
        if let Some((id, (_, _))) = resolved.filter(|(_, (at, _))| self.t - at < 4000) {
            let name = self.sessions.iter().find(|s| s.id == id).map_or(id, |s| s.name.as_str());
            self.draw_dialogue(g, id, State::Work, &format!("{name} - REQUEST RESOLVED"), "NO LONGER WAITING");
            return;
        }
        if let Some(session) = self.sessions.iter().find(|s| Some(&s.id) == self.selected.as_ref()) {
            let pages = self.selected_task_pages();
            let page = self.task_page % pages.len();
            let previous_request = self.resolved_attention.get(&session.id).is_some_and(|(_, old)| {
                old.as_ref().is_some_and(|text| self.details.iter().any(|d| d.id == session.id && &d.task == text))
            });
            let prefix = if previous_request {
                "LAST REQUEST"
            } else if session.state == State::Idle {
                "LAST"
            } else {
                "TASK"
            };
            let outside = !self.agents.iter().any(|ag| ag.id == session.id && ag.seen);
            let location = if outside { "OUTSIDE " } else { "" };
            let name = if self.attention.is_empty() {
                format!("{location}{prefix} {}", session.name)
            } else {
                format!("!{} WAIT {location}{prefix} {}", self.attention.len(), session.name)
            };
            self.draw_dialogue(g, &session.id, session.state, &name, &pages[page]);
            // Vertical scrollbar; the surrounding 44x60 area is its hit target.
            let track_y = y + 6;
            let track_h = h - 12;
            let thumb_h = (track_h / pages.len() as i32).max(8);
            let travel = track_h - thumb_h;
            let offset = if pages.len() > 1 { travel * page as i32 / (pages.len() - 1) as i32 } else { 0 };
            let sx = x + w - 14;
            // Recessed blue rail tucked inside the panel's right border.
            g.rect(sx - 1, track_y - 1, 10, track_h + 2, 0x10205b);
            g.rect(sx, track_y, 8, track_h, 0x1a3280);
            g.rect(sx + 8, track_y, 1, track_h, 0x687fc1);
            g.rect(sx, track_y + track_h, 9, 1, 0x687fc1);
            // Raised thumb with a bright upper edge and paired grip marks.
            let ty = track_y + offset;
            g.rect(sx, ty, 8, thumb_h, 0x536cab);
            g.rect(sx, ty, 8, 1, 0xe0e8ff);
            g.rect(sx, ty + 1, 1, thumb_h - 2, 0xc0cff5);
            g.rect(sx + 1, ty + 1, 6, thumb_h - 3, 0xa1b6e8);
            g.rect(sx + 2, ty + thumb_h / 2 - 1, 4, 1, 0x536cab);
            g.rect(sx + 2, ty + thumb_h / 2 + 1, 4, 1, 0x536cab);
            return;
        }
        // With no selection, show the stable cafe summary.
        let (mut working, mut idle) = (0, 0);
        for ag in self.agents.iter().filter(|a| a.mode == Mode::Seated) {
            working += (ag.state == State::Work) as usize;
            idle += (ag.state == State::Idle) as usize;
        }
        let mut cx = x + 12;
        let mut lp = Pal::base();
        lp.set(slot::A, a::ICON_LAPTOP_A);
        lp.set(slot::a, a::ICON_LAPTOP_a);
        for (icon, pal, count) in [(&a::ICON_LAPTOP, Some(&lp), working), (&a::ICON_CUP, None, idle)] {
            g.blit_scaled(icon, cx, y + 9, pal, 2, None);
            cx += icon.w as i32 * 2 + 4;
            let s = count.to_string();
            g.text(&s, cx, y + 10, c::TEXT, 2, Some(c::TEXT_SHADOW));
            cx += text_width(&s, 2) + 14;
        }
        let colon = if (t / 500) % 2 != 0 { ':' } else { ' ' };
        let clock = format!("{:02}{colon}{:02}", now.hour(), now.minute());
        g.text(&clock, x + w - 12 - text_width(&clock, 2), y + 10, c::GOLD, 2, Some(c::TEXT_SHADOW));
        let line = if self.overflow > 0 {
            format!("+{} OUTSIDE. TAP A PATRON FOR DETAILS", self.overflow)
        } else if self.sessions.is_empty() {
            "THE CAFE IS EMPTY. NO AGENTS ONLINE.".into()
        } else {
            "TAP A PATRON FOR DETAILS".into()
        };
        g.text(&line, x + 12, y + 31, c::TEXT_DIM, 1, Some(c::TEXT_SHADOW));
    }

    /// Draw the café as of the last tick, with the clocks at local time `now`.
    pub fn render(&self, fb: &mut [u32], now: NaiveTime) {
        if self.usage_open && self.review.is_none() {
            self.draw_usage(fb);
            return;
        }
        // Review is opaque. Drawing the hidden room first wastes a full scene
        // render on the board and delays the next touch sample.
        if let Some(review) = &self.review {
            let id = &review.request().session;
            let name = self.sessions.iter().find(|s| &s.id == id).map_or(id.as_str(), |s| s.name.as_str());
            let (pal, style) = self.patron_look(id);
            let mut g = Gfx::new(fb);
            review.draw_for_patron(&mut g, name, &pal, style);
            self.draw_patron_portrait(&mut g, id, 104, 9);
            return;
        }
        paint_background(fb);
        let mut g = Gfx::new(fb);
        // the room's window is part of the background, so the scene stays above it
        g.clip(0, 0, a::W as i32, a::WIN_Y);
        self.draw_stars(&mut g);
        self.draw_sign(&mut g);
        self.draw_clock(&mut g, 86, 17, now);
        self.draw_tab(&mut g);
        // depth-sorted: chairs, desks and patrons by their feet
        let mut items: Vec<(f64, usize, Item)> = Vec::with_capacity(SEATS * 2 + self.agents.len());
        for i in 0..SEATS {
            let sy = seat_geom(i).sy as f64;
            items.push((sy + 27.0, 2 * i, Item::Chair(i)));
            items.push((sy + 51.0, 2 * i + 1, Item::Desk(i)));
        }
        for (k, ag) in self.agents.iter().enumerate() {
            let d = if ag.mode == Mode::Seated { (seat_geom(ag.seat).sy + 42) as f64 } else { ag.y };
            items.push((d, 100 + k, Item::Agent(k)));
        }
        items.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.cmp(&q.1)));
        for (_, _, item) in &items {
            match *item {
                Item::Chair(i) => self.draw_chair(&mut g, i),
                Item::Desk(i) => self.draw_desk(&mut g, i),
                Item::Agent(k) => {
                    let ag = &self.agents[k];
                    if ag.mode == Mode::Seated { self.draw_seated(&mut g, ag) } else { self.draw_walker(&mut g, ag) }
                }
            }
        }
        self.draw_overlays(&mut g);
        g.noclip();
        self.draw_message(&mut g, now);
        if !self.attention.is_empty() && self.attention_id().is_none() {
            g.rect(2, a::WIN_Y + 1, a::W as i32 - 4, 2, if self.attention_bright() { base::W } else { c::GOLD });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "std")]
    #[test]
    fn creature_walk_frames_follow_motion_for_every_seat() {
        for roster in ["dragons", "pokemon"] {
            for seat in 0..SEATS {
                for leaving in [false, true] {
                    let mut scene = Scene::new();
                    scene.patrons = crate::patrons::Patrons::roster(roster).unwrap();
                    scene.set_sessions(&[Session {
                        id: "walk".into(),
                        name: "walk".into(),
                        kind: Kind::Claude,
                        state: State::Work,
                        why: Why::None,
                        pose: ForcedPose::Auto,
                        age_s: 0,
                        branch: None,
                        active_subagents: None,
                    }]);
                    let mut path = path_to(seat);
                    if leaving {
                        path.reverse();
                    }
                    scene.agents[0].seat = seat;
                    scene.agents[0].start_walk(&path, if leaving { Mode::Leaving } else { Mode::Arriving });
                    let mut checked = 0u32;
                    for _ in 0..3000 {
                        let Some(ag) = scene.agents.first() else { break };
                        if ag.mode == Mode::Seated {
                            break;
                        }
                        let (x, y, segment) = (ag.x, ag.y, ag.path_i);
                        scene.tick(10);
                        let Some(ag) = scene.agents.first() else { break };
                        if ag.mode == Mode::Seated || ag.path_i != segment {
                            continue;
                        }
                        let (dx, dy) = (ag.x - x, ag.y - y);
                        if dx == 0.0 && dy == 0.0 {
                            continue;
                        }
                        let expected = if dx.abs() > dy.abs() {
                            if dx > 0.0 { Animation::WalkRight } else { Animation::WalkLeft }
                        } else if dy > 0.0 {
                            Animation::WalkDown
                        } else {
                            Animation::WalkUp
                        };
                        // Compare actual walker rendering against the clip for its movement.
                        // Once per path segment keeps this a compact real-path regression.
                        if checked & (1 << segment) == 0 {
                            let mut actual = vec![0; a::W * a::H * 4];
                            scene.draw_walker(&mut Gfx::new(&mut actual), ag);
                            let mut wanted = vec![0; a::W * a::H * 4];
                            scene.patrons.draw(
                                &mut Gfx::new(&mut wanted),
                                seat,
                                (jround(ag.x), jround(ag.y)),
                                expected,
                                ag.dist as u64,
                            );
                            assert_eq!(actual, wanted, "{roster} seat {seat}, leaving={leaving}, segment={segment}");
                            checked |= 1 << segment;
                        }
                    }
                    assert!(checked.count_ones() >= 3, "path was not exercised");
                }
            }
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn all_creatures_animate_inside_the_actual_seated_scene() {
        let mut scene = Scene::new();
        scene.set_sessions(&[Session {
            id: "pose-test".into(),
            name: "test".into(),
            kind: Kind::Claude,
            state: State::Work,
            why: Why::Perm,
            pose: ForcedPose::Auto,
            age_s: 0,
            branch: None,
            active_subagents: None,
        }]);
        scene.agents[0].phase = 0;
        for name in &crate::patrons::NAMES[1..] {
            scene.patrons.assign(&format!("1={name}")).unwrap();
            for (state, force, start, duration) in [
                (State::Work, ForcedPose::Auto, 3000, 180),
                (State::Work, ForcedPose::Think, 0, 850),
                (State::Wait, ForcedPose::Auto, 0, 280),
                (State::Idle, ForcedPose::Sip, 0, 1100),
                (State::Idle, ForcedPose::Sleep, 0, 1000),
                (State::Idle, ForcedPose::Stretch, 0, 650),
            ] {
                scene.agents[0].state = state;
                scene.agents[0].force = force;
                let mut rendered = Vec::new();
                for t in [start, start + duration] {
                    scene.t = t;
                    let mut fb = vec![0; a::W * a::H];
                    scene.render(&mut fb, NaiveTime::from_hms_opt(12, 0, 0).unwrap());
                    // Exclude status bubbles, laptop and particles: inspect the
                    // visible head/torso to catch a static sprite behind effects.
                    rendered.push(
                        (63..82).flat_map(|y| (45..76).map(move |x| y * a::W + x)).map(|i| fb[i]).collect::<Vec<_>>(),
                    );
                }
                let changed = rendered[0].iter().zip(&rendered[1]).filter(|(a, b)| a != b).count();
                assert!(changed >= 3, "{name} {force:?} {state:?}: visible body did not animate ({changed})");
            }
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn creature_configuration_survives_updates_arrivals_and_departures() {
        let mut scene = Scene::new();
        scene.patrons = crate::patrons::Patrons::roster("mixed").unwrap();
        let session = |id: &str, state| Session {
            id: id.into(),
            name: "demo".into(),
            kind: Kind::Claude,
            state,
            why: Why::Perm,
            pose: ForcedPose::Auto,
            age_s: 0,
            branch: None,
            active_subagents: None,
        };
        scene.set_sessions(&[session("one", State::Wait)]);
        let config = scene.patrons.clone();
        let mut frame = vec![0; a::W * a::H];
        let now = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
        scene.render(&mut frame, now);
        // A creature portrait must differ from the legacy human portrait.
        let creature = frame.clone();
        scene.patrons = crate::patrons::Patrons::default();
        scene.render(&mut frame, now);
        assert_ne!(&frame[199 * a::W..], &creature[199 * a::W..]);
        scene.patrons = config.clone();
        scene.set_sessions(&[session("one", State::Idle), session("two", State::Work)]);
        assert!(scene.agents.iter().any(|a| a.mode == Mode::Arriving));
        for _ in 0..200 {
            scene.tick(33);
            scene.render(&mut frame, now);
        }
        scene.set_sessions(&[session("two", State::Wait)]);
        assert!(scene.agents.iter().any(|a| a.mode == Mode::Leaving));
        for _ in 0..200 {
            scene.tick(33);
            scene.render(&mut frame, now);
        }
        assert_eq!(scene.patrons, config);
        assert_eq!(scene.agents.len(), 1);
        assert_eq!(scene.agents[0].state, State::Wait);
    }

    #[cfg(feature = "std")]
    #[test]
    fn creature_contact_does_not_overpaint_laptop_logo() {
        let mut scene = Scene::new();
        scene.set_sessions(&[Session {
            id: "logo-test".into(),
            name: "test".into(),
            kind: Kind::Claude,
            state: State::Wait,
            why: Why::Perm,
            pose: ForcedPose::Auto,
            age_s: 0,
            branch: None,
            active_subagents: None,
        }]);
        let geo = seat_geom(0);
        for name in &crate::patrons::NAMES[1..] {
            scene.patrons.assign(&format!("1={name}")).unwrap();
            for k in [1, 2] {
                for t in [0, 280, 650, 1100] {
                    scene.t = t;
                    let mut expected = vec![0; a::W * a::H * k * k];
                    scene.draw_laptop(&mut Gfx::new(&mut expected), &scene.agents[0], geo.sx + LID_X, geo.sy + LID_Y);
                    let mut actual = vec![0; a::W * a::H * k * k];
                    scene.draw_desk(&mut Gfx::new(&mut actual), 0);
                    for y in (geo.sy + LID_Y)..(geo.sy + LID_Y + 11) {
                        for x in (geo.sx + LID_X)..(geo.sx + LID_X + 13) {
                            for dy in 0..k {
                                for dx in 0..k {
                                    let i = (y as usize * k + dy) * a::W * k + x as usize * k + dx;
                                    assert_eq!(actual[i], expected[i], "{name} at {t}, scale {k}, pixel {x},{y}");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn single_cli_activation_selects_exact_terminal_and_keeps_other_navigation_unchanged() {
        let mut scene = Scene::new();
        scene.set_sessions(&[
            session("one", State::Wait),
            session("two", State::Work),
            session("desktop", State::Idle),
            session("unmapped", State::Idle),
        ]);
        scene.set_details(vec![
            SessionDetail {
                id: "one".into(),
                task: "same project".into(),
                target: Some(SessionTarget::ITerm("pane-one".into())),
            },
            SessionDetail {
                id: "two".into(),
                task: "same project".into(),
                target: Some(SessionTarget::ITerm("pane-two".into())),
            },
            SessionDetail {
                id: "desktop".into(),
                task: "task".into(),
                target: Some(SessionTarget::Claude("local_desktop".into())),
            },
            SessionDetail { id: "unmapped".into(), task: "task".into(), target: None },
        ]);
        let point = |s: &Scene, id: &str| {
            let seat = s.agents.iter().find(|a| a.id == id).unwrap().seat;
            ((seat % 4) as i32 * 120 + 60, (seat / 4) as i32 * 82 + 80)
        };
        for (id, pane) in [("one", "pane-one"), ("two", "pane-two")] {
            let (x, y) = point(&scene, id);
            assert_eq!(
                scene.activate_terminal_patron(x, y).as_deref(),
                Some(format!("iterm2:reveal?sessionid={pane}").as_str())
            );
            assert_eq!(scene.selected_id(), Some(id));
            assert!(scene.activate_terminal_patron(x, y).is_some());
            assert_eq!(scene.selected_id(), Some(id));
        }
        let (x, y) = point(&scene, "desktop");
        assert!(scene.activate_terminal_patron(x, y).is_none());
        assert_eq!(scene.activate_patron(x, y).as_deref(), Some("claude://code/continue?session=local_desktop"));
        let (x, y) = point(&scene, "unmapped");
        assert!(scene.activate_terminal_patron(x, y).is_none());
        assert!(scene.activate_patron(x, y).is_none());
        assert!(scene.activate_terminal_patron(30, 250).is_none());
    }

    #[test]
    fn usage_window_pages_all_accounts_and_blocks_underlying_patron_actions() {
        let mut scene = Scene::new();
        scene.set_sessions(&[session("live", State::Wait)]);
        scene.set_usage(Usage { tokens: 150., usd: 0.1 });
        let rows: Vec<_> =
            (0..6).map(|n| UsageRow { label: format!("CLI ORG {n}"), tokens: n as f64 * 10., usd: 0. }).collect();
        scene.set_usage_rows(rows[..4].to_vec());
        scene.set_usage_accounts(rows.clone());
        scene.click(TAB_X + 5, TAB_Y + 5);
        assert!(scene.usage_is_open());
        assert!(scene.patron_at(40, 70).is_none());
        scene.click(40, 70);
        assert!(scene.selected_id().is_none());
        scene.toggle_usage_accounts();
        scene.page_task_key(true);
        assert_eq!(scene.usage_page, 1);
        let mut frame = vec![0; a::W * a::H];
        scene.render(&mut frame, NaiveTime::from_hms_opt(12, 0, 0).unwrap());
        assert!(frame.contains(&c::CHALK_HI));
        scene.set_usage_accounts(rows[..1].to_vec());
        assert_eq!(scene.usage_page, 0);
        scene.clear_selection();
        assert!(!scene.usage_is_open());
        assert_eq!(scene.usage_account_rows.len(), 1);
    }
    #[test]
    fn review_and_footer_keep_the_authoritative_patron_portrait() {
        let now = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
        for style in 0..3 {
            let mut scene = Scene::new();
            scene.set_sessions(&[session("a", State::Wait)]);
            scene.set_appearance("a", (style << 20) | 1234);
            let mut cafe = vec![0; a::W * a::H];
            scene.render(&mut cafe, now);
            scene.review = Some(crate::review::Review::new(
                crate::approval::Request {
                    choices: 0,
                    id: "r".into(),
                    session: "a".into(),
                    title: "Permission".into(),
                    tool_name: "Test".into(),
                    text: "Synthetic request".into(),
                },
                true,
            ));
            let mut review = vec![0; a::W * a::H];
            scene.render(&mut review, now);
            for y in 0..36 {
                assert_eq!(
                    &cafe[(a::WIN_Y as usize + 15 + y) * a::W + 8..(a::WIN_Y as usize + 15 + y) * a::W + 42],
                    &review[(9 + y) * a::W + 104..(9 + y) * a::W + 138]
                );
            }
        }
    }

    #[test]
    fn one_waiter_has_no_next_and_resolution_preserves_history() {
        let mut scene = Scene::new();
        scene.set_sessions(&[session("a", State::Wait)]);
        scene.set_details(vec![SessionDetail { id: "a".into(), task: "May I run tests?".into(), target: None }]);
        assert!(scene.next_attention_id().is_none());
        scene.click(450, a::WIN_Y + 30);
        assert!(scene.selected_id().is_none());
        assert!(scene.attention_bright(), "disabled Next must not acknowledge a request");
        scene.focus_patron("a".into());
        scene.set_sessions(&[session("a", State::Work)]);
        assert!(scene.attention_id().is_none());
        assert_eq!(scene.resolved_attention["a"].1.as_deref(), Some("May I run tests?"));
        let timestamp = scene.resolved_attention["a"].0;
        scene.tick(4100);
        scene.set_sessions(&[session("a", State::Work)]);
        assert_eq!(scene.resolved_attention["a"].0, timestamp, "refresh cannot replay the resolved notification");
        scene.set_sessions(&[session("a", State::Wait)]);
        assert!(!scene.resolved_attention.contains_key("a"));
    }

    #[test]
    fn attention_pulses_a_visible_surface_while_reading_another_patron() {
        let mut scene = Scene::new();
        scene.set_sessions(&[session("waiting", State::Wait), session("reading", State::Work)]);
        scene.focus_patron("reading".into());
        let now = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
        let mut bright = vec![0; a::W * a::H];
        let mut dim = bright.clone();
        scene.render(&mut bright, now);
        scene.tick(600);
        scene.render(&mut dim, now);
        let footer = a::WIN_Y as usize * a::W;
        let changed = bright[footer..].iter().zip(&dim[footer..]).filter(|(a, b)| a != b).count();
        assert!(changed > 14_000, "attention must change most of the footer, got {changed}");
        assert_eq!(scene.selected_id(), Some("reading"));
        scene.tick(600);
        scene.acknowledge_attention("waiting");
        scene.render(&mut bright, now);
        assert_eq!(&bright[footer..], &dim[footer..], "acknowledgement restores the steady footer");
    }

    #[test]
    fn attention_pulses_once_and_acknowledgement_survives_refresh() {
        let mut scene = Scene::new();
        let waiting = session("a", State::Wait);
        scene.set_sessions(core::slice::from_ref(&waiting));
        assert!(scene.attention_bright());
        scene.tick(600);
        assert!(!scene.attention_bright());
        scene.set_sessions(core::slice::from_ref(&waiting));
        assert!(!scene.attention_bright(), "refresh cannot restart a pulse");
        scene.tick(600);
        assert!(scene.attention_bright());
        scene.click(100, a::WIN_Y + 25);
        assert!(!scene.attention_bright());
        assert!(scene.review.is_none());
        scene.set_sessions(core::slice::from_ref(&waiting));
        assert_eq!(scene.attention.get("a"), Some(&None));
        scene.set_sessions(&[session("a", State::Work)]);
        assert!(scene.attention.is_empty());
        scene.set_sessions(&[waiting]);
        assert!(scene.attention_bright());
        scene.tick(28_800);
        assert!(scene.attention_bright(), "pulse must continue near the end of the 30-second window");
        scene.tick(1_200);
        assert!(!scene.attention_bright(), "pulse must stop at 30 seconds");
        assert!(scene.attention.contains_key("a"), "steady marker survives pulse expiry");
    }

    #[test]
    fn attention_queue_includes_overflow_and_preserves_reading_selection() {
        let mut scene = Scene::new();
        let mut sessions: Vec<_> = (0..12).map(|i| session(&i.to_string(), State::Work)).collect();
        sessions[10].state = State::Wait;
        scene.set_sessions(&sessions);
        assert_eq!(scene.attention_id(), Some("10"));
        scene.focus_patron("0".into());
        sessions[11].state = State::Wait;
        scene.set_sessions(&sessions);
        assert_eq!(scene.selected_id(), Some("0"));
        scene.clear_selection();
        assert_eq!(scene.attention_id(), Some("10"));
        scene.click(450, a::WIN_Y + 30);
        assert_eq!(scene.attention_id(), Some("11"));
        assert!(scene.review.is_none());
        scene.click(100, a::WIN_Y + 48);
        let review = scene.review.as_ref().unwrap();
        assert_eq!(review.request().session, "11");
        assert!(review.decision.is_none());
    }

    #[test]
    fn replacement_request_realerts_without_moving_selection() {
        let mut scene = Scene::new();
        scene.set_sessions(&[session("a", State::Wait), session("b", State::Work)]);
        let request = crate::approval::Request {
            choices: 0,
            id: "one".into(),
            session: "a".into(),
            title: "Permission".into(),
            tool_name: "test".into(),
            text: "May I run tests?".into(),
        };
        scene.set_requests(vec![request.clone()]);
        scene.acknowledge_attention("a");
        scene.focus_patron("b".into());
        scene.set_requests(vec![request.clone()]);
        assert!(!scene.attention_bright());
        scene.set_requests(vec![crate::approval::Request { id: "two".into(), ..request }]);
        assert!(scene.attention_bright());
        assert_eq!(scene.selected_id(), Some("b"));
    }

    fn session(id: &str, state: State) -> Session {
        Session {
            id: id.into(),
            name: format!("Moon {id}"),
            kind: if id.parse::<usize>().is_ok_and(|i| i % 4 >= 2) { Kind::Codex } else { Kind::Claude },
            state,
            why: Why::None,
            pose: ForcedPose::Auto,
            age_s: 0,
            branch: None,
            active_subagents: None,
        }
    }

    #[test]
    fn codex_logo_animates_only_while_working() {
        let mut scene = Scene::new();
        scene.set_sessions(&[session("2", State::Work)]);
        let render = |scene: &Scene| {
            let mut frame = vec![0; a::W * a::H];
            scene.draw_laptop(&mut Gfx::new(&mut frame), &scene.agents[0], 10, 10);
            frame
        };
        scene.t = 0;
        let first = render(&scene);
        scene.t = 259;
        assert_eq!(first, render(&scene));
        scene.t = 260;
        assert_ne!(first, render(&scene));
        scene.t = 1560;
        assert_eq!(first, render(&scene));
        scene.agents[0].state = State::Idle;
        let idle = render(&scene);
        scene.t = 1820;
        assert_eq!(idle, render(&scene));
        scene.agents[0].state = State::Wait;
        scene.t = 0;
        let waiting = render(&scene);
        scene.t = 130;
        assert_eq!(waiting, render(&scene));
        scene.t = 300;
        assert_ne!(waiting, render(&scene));
    }

    #[test]
    fn new_focus_does_not_reuse_an_old_footer_identity() {
        let mut scene = Scene::new();
        scene.set_sessions(&[session("old", State::Work), session("new", State::Wait)]);
        scene.focus_patron("old".into());
        scene.click(200, 240);
        scene.focus_patron("new".into());
        scene.review_input(true, 200, 240, false);
        assert_eq!(scene.review.as_ref().unwrap().request().session, "new");
        scene.clear_selection();
        assert!(scene.footer_origin.is_none());
    }

    #[test]
    fn unlinked_permission_never_presents_stale_task_as_question() {
        let mut scene = Scene::new();
        let mut pending = session("claude:pending", State::Wait);
        pending.why = Why::Perm;
        scene.set_sessions(&[pending]);
        scene.focus_patron("claude:pending".into());
        scene.review_input(true, 200, 240, false);
        let review = scene.review.as_ref().unwrap();
        assert_eq!(review.request().title, "APPROVAL ON DESKTOP");
        assert!(review.request().text.contains("not linked to board approval"));
        assert!(!review.actionable);
    }

    #[test]
    fn review_keeps_request_identity_and_requires_verified_input() {
        let mut scene = Scene::new();
        let request = crate::approval::Request {
            choices: 0,
            tool_name: "Bash".into(),
            id: "one".into(),
            session: "claude:test".into(),
            title: "ALLOW?".into(),
            text: "Original command".into(),
        };
        scene.set_requests(vec![request.clone()]);
        scene.focus_patron(request.session.clone());
        scene.review_input(true, 200, 240, false);
        scene.review_input(false, 250, 240, false);
        assert!(scene.review.as_ref().unwrap().decision.is_none());
        scene.set_requests(vec![crate::approval::Request { id: "two".into(), ..request.clone() }]);
        scene.review_input(false, 250, 240, true);
        assert!(scene.review.as_ref().unwrap().decision.is_none());
        assert_eq!(scene.review.as_ref().unwrap().request().id, "one");
        scene.review = None;
        scene.set_requests(vec![request]);
        scene.review_input(true, 200, 240, false);
        scene.review_input(false, 250, 240, true);
        assert_eq!(scene.review.as_ref().unwrap().decision, Some(crate::approval::Decision::Allow));
        scene.review_input(false, 50, 240, true);
        assert_eq!(scene.review.as_ref().unwrap().decision, Some(crate::approval::Decision::Allow));
    }

    #[test]
    fn providers_keep_their_side_and_identity_across_refreshes() {
        let mut scene = Scene::new();
        let mut list: Vec<_> = (0..10).map(|i| session(&i.to_string(), State::Work)).collect();
        list.reverse(); // Collector order must not determine provider side.
        scene.set_sessions(&list);
        assert_eq!(scene.agents.len(), 8);
        assert_eq!(scene.overflow, 2);
        scene.set_details(
            list.iter()
                .map(|s| SessionDetail {
                    id: s.id.clone(),
                    task: String::new(),
                    target: Some(if s.kind == Kind::Codex {
                        SessionTarget::Codex(s.id.clone())
                    } else {
                        SessionTarget::Claude(format!("local_{}", s.id))
                    }),
                })
                .collect(),
        );
        let seats: Vec<_> = scene.agents.iter().map(|a| (a.id.clone(), a.seat)).collect();
        for a in &scene.agents {
            assert_eq!(a.seat % 4 >= 2, a.codex);
            let x = (a.seat % 4) as i32 * 120 + 60;
            let y = (a.seat / 4) as i32 * 88 + 80;
            assert_eq!(scene.patron_at(x, y), Some(a.id.as_str()));
        }
        for (id, seat) in &seats {
            let expected = scene.session_target(id).unwrap().url();
            assert_eq!(scene.activate_patron((*seat % 4) as i32 * 120 + 60, (*seat / 4) as i32 * 88 + 80), expected);
        }
        scene.focus_patron(seats[0].0.clone());
        list.reverse();
        scene.set_sessions(&list);
        assert_eq!(scene.selected.as_ref(), Some(&seats[0].0));
        for (id, seat) in seats {
            assert_eq!(scene.agents.iter().find(|a| a.id == id).unwrap().seat, seat);
        }
        // A single provider cannot occupy the other half, even if it is empty.
        let mut solo = Scene::new();
        for s in &mut list {
            s.kind = Kind::Codex;
        }
        solo.set_sessions(&list);
        assert_eq!(solo.agents.len(), 4);
        assert_eq!(solo.overflow, 6);
        assert!(solo.agents.iter().all(|a| a.seat % 4 >= 2));
    }

    #[test]
    fn already_active_fourth_patron_is_promoted_on_startup_for_both_providers() {
        for state in [State::Work, State::Wait] {
            let mut scene = Scene::new();
            let mut list: Vec<_> = (0..8).map(|i| session(&i.to_string(), State::Idle)).collect();
            list[5].state = state; // Claude seat 4
            list[7].state = state; // Codex seat 4
            scene.set_sessions(&list);
            scene.focus_patron("5".into());
            assert_eq!(scene.agents.iter().find(|a| a.id == "5").unwrap().seat, 0);
            assert_eq!(scene.agents.iter().find(|a| a.id == "7").unwrap().seat, 2);
            for _ in 0..200 {
                scene.tick(33);
                scene.set_sessions(&list);
            }
            assert_eq!(scene.patron_at(60, 80), Some("5"));
            assert_eq!(scene.patron_at(300, 80), Some("7"));
            assert_eq!(scene.selected.as_deref(), Some("5"));
            assert!(scene.agents.iter().all(|a| a.mode == Mode::Seated));
        }
    }

    #[test]
    fn active_patron_retries_promotion_after_idle_target_finishes_walking() {
        let mut scene = Scene::new();
        let mut list: Vec<_> = (0..6).map(|i| session(&i.to_string(), State::Idle)).collect();
        scene.set_sessions(&list);
        let first = scene.agents.iter_mut().find(|a| a.id == "0").unwrap();
        first.start_walk(&path_to(first.seat), Mode::Arriving);
        list[1].state = State::Work;
        scene.set_sessions(&list);
        // Seat 1 is temporarily unavailable while its idle patron arrives.
        assert_eq!(scene.agents.iter().find(|a| a.id == "1").unwrap().seat, 1);
        for _ in 0..600 {
            scene.tick(33);
        }
        assert_eq!(scene.patron_at(60, 80), Some("1"));
        assert!(scene.agents.iter().all(|a| a.mode == Mode::Seated));
    }

    #[test]
    fn resumed_patrons_swap_with_first_idle_seat_on_their_own_side() {
        let mut scene = Scene::new();
        let mut list: Vec<_> = (0..8).map(|i| session(&i.to_string(), State::Idle)).collect();
        scene.set_sessions(&list);
        scene.set_details(
            list.iter()
                .map(|s| SessionDetail {
                    id: s.id.clone(),
                    task: "Same task".into(),
                    target: Some(SessionTarget::Codex(s.id.clone())),
                })
                .collect(),
        );
        scene.focus_patron("4".into());
        list[4].state = State::Work;
        list[7].state = State::Wait;
        scene.set_sessions(&list);
        for (id, seat) in [("4", 0), ("0", 4), ("7", 2), ("2", 7)] {
            let a = scene.agents.iter().find(|a| a.id == id).unwrap();
            assert_eq!(a.seat, seat);
            assert!(a.mode == Mode::Swapping);
            assert!(a.path.iter().all(|p| (p[0] >= 240.0) == a.codex));
            assert_eq!(scene.patron_at((seat % 4) as i32 * 120 + 60, (seat / 4) as i32 * 88 + 80), None);
        }
        assert_eq!(scene.selected.as_deref(), Some("4"));
        scene.tick(300);
        let before: Vec<_> = scene.agents.iter().map(|a| (a.x, a.y, a.path_i)).collect();
        scene.set_sessions(&list); // Refresh must not restart an in-flight walk.
        assert_eq!(before, scene.agents.iter().map(|a| (a.x, a.y, a.path_i)).collect::<Vec<_>>());
        scene.tick(10000);
        assert_eq!(scene.agents.len(), 8);
        assert!(scene.agents.iter().all(|a| a.mode == Mode::Seated));
        assert_eq!(scene.activate_patron(60, 80).as_deref(), Some("codex://threads/4"));
        assert_eq!(scene.activate_patron(300, 80).as_deref(), Some("codex://threads/7"));
        list[4].state = State::Idle; // Becoming idle doesn't reshuffle everyone.
        scene.set_sessions(&list);
        assert_eq!(scene.agents.iter().find(|a| a.id == "4").unwrap().seat, 0);
    }

    #[test]
    fn swaps_are_ordered_and_can_be_interrupted_by_departure() {
        let mut scene = Scene::new();
        let mut list: Vec<_> = (0..8).map(|i| session(&i.to_string(), State::Idle)).collect();
        scene.set_sessions(&list);
        list[4].state = State::Work;
        list[5].state = State::Work;
        list.reverse();
        scene.set_sessions(&list);
        assert_eq!(scene.agents.iter().find(|a| a.id == "4").unwrap().seat, 0);
        assert_eq!(scene.agents.iter().find(|a| a.id == "5").unwrap().seat, 1);
        scene.tick(300);
        list.retain(|s| s.id != "4");
        scene.set_sessions(&list);
        scene.tick(20000);
        assert!(!scene.agents.iter().any(|a| a.id == "4"));
        assert_eq!(scene.agents.len(), 7);
        let mut seats: Vec<_> = scene.agents.iter().map(|a| a.seat).collect();
        seats.sort();
        seats.dedup();
        assert_eq!(seats.len(), 7);
    }

    #[test]
    fn reappearing_session_keeps_its_seat_and_remains_clickable() {
        let mut scene = Scene::new();
        scene.set_sessions(&[session("a", State::Work)]);
        scene.set_sessions(&[]);
        scene.tick(33);
        assert_eq!(scene.patron_at(60, 80), None);
        scene.set_sessions(&[session("a", State::Work)]);
        for _ in 0..300 {
            scene.tick(33);
        }
        assert_eq!(scene.patron_at(60, 80), Some("a"));
        assert_eq!(scene.agents.len(), 1);
        for n in 0..5 {
            assert!(shorten("long-name-abc", n).chars().count() <= n);
        }
    }

    #[test]
    fn navigation_targets_validate_ids_and_exclude_empty_seats() {
        assert_eq!(SessionTarget::Codex("abc-123".into()).url().as_deref(), Some("codex://threads/abc-123"));
        for id in ["", "abc?x=y", "../x", "a b"] {
            assert!(SessionTarget::Codex(id.into()).url().is_none());
        }
        assert!(SessionTarget::Claude("cli-uuid".into()).url().is_none());
        let mut scene = Scene::new();
        scene.set_sessions(&[session("a", State::Work)]);
        assert_eq!(scene.patron_at(60, 80), Some("a"));
        assert_eq!(scene.patron_at(210, 80), None);
        assert_eq!(scene.patron_at(40, 220), None);
        scene.click(60, 80);
        scene.focus_patron("a".into());
        assert_eq!(scene.selected.as_deref(), Some("a"));
        scene.click(60, 80);
        assert!(scene.selected.is_none());
        scene.set_sessions(&[]);
        assert_eq!(scene.patron_at(60, 80), None);
    }

    #[test]
    fn activation_focuses_linked_patron_and_rejects_missing_or_invalid_targets() {
        let mut scene = Scene::new();
        scene.set_sessions(&[session("a", State::Work)]);
        for (target, expected) in [
            (Some(SessionTarget::Codex("abc-123".into())), Some("codex://threads/abc-123")),
            (Some(SessionTarget::Claude("local_abc-123".into())), Some("claude://code/continue?session=local_abc-123")),
            (Some(SessionTarget::Claude("terminal-id".into())), None),
            (Some(SessionTarget::Codex("../bad".into())), None),
            (None, None),
        ] {
            scene.set_details(vec![SessionDetail { id: "a".into(), task: "Task".into(), target }]);
            scene.click(60, 80);
            assert_eq!(scene.activate_patron(60, 80).as_deref(), expected);
            assert_eq!(scene.selected.as_deref(), Some("a"));
            assert_eq!(scene.activate_patron(210, 80), None);
            assert_eq!(scene.activate_patron(210, 252), None);
        }
    }

    #[test]
    fn cached_pages_refresh_evict_and_clamp_scroll_boundaries() {
        let mut scene = Scene::new();
        scene.set_sessions(&[session("a", State::Work)]);
        let details = vec![SessionDetail {
            target: None,
            id: "a".into(),
            task: "one two three four five six seven eight nine ten eleven twelve thirteen fourteen".into(),
        }];
        scene.set_details(details.clone());
        scene.select_next();
        scene.scroll_task_to(999);
        assert_eq!(scene.task_page, scene.selected_task_pages().len() - 1);
        let allocation = scene.pages["a"].as_ptr();
        scene.set_details(details);
        assert_eq!(scene.pages["a"].as_ptr(), allocation);
        scene.scroll_task_to(-999);
        assert_eq!(scene.task_page, 0);
        scene.set_details(vec![SessionDetail { target: None, id: "a".into(), task: "Short".into() }]);
        scene.scroll_task_to(999);
        assert_eq!(scene.task_page, 0);
        assert_eq!(scene.selected_task_pages(), ["Short"]);
        scene.set_details(vec![]);
        assert!(scene.pages.is_empty());
        assert_eq!(scene.selected_task_pages(), ["TASK UNAVAILABLE"]);
        scene.set_sessions(&[]);
        assert!(scene.selected.is_none());
        let mut frame = vec![0; a::W * a::H];
        scene.render(&mut frame, NaiveTime::from_hms_opt(12, 0, 0).unwrap());
    }

    #[test]
    fn touch_targets_open_cycle_page_and_toggle_focus() {
        let mut scene = Scene::new();
        scene.set_sessions(&(0..9).map(|i| session(&i.to_string(), State::Work)).collect::<Vec<_>>());
        scene.click(210, 252);
        assert_eq!(scene.selected.as_deref(), Some("0"));
        scene.click(60, 80);
        assert!(scene.selected.is_none());
        scene.click(60, 80);
        assert_eq!(scene.selected.as_deref(), Some("0"));
        scene.set_details(vec![SessionDetail {
            target: None,
            id: "0".into(),
            task: "A long request that spans several pages of readable text".into(),
        }]);
        scene.click(450, 271);
        assert_eq!(scene.task_page, scene.selected_task_pages().len() - 1);
        for _ in 0..8 {
            scene.click(210, 252);
        }
        assert_eq!(scene.selected.as_deref(), Some("8")); // touch can reach overflow
        scene.click(210, 252);
        assert_eq!(scene.selected.as_deref(), Some("0"));
        scene.set_sessions(&[session("0", State::Work)]);
        scene.click(420, 190);
        assert!(scene.selected.is_none()); // empty desk clears
    }

    #[test]
    fn up_down_open_focus_clamp_pages_and_preserve_selected_patron() {
        let mut scene = Scene::new();
        scene.page_task_key(true);
        assert!(scene.selected.is_none());
        scene.set_sessions(&[session("a", State::Work), session("b", State::Idle)]);
        scene.set_details(vec![SessionDetail {
            target: None,
            id: "a".into(),
            task: "One two three four five six seven eight nine ten eleven twelve".into(),
        }]);
        scene.page_task_key(false);
        assert_eq!(scene.selected.as_deref(), Some("a"));
        assert_eq!(scene.task_page, 0);
        scene.page_task_key(false);
        assert_eq!(scene.task_page, 0);
        let last = scene.selected_task_pages().len() - 1;
        for _ in 0..20 {
            scene.page_task_key(true);
        }
        assert_eq!(scene.task_page, last);
        assert_eq!(scene.selected.as_deref(), Some("a"));
        for _ in 0..20 {
            scene.page_task_key(false);
        }
        assert_eq!(scene.task_page, 0);
        scene.select_next();
        scene.page_task_key(true);
        scene.page_task_key(false);
        assert_eq!(scene.selected.as_deref(), Some("b"));
        assert_eq!(scene.task_page, 0);
        scene.clear_selection();
        scene.page_task_key(true);
        assert_eq!(scene.task_page, 0);
        scene.set_details(vec![SessionDetail { target: None, id: "a".into(), task: "Short".into() }]);
        scene.page_task_key(true);
        assert_eq!(scene.task_page, 0);
    }

    #[test]
    fn banner_and_arrows_keep_highlight_and_footer_on_same_identity() {
        let mut scene = Scene::new();
        scene.set_sessions(&[session("a", State::Work), session("b", State::Work), session("c", State::Idle)]);
        scene.set_details(vec![SessionDetail {
            target: None,
            id: "b".into(),
            task: "Unique task for patron B".into(),
        }]);
        scene.click(150, 252); // open A
        scene.click(150, 252); // next B
        assert_eq!(scene.selected.as_deref(), Some("b"));
        assert_eq!(scene.agents.iter().find(|a| Some(&a.id) == scene.selected.as_ref()).unwrap().seat, 1);
        assert_eq!(scene.selected_task_pages()[0], "Unique task for patron B");
        scene.select_patron(false);
        assert_eq!(scene.selected.as_deref(), Some("a"));
        scene.select_patron(false);
        assert_eq!(scene.selected.as_deref(), Some("c"));
        scene.select_patron(true);
        assert_eq!(scene.selected.as_deref(), Some("a"));
        scene.select_patron(true);
        assert_eq!(scene.selected.as_deref(), Some("b"));
        scene.click(450, 271);
        assert_eq!(scene.selected.as_deref(), Some("b"));
        assert_eq!(scene.task_page, scene.selected_task_pages().len() - 1); // scrollbar changes text only
    }

    #[test]
    fn selection_tracks_identity_and_updates_without_resetting_pages() {
        let mut scene = Scene::new();
        scene.set_sessions(&[session("a", State::Work), session("b", State::Idle)]);
        scene.set_details(vec![SessionDetail {
            target: None,
            id: "b".into(),
            task: "A sufficiently long request to require multiple readable pages and preserve the selected page when the session list is refreshed".into(),
        }]);
        scene.click(180, 80);
        assert_eq!(scene.selected.as_deref(), Some("b"));
        scene.page_task(true);
        assert_eq!(scene.task_page, 1);
        scene.set_sessions(&[session("b", State::Work), session("a", State::Idle)]);
        scene.set_details(scene.details.clone());
        assert_eq!(scene.selected.as_deref(), Some("b"));
        assert_eq!(scene.task_page, 1);
        scene.set_details(vec![SessionDetail { target: None, id: "b".into(), task: "New request".into() }]);
        assert_eq!(scene.task_page, 0);
        scene.set_sessions(&[session("a", State::Work)]);
        assert!(scene.selected.is_none());
    }

    #[test]
    fn keyboard_selection_includes_overflow_and_page_wrap_handles_unicode() {
        let mut scene = Scene::new();
        scene.set_sessions(&(0..10).map(|i| session(&i.to_string(), State::Work)).collect::<Vec<_>>());
        for _ in 0..10 {
            scene.select_next();
        }
        assert_eq!(scene.selected.as_deref(), Some("9"));
        scene.select_next();
        assert_eq!(scene.selected.as_deref(), Some("0"));
        let pages = task_pages(&format!("{} other words", "é".repeat(49)));
        assert!(
            pages.iter().all(|p| p.lines().count() <= 2 && p.lines().all(|line| line.chars().count() <= TASK_LINE_LEN))
        );
        assert_eq!(pages.concat().replace('\n', "").matches("\\u{e9}").count(), 49);
        scene.clear_selection();
        scene.page_task(false);
        assert_eq!(scene.task_page, 0);
    }

    #[test]
    fn waiting_alert_remains_available_while_manual_focus_is_usable() {
        let mut scene = Scene::new();
        scene.set_sessions(&[session("a", State::Wait), session("b", State::Work)]);
        let mut before = vec![0; a::W * a::H];
        let mut selected = before.clone();
        let now = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
        scene.render(&mut before, now);
        scene.click(180, 80);
        scene.render(&mut selected, now);
        assert_ne!(&before[a::WIN_Y as usize * a::W..], &selected[a::WIN_Y as usize * a::W..]);
        scene.click(180, 80);
        scene.render(&mut selected, now);
        assert_eq!(&before[a::WIN_Y as usize * a::W..], &selected[a::WIN_Y as usize * a::W..]);
        scene.click(180, 80);
        scene.set_sessions(&[session("a", State::Idle), session("b", State::Work)]);
        scene.render(&mut selected, now);
        assert_ne!(&before[a::WIN_Y as usize * a::W..], &selected[a::WIN_Y as usize * a::W..]);
        assert_eq!(scene.selected.as_deref(), Some("b"));
    }

    #[test]
    fn shorten_keeps_the_suffix() {
        assert_eq!(shorten("esp-ee", 8), "esp-ee");
        assert_eq!(shorten("mithrandir-1c", 8), "mithr-1c");
        assert_eq!(shorten("checkout-flow-1c", 12), "checkout-1c"); // no "checkout--1c"
        assert_eq!(shorten("observability", 12), "observabilit");
    }
    fn patron(id: &str) -> Session {
        Session {
            id: id.into(),
            name: "Moon Cafe".into(),
            kind: Kind::Codex,
            state: State::Work,
            why: Why::None,
            pose: ForcedPose::Auto,
            age_s: 0,
            branch: Some(format!("feature/{id}")),
            active_subagents: Some(2),
        }
    }

    #[test]
    fn elapsed_time_never_selects_or_cycles_working_patrons() {
        let mut scene = Scene::new();
        scene.set_sessions(&[patron("one"), patron("two")]);
        let now = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
        let mut before = alloc::vec![0;a::W*a::H];
        let mut after = before.clone();
        scene.render(&mut before, now);
        for _ in 0..600 {
            scene.tick(100);
        }
        scene.render(&mut after, now);
        assert!(scene.selected_id().is_none());
        assert_eq!(&before[a::WIN_Y as usize * a::W..], &after[a::WIN_Y as usize * a::W..]);
        scene.focus_patron("two".into());
        for _ in 0..600 {
            scene.tick(100);
        }
        assert_eq!(scene.selected_id(), Some("two"));
        scene.clear_selection();
        scene.tick(60000);
        assert!(scene.selected_id().is_none());
    }
    #[test]
    fn waiting_focus_does_not_rotate_on_a_timer() {
        let mut scene = Scene::new();
        let mut list = [patron("one"), patron("two")];
        for s in &mut list {
            s.state = State::Wait;
            s.why = Why::Ask;
        }
        scene.set_sessions(&list);
        let first = scene.waiting_list()[scene.focus];
        for _ in 0..600 {
            scene.tick(100);
        }
        assert_eq!(scene.waiting_list()[scene.focus], first);
        scene.focus_patron("two".into());
        scene.tick(60000);
        assert_eq!(scene.selected_id(), Some("two"));
    }
}

#[cfg(test)]
mod background_tests {
    use super::*;
    #[test]
    fn compressed_background_matches_every_original_pixel() {
        let mut frame = vec![0xdeadbeef; a::W * a::H];
        paint_background(&mut frame);
        for (pixel, rgb) in frame.iter().zip(a::BG.as_chunks::<3>().0) {
            assert_eq!(*pixel, (rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | rgb[2] as u32);
        }
    }
}
