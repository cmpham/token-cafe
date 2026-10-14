//! Repeatable native allocation/CPU profile; hardware timing is measured separately.
use cafe_ui::{
    approval::Request,
    assets::{H, W},
    review::Review,
    scene::Scene,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
    time::Instant,
};
struct Count;
static CALLS: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            CALLS.fetch_add(1, Relaxed);
            let n = LIVE.fetch_add(l.size(), Relaxed) + l.size();
            PEAK.fetch_max(n, Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, n) };
        if !q.is_null() {
            CALLS.fetch_add(1, Relaxed);
            let live = LIVE.fetch_add(n, Relaxed) + n;
            LIVE.fetch_sub(l.size(), Relaxed);
            PEAK.fetch_max(live - l.size(), Relaxed);
        }
        q
    }
}
#[global_allocator]
static ALLOC: Count = Count;
fn main() {
    let initial = LIVE.load(Relaxed);
    let mut scene = Scene::new();
    let scene_bytes = LIVE.load(Relaxed) - initial;
    let mut fb = vec![0; W * H];
    let now = chrono::NaiveTime::from_hms_opt(12, 0, 0).unwrap();
    println!("scene_heap_bytes={scene_bytes}");
    for (name, text) in [
        ("short", "Short task notes".to_owned()),
        ("32k-ascii", "x".repeat(32768)),
        ("32k-controls", "\t".repeat(32768)),
    ] {
        scene.review = Some(Review::new(
            Request {
                id: "profile".into(),
                session: "profile".into(),
                title: "Profile".into(),
                tool_name: String::new(),
                text,
            },
            false,
        ));
        let base = LIVE.load(Relaxed);
        PEAK.store(base, Relaxed);
        CALLS.store(0, Relaxed);
        let start = Instant::now();
        for _ in 0..30 {
            scene.render(black_box(&mut fb), now);
            black_box(&fb);
        }
        let elapsed = start.elapsed().as_micros() / 30;
        let calls = CALLS.load(Relaxed) / 30;
        let extra = PEAK.load(Relaxed) - base;
        let hash = fb.iter().fold(0u64, |h, p| h.wrapping_mul(31).wrapping_add(*p as u64));
        println!(
            "{name}: us/frame={elapsed} allocations/frame={calls} transient_peak_bytes={extra} frame_hash={hash:016x}"
        );
    }
}
