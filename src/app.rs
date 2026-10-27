//! The window: ticks the scene in fixed 33 ms steps, renders the
//! 480x272 frame and shows it at the largest whole-number scale that fits,
//! letterboxed, so the pixel art stays crisp at any size or DPI. Only the
//! part of the window that changed since the buffer was last shown is
//! repainted and reported to the compositor: a quiet café costs little.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::error::Error;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use softbuffer::{Context, Rect, Surface};
use token_cafe::assets::{H, W};
use token_cafe::scene::Scene;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
#[cfg(target_os = "linux")]
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::window::{Fullscreen, Window, WindowId, WindowLevel};

use crate::Update;

const FRAME_MS: u64 = 33;
const FRAME: Duration = Duration::from_millis(FRAME_MS);
/// Presents remembered for repainting an older buffer (Wayland keeps two).
const HISTORY: usize = 4;

/// A half-open box of frame pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Area {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Area {
    const ALL: Area = Area { x0: 0, y0: 0, x1: W, y1: H };

    fn union(self, o: Area) -> Area {
        Area { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
}

/// The pixels that differ between two frames, or None.
fn diff(a: &[u32], b: &[u32]) -> Option<Area> {
    let mut area: Option<Area> = None;
    for (y, (ra, rb)) in a.as_chunks::<W>().0.iter().zip(b.as_chunks::<W>().0.iter()).enumerate() {
        let Some(x0) = ra.iter().zip(rb).position(|(p, q)| p != q) else { continue };
        let x1 = W - ra.iter().zip(rb).rev().position(|(p, q)| p != q).unwrap();
        let row = Area { x0, y0: y, x1, y1: y + 1 };
        area = Some(area.map_or(row, |a| a.union(row)));
    }
    area
}

/// Where the frame sits in a w x h window: scale k, offset (ox, oy).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Fit {
    k: usize,
    ox: usize,
    oy: usize,
}

fn fit(w: usize, h: usize) -> Fit {
    let k = (w / W).min(h / H).max(1);
    Fit { k, ox: w.saturating_sub(W * k) / 2, oy: h.saturating_sub(H * k) / 2 }
}

fn frame_point(x: f64, y: f64, width: usize, height: usize) -> Option<(i32, i32)> {
    let f = fit(width, height);
    let (x, y) = ((x - f.ox as f64) / f.k as f64, (y - f.oy as f64) / f.k as f64);
    (x >= 0.0 && y >= 0.0 && x < W as f64 && y < H as f64).then_some((x as i32, y as i32))
}

/// Paint one area of the frame into a w x h window buffer, scaled by the fit;
/// a full paint also blacks out the letterbox. Returns the window pixels
/// touched, or None when the area falls outside the window.
fn paint(fb: &[u32], out: &mut [u32], w: usize, h: usize, area: Area, full: bool) -> Option<Rect> {
    let f = fit(w, h);
    if full {
        out.fill(0);
    }
    let (x0, x1) = ((f.ox + area.x0 * f.k).min(w), (f.ox + area.x1 * f.k).min(w));
    let (y0, y1) = ((f.oy + area.y0 * f.k).min(h), (f.oy + area.y1 * f.k).min(h));
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let mut row = vec![0u32; x1 - x0];
    let mut row_of = usize::MAX;
    for y in y0..y1 {
        let sy = (y - f.oy) / f.k;
        if sy != row_of {
            let src = &fb[sy * W..(sy + 1) * W];
            for (i, px) in row.iter_mut().enumerate() {
                *px = src[(x0 + i - f.ox) / f.k];
            }
            row_of = sy;
        }
        out[y * w + x0..y * w + x1].copy_from_slice(&row);
    }
    let nz = |v: usize| NonZeroU32::new(v as u32).unwrap();
    Some(Rect { x: x0 as u32, y: y0 as u32, width: nz(x1 - x0), height: nz(y1 - y0) })
}

struct Shown {
    window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
}

#[derive(Default)]
struct Tap {
    contact: Option<(u64, f64, f64, bool)>,
}
impl Tap {
    fn event(&mut self, id: u64, phase: TouchPhase, x: f64, y: f64) -> Option<(f64, f64)> {
        if phase == TouchPhase::Started && self.contact.is_none() {
            self.contact = Some((id, x, y, true));
            return None;
        }
        let (active, sx, sy, valid) = self.contact.as_mut()?;
        if *active != id {
            *valid = false; // multitouch is not a tap
            return None;
        }
        if (x - *sx).hypot(y - *sy) > 12.0 {
            *valid = false;
        }
        match phase {
            TouchPhase::Ended => {
                let result = (*valid).then_some((x, y));
                self.contact = None;
                result
            }
            TouchPhase::Cancelled => {
                self.contact = None;
                None
            }
            _ => None,
        }
    }
}

/// Accumulate high-resolution trackpad deltas instead of paging for every pixel.
#[derive(Default)]
struct Wheel {
    remainder: f64,
    last: Option<Instant>,
}
impl Wheel {
    fn steps(&mut self, delta: MouseScrollDelta, scale: f64) -> i32 {
        let now = Instant::now();
        let amount = match delta {
            MouseScrollDelta::LineDelta(_, y) => y as f64,
            MouseScrollDelta::PixelDelta(p) => p.y / scale / 40.0,
        };
        if !amount.is_finite() {
            return 0;
        }
        if self.last.is_none_or(|last| now.duration_since(last) > Duration::from_millis(250))
            || (self.remainder != 0.0 && self.remainder.signum() != amount.signum())
        {
            self.remainder = 0.0;
        }
        self.last = Some(now);
        self.remainder += amount;
        let steps = self.remainder.trunc().clamp(-16.0, 16.0) as i32;
        self.remainder %= 1.0;
        steps
    }
}

#[derive(Default)]
struct DoubleClick {
    previous: Option<(String, i32, i32, Instant)>,
}

impl DoubleClick {
    fn click(&mut self, id: Option<&str>, x: i32, y: i32, now: Instant, interval: Duration) -> bool {
        let previous = self.previous.take();
        let Some(id) = id else {
            return false;
        };
        if previous.is_some_and(|(old, px, py, at)| {
            old == id && now.duration_since(at) <= interval && (x - px).abs() <= 4 && (y - py).abs() <= 4
        }) {
            return true;
        }
        self.previous = Some((id.to_owned(), x, y, now));
        false
    }
}

#[cfg(target_os = "macos")]
fn double_click_interval() -> Duration {
    Duration::from_secs_f64(unsafe { objc2_app_kit::NSEvent::doubleClickInterval() })
}

fn supports_pinning(display: winit::raw_window_handle::RawDisplayHandle) -> bool {
    use winit::raw_window_handle::RawDisplayHandle;
    matches!(display, RawDisplayHandle::AppKit(_) | RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_))
}

/// Permission requests override provider status while their connections are
/// live. The usual request-free frame can borrow the collector snapshot.
fn approval_sessions<'a>(
    providers: &'a [token_cafe::scene::Session],
    requests: &[token_cafe::approval::Request],
) -> Cow<'a, [token_cafe::scene::Session]> {
    use token_cafe::scene::{ForcedPose, Kind, Session, State, Why};
    if requests.is_empty() {
        return Cow::Borrowed(providers);
    }
    let mut sessions = providers.to_vec();
    for session in &mut sessions {
        if let Some(r) = requests.iter().find(|r| r.session == session.id) {
            session.state = State::Wait;
            session.why = if r.choices > 0 { Why::Ask } else { Why::Perm };
        }
    }
    for request in requests {
        if !sessions.iter().any(|s| s.id == request.session) {
            sessions.push(Session {
                id: request.session.clone(),
                name: if request.session.starts_with("codex:") { "Codex request" } else { "Claude approval" }.into(),
                kind: if request.session.starts_with("codex:") { Kind::Codex } else { Kind::Claude },
                state: State::Wait,
                why: if request.choices > 0 { Why::Ask } else { Why::Perm },
                pose: ForcedPose::Auto,
                age_s: 0,
                branch: None,
                active_subagents: None,
            });
        }
    }
    Cow::Owned(sessions)
}

struct App {
    #[cfg(feature = "bluetooth")]
    companion: Option<token_cafe::companion::Mirror>,
    #[cfg(feature = "bluetooth")]
    semantic: Option<token_cafe::companion::semantic::SemanticMirror>,
    approvals: Option<token_cafe::approval::Broker>,
    provider_sessions: Vec<token_cafe::scene::Session>,
    rx: Receiver<Update>,
    scale: u32,
    always_on_top: bool,
    pinning_supported: bool,
    last_window_title: String,
    shown: Option<Shown>,
    scene: Scene,
    fb: Vec<u32>,
    on_screen: Vec<u32>,
    /// What each recent present changed, newest last; None = everything.
    history: VecDeque<Option<Area>>,
    size: (usize, usize),
    /// Real time handed to the scene so far, in whole milliseconds.
    last: Instant,
    acc: u64,
    next: Instant,
    stale: bool,
    occluded: bool,
    cursor: Option<(f64, f64)>,
    dragging_scrollbar: bool,
    wheel: Wheel,
    tap: Tap,
    touch_scroll: Option<u64>,
    last_touch: Option<Instant>,
    double_click: DoubleClick,
    double_click_interval: Duration,
}

#[derive(Clone, Copy)]
#[cfg_attr(not(feature = "bluetooth"), allow(dead_code))]
pub enum CompanionConfig {
    Semantic,
    Legacy([u8; 32]),
}

pub fn run(
    rx: Receiver<Update>,
    scale: u32,
    always_on_top: bool,
    bluetooth: Option<CompanionConfig>,
) -> Result<(), Box<dyn Error>> {
    #[cfg(not(feature = "bluetooth"))]
    if bluetooth.is_some() {
        return Err("Built without Bluetooth support".into());
    }
    let event_loop = EventLoop::new()?;
    use winit::raw_window_handle::HasDisplayHandle;
    let pinning_supported = event_loop.display_handle().is_ok_and(|handle| supports_pinning(handle.as_raw()));
    if always_on_top && !pinning_supported {
        eprintln!("token-cafe: pinning is unavailable on this window backend; use a compositor window rule");
    }
    let now = Instant::now();
    let mut app = App {
        #[cfg(feature = "bluetooth")]
        companion: match bluetooth {
            Some(CompanionConfig::Legacy(key)) => Some(token_cafe::companion::Mirror::start(key)),
            _ => None,
        },
        #[cfg(feature = "bluetooth")]
        semantic: match bluetooth {
            Some(CompanionConfig::Semantic) => Some(token_cafe::companion::semantic::SemanticMirror::start()),
            _ => None,
        },
        approvals: token_cafe::approval::Broker::start().map_err(|e| eprintln!("Approval bridge: {e}")).ok(),
        provider_sessions: Vec::new(),
        rx,
        scale,
        always_on_top: always_on_top && pinning_supported,
        pinning_supported,
        last_window_title: String::new(),
        shown: None,
        scene: Scene::new(),
        fb: vec![0; W * H],
        on_screen: vec![0; W * H],
        history: VecDeque::new(),
        size: (0, 0),
        last: now,
        acc: 0,
        next: now,
        stale: true,
        occluded: false,
        cursor: None,
        dragging_scrollbar: false,
        wheel: Wheel::default(),
        tap: Tap::default(),
        touch_scroll: None,
        last_touch: None,
        double_click: DoubleClick::default(),
        double_click_interval: {
            #[cfg(target_os = "macos")]
            {
                double_click_interval()
            }
            #[cfg(not(target_os = "macos"))]
            {
                Duration::from_millis(500)
            }
        },
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Launch only on macOS; other platforms keep selection without attempting Mac URLs.
fn open_session(url: String) {
    #[cfg(target_os = "macos")]
    std::thread::spawn(move || match std::process::Command::new("/usr/bin/open").arg(&url).status() {
        Ok(status) if status.success() => eprintln!("token-cafe: opened session {url}"),
        result => eprintln!("token-cafe: could not open session: {result:?}"),
    });
    #[cfg(not(target_os = "macos"))]
    let _ = url;
}

impl App {
    fn click(&mut self, x: i32, y: i32) {
        #[cfg(target_os = "macos")]
        if let Some(shown) = &self.shown {
            // Borderless floating windows may receive mouse events without becoming key.
            shown.window.focus_window();
            self.update_window_appearance(true);
        }
        {
            if self.scene.usage_is_open() {
                self.scene.click(x, y);
                return;
            }
            if let Some(url) = self.scene.activate_terminal_patron(x, y) {
                self.double_click = DoubleClick::default();
                open_session(url);
                return;
            }
            let id = self.scene.interaction_id(x, y);
            if self.double_click.click(id.as_deref(), x, y, Instant::now(), self.double_click_interval) {
                let url = if self.scene.review.is_some() || y >= token_cafe::assets::WIN_Y {
                    self.scene.review_input(true, x, y, true)
                } else {
                    self.scene.activate_patron(x, y)
                };
                if let Some(url) = url {
                    open_session(url);
                }
            } else if self.scene.review.is_some() {
                if let Some(url) = self.scene.review_input(false, x, y, true) {
                    open_session(url);
                }
            } else {
                self.scene.click(x, y);
            }
        }
    }

    fn update_window_appearance(&self, focused: bool) {
        #[cfg(target_os = "macos")]
        {
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let Some(shown) = &self.shown else { return };
            // Remove the native header, including its traffic-light buttons.
            shown.window.set_decorations(focused);
            let Ok(handle) = shown.window.window_handle() else { return };
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
            // SAFETY: Winit supplies a live NSView, held alive by `shown.window`.
            // This method runs only on the AppKit event-loop (main) thread.
            unsafe {
                let view = &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>();
                if let Some(window) = view.window() {
                    window.setAlphaValue(if self.always_on_top && !focused { 0.6 } else { 1.0 });
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = focused;
    }

    fn window_level(&self) -> WindowLevel {
        if self.always_on_top { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal }
    }

    fn window_title(&self) -> String {
        let base = if !self.pinning_supported {
            "Token Café — Pin via compositor"
        } else if self.always_on_top {
            "Token Café — Pinned (T)"
        } else {
            "Token Café — Pin: T"
        };
        #[cfg(feature = "bluetooth")]
        if let Some(companion) = &self.semantic {
            return format!("{base} — BLE: {}", companion.status());
        }
        #[cfg(feature = "bluetooth")]
        if let Some(companion) = &self.companion {
            return format!("{base} — BLE: {}", companion.status());
        }
        base.into()
    }

    /// Apply what the feeders sent, advance the scene and render it.
    fn step(&mut self) {
        while let Ok(u) = self.rx.try_recv() {
            match u {
                Update::Sessions(list) => self.provider_sessions = list,
                Update::Details(details) => self.scene.set_details(details),
                Update::Usage(u) => self.scene.set_usage(u),
                Update::UsageRows(rows) => self.scene.set_usage_rows(rows),
                Update::UsageAccounts(rows) => self.scene.set_usage_accounts(rows),
            }
        }
        let requests = self.approvals.as_ref().map_or_else(Vec::new, |b| b.requests());
        let sessions = approval_sessions(&self.provider_sessions, &requests);
        self.scene.set_requests(requests);
        self.scene.set_sessions(&sessions);
        #[cfg(feature = "bluetooth")]
        if let Some(semantic) = &mut self.semantic {
            semantic.synchronize(&mut self.scene, self.approvals.as_ref());
        }
        let title = self.window_title();
        if title != self.last_window_title {
            if let Some(shown) = &self.shown {
                shown.window.set_title(&title);
            }
            self.last_window_title = title;
        }
        let now = Instant::now();
        let ms = (now - self.last).as_millis() as u64;
        self.last += Duration::from_millis(ms);
        self.acc = (self.acc + ms).min(1000); // after a stall, do not fast-forward
        while self.acc >= FRAME_MS {
            self.scene.tick(FRAME_MS as u32);
            self.acc -= FRAME_MS;
        }
        #[cfg(feature = "bluetooth")]
        if let Some(companion) = &mut self.companion {
            for (input, presented) in companion.drain_verified_input() {
                input.trace(&self.scene);
                let url = if let token_cafe::companion::Input::Tap(x, y) = input {
                    if self.scene.review.is_some() {
                        let expected =
                            token_cafe::companion::frame_fingerprint(&token_cafe::companion::encode_frame(&self.fb));
                        self.scene.review_input(false, x, y, presented == Some(expected) && expected != 0)
                    } else {
                        input.apply(&mut self.scene)
                    }
                } else {
                    input.apply(&mut self.scene)
                };
                if let Some(url) = url {
                    open_session(url);
                }
                self.scene.render(&mut self.fb, chrono::Local::now().time());
            }
        }
        if let Some(id) = self.scene.handoff_request.take()
            && let Some(broker) = &self.approvals
        {
            broker.cancel(&id);
        }
        // With a semantic companion, Store::apply derives the status from the shared model.
        #[cfg(feature = "bluetooth")]
        let shared_status = self.semantic.is_some();
        #[cfg(not(feature = "bluetooth"))]
        let shared_status = false;
        if let Some(review) = &mut self.scene.review {
            if !shared_status
                && let Some(outcome) = self.approvals.as_ref().and_then(|b| b.outcome(&review.request().id))
            {
                match outcome {
                    token_cafe::approval::Outcome::Delivered(_) => review.status = "DECISION RETURNED TO HOOK".into(),
                    token_cafe::approval::Outcome::Failed(_) => review.status = "DELIVERY FAILED - OPEN DESKTOP".into(),
                    token_cafe::approval::Outcome::Closed | token_cafe::approval::Outcome::External(_) => {
                        review.actionable = false;
                        review.status = match outcome {
                            token_cafe::approval::Outcome::External(Some(token_cafe::approval::Decision::Allow)) => {
                                "APPROVED ON DESKTOP"
                            }
                            token_cafe::approval::Outcome::External(Some(token_cafe::approval::Decision::Deny)) => {
                                "DENIED ON DESKTOP"
                            }
                            _ => "REQUEST RESOLVED",
                        }
                        .into();
                    }
                }
            }
            if let Some(decision) = review.decision.take() {
                let sent = self.approvals.as_ref().is_some_and(|b| b.decide(&review.request().id, decision));
                review.actionable = false;
                review.status = if sent { "SENDING DECISION..." } else { "REQUEST EXPIRED - OPEN DESKTOP" }.into();
            }
        }
        self.scene.render(&mut self.fb, chrono::Local::now().time());
        #[cfg(feature = "bluetooth")]
        if let Some(companion) = &mut self.companion {
            companion.publish(&self.fb);
        }
        if let Some(s) = &self.shown
            && !self.occluded
            && (self.stale || self.fb != self.on_screen)
        {
            s.window.request_redraw();
        }
    }

    fn present(&mut self) {
        let Some(s) = &mut self.shown else { return };
        let px = s.window.inner_size();
        let (Some(nw), Some(nh)) = (NonZeroU32::new(px.width), NonZeroU32::new(px.height)) else { return };
        let size = (px.width as usize, px.height as usize);
        if size != self.size {
            self.size = size;
            self.stale = true;
        }
        let changed = if self.stale { Some(None) } else { diff(&self.on_screen, &self.fb).map(Some) };
        let Some(changed) = changed else { return };
        if let Err(e) = s.surface.resize(nw, nh) {
            eprintln!("token-cafe: cannot resize the surface: {e}");
            return;
        }
        let mut buf = match s.surface.buffer_mut() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("token-cafe: no surface buffer: {e}");
                return;
            }
        };
        // The buffer shows the frame from `age` presents ago (0 = unknown):
        // repaint what changed in the presents since, plus this frame.
        let age = buf.age() as usize;
        let mut area = changed;
        if age == 0 || age > self.history.len() + 1 {
            area = None;
        } else {
            for past in self.history.iter().rev().take(age - 1) {
                area = match (area, past) {
                    (Some(a), Some(p)) => Some(a.union(*p)),
                    _ => None,
                };
            }
        }
        let (w, h) = size;
        let result = match paint(&self.fb, &mut buf, w, h, area.unwrap_or(Area::ALL), area.is_none()) {
            Some(damage) if area.is_some() => buf.present_with_damage(&[damage]),
            _ => buf.present(),
        };
        if let Err(e) = result {
            eprintln!("token-cafe: cannot present: {e}");
            self.history.clear();
            self.stale = true;
            return;
        }
        self.history.push_back(changed);
        if self.history.len() > HISTORY {
            self.history.pop_front();
        }
        self.on_screen.copy_from_slice(&self.fb);
        self.stale = false;
    }

    fn key(&mut self, el: &ActiveEventLoop, key: Key) {
        let Some(s) = &self.shown else { return };
        match key.as_ref() {
            Key::Character("q") => el.exit(),
            Key::Character("u" | "U") => self.scene.toggle_usage(),
            Key::Character("a" | "A") => self.scene.toggle_usage_accounts(),
            Key::Character("t" | "T") if self.pinning_supported => {
                self.always_on_top = !self.always_on_top;
                s.window.set_window_level(self.window_level());
                s.window.set_title(&self.window_title());
                self.update_window_appearance(s.window.has_focus());
            }
            Key::Character("f") | Key::Named(NamedKey::F11) => {
                let full = if s.window.fullscreen().is_some() { None } else { Some(Fullscreen::Borderless(None)) };
                s.window.set_fullscreen(full);
            }
            Key::Named(NamedKey::Escape) => {
                if s.window.fullscreen().is_some() {
                    s.window.set_fullscreen(None);
                } else {
                    self.scene.clear_selection();
                }
            }
            Key::Named(NamedKey::Tab) => self.scene.select_next(),
            Key::Named(NamedKey::ArrowRight) => self.scene.select_patron(true),
            Key::Named(NamedKey::ArrowLeft) => self.scene.select_patron(false),
            Key::Named(NamedKey::ArrowDown) => self.scene.page_task_key(true),
            Key::Named(NamedKey::ArrowUp) => self.scene.page_task_key(false),
            Key::Character(c) => {
                if let Some(k) = c.parse::<u32>().ok().filter(|k| (1..=6).contains(k)) {
                    let _ = s.window.request_inner_size(LogicalSize::new(W as u32 * k, H as u32 * k));
                }
            }
            _ => {}
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.shown.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.window_title())
            .with_window_level(self.window_level())
            .with_inner_size(LogicalSize::new(W as u32 * self.scale, H as u32 * self.scale))
            .with_min_inner_size(LogicalSize::new(W as u32, H as u32));
        // app_id (Wayland) / WM_CLASS (X11) "token-cafe", for window rules
        #[cfg(target_os = "linux")]
        let attrs = WindowAttributesExtWayland::with_name(attrs, "token-cafe", "token-cafe");
        let made = el.create_window(attrs).map_err(|e| e.to_string()).and_then(|window| {
            let window = Rc::new(window);
            let context = Context::new(window.clone()).map_err(|e| e.to_string())?;
            let surface = Surface::new(&context, window.clone()).map_err(|e| e.to_string())?;
            Ok(Shown { window, surface })
        });
        match made {
            Ok(s) => {
                s.window.request_redraw();
                let focused = s.window.has_focus();
                self.shown = Some(s);
                self.update_window_appearance(focused);
                self.stale = true;
            }
            Err(e) => {
                eprintln!("token-cafe: cannot open the window: {e}");
                el.exit();
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested => self.present(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.stale = true;
                if let Some(s) = &self.shown {
                    s.window.request_redraw();
                }
            }
            WindowEvent::Touch(touch) => {
                self.last_touch = Some(Instant::now());
                if let Some(s) = &self.shown {
                    let scale = s.window.scale_factor();
                    let size = s.window.inner_size();
                    let point =
                        frame_point(touch.location.x, touch.location.y, size.width as usize, size.height as usize);
                    let tap = self.tap.event(touch.id, touch.phase, touch.location.x / scale, touch.location.y / scale);
                    if let Some(id) = self.touch_scroll {
                        if id != touch.id || matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                            self.touch_scroll = None;
                        } else if let Some((_, y)) = point {
                            self.scene.scroll_task_to(y);
                        }
                    } else if touch.phase == TouchPhase::Started
                        && self.tap.contact.is_some_and(|(id, _, _, valid)| id == touch.id && valid)
                        && point.is_some_and(|(x, y)| self.scene.scrollbar_hit(x, y))
                    {
                        self.touch_scroll = Some(touch.id);
                        self.scene.scroll_task_to(point.unwrap().1);
                    } else if tap.is_some()
                        && let Some((x, y)) = point
                    {
                        self.click(x, y);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some((position.x, position.y));
                if self.dragging_scrollbar
                    && let Some(s) = &self.shown
                {
                    let size = s.window.inner_size();
                    if let Some((_, y)) = frame_point(position.x, position.y, size.width as usize, size.height as usize)
                    {
                        self.scene.scroll_task_to(y);
                    }
                }
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                self.dragging_scrollbar = false;
            }
            WindowEvent::Focused(focused) => {
                self.update_window_appearance(focused);
                if focused {
                    return;
                }
                self.dragging_scrollbar = false;
                self.touch_scroll = None;
                self.tap = Tap::default();
                #[cfg(target_os = "macos")]
                {
                    self.double_click = DoubleClick::default();
                }
                self.wheel = Wheel::default();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let (Some((x, y)), Some(s)) = (self.cursor, &self.shown) {
                    let size = s.window.inner_size();
                    if frame_point(x, y, size.width as usize, size.height as usize).is_some_and(|(_, y)| y >= 192) {
                        let steps = self.wheel.steps(delta, s.window.scale_factor());
                        for _ in 0..steps.abs() {
                            self.scene.page_task_key(steps < 0);
                        }
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                self.dragging_scrollbar = false;
                self.wheel = Wheel::default();
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                if self.last_touch.is_some_and(|t| t.elapsed() < Duration::from_millis(500)) {
                    return;
                }
                if let (Some((x, y)), Some(s)) = (self.cursor, &self.shown) {
                    let size = s.window.inner_size();
                    if let Some((x, y)) = frame_point(x, y, size.width as usize, size.height as usize) {
                        self.dragging_scrollbar = self.scene.scrollbar_hit(x, y);
                        self.click(x, y);
                    }
                }
            }
            WindowEvent::Occluded(o) => {
                self.occluded = o;
                self.stale = true;
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, repeat, .. },
                ..
            } if !repeat || matches!(logical_key, Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown)) => {
                self.key(el, logical_key);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next {
            self.step();
            self.next += FRAME;
            if self.next < now {
                self.next = now + FRAME;
            }
        }
        el.set_control_flow(ControlFlow::WaitUntil(self.next));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_overlay_borrows_quiet_frames_and_restores_provider_state() {
        use token_cafe::scene::{ForcedPose, Kind, Session, State, Why};
        let providers = vec![Session {
            id: "claude:moon".into(),
            name: "Moon Café".into(),
            kind: Kind::Claude,
            state: State::Work,
            why: Why::None,
            pose: ForcedPose::Auto,
            age_s: 17,
            branch: Some("feature/moon".into()),
            active_subagents: Some(2),
        }];
        assert!(matches!(approval_sessions(&providers, &[]), Cow::Borrowed(_)));
        let request = token_cafe::approval::Request {
            choices: 0,
            id: "permission".into(),
            session: providers[0].id.clone(),
            title: "Fixture permission".into(),
            tool_name: "Read".into(),
            text: "Synthetic fixture".into(),
        };
        let missing = token_cafe::approval::Request { session: "claude:missing".into(), ..request.clone() };
        let sessions = approval_sessions(&providers, &[request, missing.clone(), missing]);
        assert!(matches!(sessions, Cow::Owned(_)));
        assert_eq!(sessions.len(), 2, "multiple requests for one missing session add one patron");
        assert_eq!(sessions[0].state, State::Wait);
        assert_eq!(sessions[0].why, Why::Perm);
        assert_eq!(sessions[0].age_s, 17);
        assert_eq!(sessions[0].branch, providers[0].branch);
        assert_eq!(sessions[0].active_subagents, Some(2));
        assert_eq!(sessions[1].id, "claude:missing");
        assert_eq!(sessions[1].state, State::Wait);
        assert_eq!(sessions[1].why, Why::Perm);
        let restored = approval_sessions(&providers, &[]);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].state, State::Work);
        assert_eq!(restored[0].why, Why::None);
    }

    #[test]
    fn pinning_support_matches_native_backends() {
        use winit::raw_window_handle::{
            AppKitDisplayHandle, RawDisplayHandle, WaylandDisplayHandle, XcbDisplayHandle, XlibDisplayHandle,
        };
        assert!(supports_pinning(RawDisplayHandle::AppKit(AppKitDisplayHandle::new())));
        assert!(supports_pinning(RawDisplayHandle::Xlib(XlibDisplayHandle::new(None, 0))));
        assert!(supports_pinning(RawDisplayHandle::Xcb(XcbDisplayHandle::new(None, 0))));
        assert!(!supports_pinning(RawDisplayHandle::Wayland(WaylandDisplayHandle::new(std::ptr::NonNull::dangling()))));
    }

    #[test]
    fn double_click_requires_same_patron_time_and_position() {
        let mut clicks = DoubleClick::default();
        let now = Instant::now();
        let interval = Duration::from_millis(400);
        assert!(!clicks.click(Some("a"), 20, 80, now, interval));
        assert!(clicks.click(Some("a"), 22, 81, now + Duration::from_millis(200), interval));
        assert!(!clicks.click(Some("a"), 22, 81, now + Duration::from_millis(250), interval));
        assert!(!clicks.click(Some("b"), 22, 81, now + Duration::from_millis(300), interval));
        assert!(!clicks.click(Some("b"), 22, 81, now + Duration::from_millis(800), interval));
        assert!(!clicks.click(Some("b"), 32, 81, now + Duration::from_millis(900), interval));
        assert!(!clicks.click(None, 32, 220, now + Duration::from_millis(950), interval));
        assert!(!clicks.click(Some("b"), 32, 81, now + Duration::from_millis(1000), interval));
    }

    #[test]
    fn wheel_accumulates_retina_pixels_and_handles_direction_changes() {
        use winit::dpi::PhysicalPosition;
        let mut wheel = Wheel::default();
        for _ in 0..7 {
            assert_eq!(wheel.steps(MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 10.0)), 2.0), 0);
        }
        assert_eq!(wheel.steps(MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 10.0)), 2.0), 1);
        assert_eq!(wheel.steps(MouseScrollDelta::LineDelta(0.0, -2.0), 2.0), -2);
        assert_eq!(wheel.steps(MouseScrollDelta::LineDelta(0.0, 0.75), 1.0), 0);
        assert_eq!(wheel.steps(MouseScrollDelta::LineDelta(0.0, -0.5), 1.0), 0);
        assert_eq!(wheel.steps(MouseScrollDelta::LineDelta(0.0, -0.5), 1.0), -1);
    }

    #[test]
    fn touch_taps_ignore_drag_cancel_and_second_finger() {
        let mut tap = Tap::default();
        assert_eq!(tap.event(1, TouchPhase::Started, 20., 30.), None);
        assert_eq!(tap.event(1, TouchPhase::Ended, 22., 31.), Some((22., 31.)));
        tap.event(1, TouchPhase::Started, 20., 30.);
        tap.event(1, TouchPhase::Moved, 50., 30.);
        assert_eq!(tap.event(1, TouchPhase::Ended, 20., 30.), None);
        tap.event(1, TouchPhase::Started, 20., 30.);
        tap.event(1, TouchPhase::Cancelled, 20., 30.);
        assert_eq!(tap.event(1, TouchPhase::Ended, 20., 30.), None);
        tap.event(1, TouchPhase::Started, 20., 30.);
        tap.event(2, TouchPhase::Started, 20., 30.);
        assert_eq!(tap.event(1, TouchPhase::Ended, 20., 30.), None);
    }

    #[test]
    fn pointer_mapping_handles_retina_and_letterbox_edges() {
        assert_eq!(frame_point(60.0, 80.0, 480, 272), Some((60, 80)));
        assert_eq!(frame_point(479.0, 271.0, 480, 272), Some((479, 271)));
        for (x, y) in [(-1.0, 0.0), (480.0, 0.0), (0.0, -1.0), (0.0, 272.0)] {
            assert_eq!(frame_point(x, y, 480, 272), None);
        }
        // Uniform 2x scaling: 20px horizontal and 28px vertical padding.
        assert_eq!(frame_point(140.0, 188.0, 1000, 600), Some((60, 80)));
        assert_eq!(frame_point(19.0, 28.0, 1000, 600), None);
        assert_eq!(frame_point(980.0, 28.0, 1000, 600), None);
        assert_eq!(frame_point(20.0, 572.0, 1000, 600), None);
    }

    fn frame() -> Vec<u32> {
        (0..W * H).map(|i| (i * 7 + i / W) as u32 & 0xffff).collect()
    }

    #[test]
    fn fit_letterboxes_at_whole_scales() {
        assert_eq!(fit(2540, 1356), Fit { k: 4, ox: 310, oy: 134 });
        assert_eq!(fit(960, 720), Fit { k: 2, ox: 0, oy: 88 });
        assert_eq!(fit(200, 100), Fit { k: 1, ox: 0, oy: 0 }); // smaller than the frame: clipped
    }

    #[test]
    fn diff_finds_the_changed_box() {
        let a = frame();
        let mut b = a.clone();
        assert_eq!(diff(&a, &b), None);
        b[10 * W + 20] ^= 1;
        b[30 * W + 5] ^= 1;
        assert_eq!(diff(&a, &b), Some(Area { x0: 5, y0: 10, x1: 21, y1: 31 }));
    }

    #[test]
    fn partial_paint_equals_full_paint() {
        let (w, h) = (1000, 760);
        let old = frame();
        let mut new = old.clone();
        for y in 40..60 {
            for x in 100..130 {
                new[y * W + x] = 0xff0000;
            }
        }
        let mut full = vec![7u32; w * h];
        paint(&new, &mut full, w, h, Area::ALL, true);
        let mut part = vec![7u32; w * h];
        paint(&old, &mut part, w, h, Area::ALL, true);
        let area = diff(&old, &new).unwrap();
        let damage = paint(&new, &mut part, w, h, area, false).unwrap();
        assert!(full == part, "partial repaint differs from a full one");
        let f = fit(w, h);
        assert_eq!((damage.x, damage.y), ((f.ox + 100 * f.k) as u32, (f.oy + 40 * f.k) as u32));
        assert_eq!((damage.width.get(), damage.height.get()), (30 * f.k as u32, 20 * f.k as u32));
        // corners of the scaled frame and the letterbox
        assert_eq!(full[f.oy * w + f.ox], new[0]);
        assert_eq!(full[(f.oy + H * f.k - 1) * w + f.ox + W * f.k - 1], new[W * H - 1]);
        assert_eq!(full[0], 0);
    }
}
