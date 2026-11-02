//! Synthetic allocation/timing profile for shared UI and desktop synchronization.
//! No process discovery, credentials, devices or running applications are accessed.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
use token_cafe::{
    patrons::Patrons,
    scene::{ForcedPose, Kind, Scene, Session, SessionDetail, State, Why},
};
struct Counter;
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(size as u64, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;

fn measure(label: &str, iterations: usize, mut f: impl FnMut()) {
    // Initialization and printing are intentionally outside the measured region.
    f();
    ALLOCATIONS.store(0, Ordering::Relaxed);
    BYTES.store(0, Ordering::Relaxed);
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let bytes = BYTES.load(Ordering::Relaxed);
    println!(
        "{label}: iterations={iterations} allocs/op={:.2} bytes/op={:.2} ns/op={:.0}",
        allocations as f64 / iterations as f64,
        bytes as f64 / iterations as f64,
        elapsed.as_nanos() as f64 / iterations as f64,
    );
}

fn sessions(count: usize) -> Vec<Session> {
    (0..count)
        .map(|i| Session {
            id: format!("synthetic-{i}"),
            name: format!("PROJECT-{i}"),
            kind: if i % 2 == 0 { Kind::Claude } else { Kind::Codex },
            state: if i % 3 == 0 { State::Wait } else { State::Work },
            why: if i % 3 == 0 { Why::Ask } else { Why::None },
            pose: ForcedPose::Auto,
            age_s: 0,
            branch: Some("feature/synthetic".into()),
            active_subagents: Some(2),
        })
        .collect()
}
fn main() {
    let list = sessions(8);
    let mut scene = Scene::new();
    scene.set_sessions(&list);
    measure("unchanged-session-reconcile-8", 2000, || scene.set_sessions(black_box(&list)));
    measure("animation-tick-8", 2000, || scene.tick(33));
    let task = "mixed é\t\n".repeat(3000);
    scene.set_details(vec![SessionDetail { id: list[0].id.clone(), task, target: None }]);
    scene.focus_patron(list[0].id.clone());
    measure("cached-task-anchor-30k", 100, || {
        scene.set_task_anchor(black_box(16000));
        black_box(scene.task_anchor());
    });
    let now = chrono::NaiveTime::from_hms_opt(12, 0, 0).unwrap();
    for roster in ["humans", "mixed"] {
        scene.patrons = Patrons::roster(roster).unwrap();
        let mut frame = vec![0; 480 * 272];
        measure(&format!("render-{roster}-native"), 200, || scene.render(black_box(&mut frame), now));
    }
    #[cfg(feature = "bluetooth")]
    for count in [8, 64] {
        let mut scene = Scene::new();
        let list = sessions(count);
        scene.set_sessions(&list);
        scene.set_details(
            list.iter()
                .map(|s| SessionDetail { id: s.id.clone(), task: "Synthetic task evidence".repeat(6), target: None })
                .collect(),
        );
        let mut store = token_cafe::companion::state::Store::new([1; 16]);
        store.ingest(&mut scene, None);
        measure(&format!("desktop-synchronize-{count}"), 500, || store.ingest(black_box(&mut scene), None));
    }
}
