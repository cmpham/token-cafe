//! Synthetic native allocation/timing profile; never connects to a device.
//! Run with `cargo run --release -p companion-core --example transport_performance`.
use companion_core::{
    crypto::{Cipher, Keys},
    sync,
    transport::Transport,
    wire::{BlobChunk, envelope::Payload},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
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

fn main() {
    for capacity in [182, 244, 509] {
        let data = vec![0x51; capacity - 36];
        let mut tx = Cipher::new(&[3; 32], [4; 4], 1);
        let mut rx = Cipher::new(&[3; 32], [4; 4], 1);
        measure(&format!("fragment-roundtrip capacity={capacity}"), 20_000, || {
            let wire = tx.seal(1, data.len() as u32, 0, black_box(&data)).unwrap();
            let plain = rx.open(black_box(&wire)).unwrap();
            assert_eq!(&plain[12..], data);
            black_box(plain);
        });

        let keys = Keys::derive(&[0x31; 32], b"synthetic performance fixture");
        let mut sender = Transport::new(&keys, true, capacity, 0).unwrap();
        let mut receiver = Transport::new(&keys, false, capacity, 0).unwrap();
        let envelope = sync::envelope(
            &[1; 16],
            &[2; 16],
            Payload::BlobChunk(BlobChunk {
                sha256: vec![3; 32],
                data: vec![4; 512],
                total_length: 512,
                r#final: true,
                ..Default::default()
            }),
        );
        measure(&format!("blob-envelope capacity={capacity}"), 5_000, || {
            sender.enqueue(black_box(&envelope)).unwrap();
            let mut received = false;
            while let Some(wire) = sender.next_fragment().unwrap() {
                if let Some((result, _)) = receiver.receive(black_box(&wire), 0).unwrap() {
                    assert_eq!(result, envelope);
                    received = true;
                }
            }
            assert!(received);
        });
    }
}
