//! Allocation instrumentation is compiled into separate binaries. Its timings
//! must never be used as latency measurements.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

struct CountingAllocator;
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);
static CALLS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

fn record(size: usize) {
    let live = LIVE.fetch_add(size as u64, Ordering::Relaxed) + size as u64;
    PEAK.fetch_max(live, Ordering::Relaxed);
    CALLS.fetch_add(1, Ordering::Relaxed);
    BYTES.fetch_add(size as u64, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the layout is forwarded unchanged to the system allocator.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the layout is forwarded unchanged to the system allocator.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: caller supplied a live allocation and its original layout.
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: the caller's allocation and size are forwarded unchanged.
        let next = unsafe { System.realloc(ptr, layout, size) };
        if !next.is_null() {
            LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
            record(size);
        }
        next
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

pub(super) struct Sample {
    live: u64,
    calls: u64,
    bytes: u64,
}

impl Sample {
    pub(super) fn start() -> Self {
        let live = LIVE.load(Ordering::Relaxed);
        PEAK.store(live, Ordering::Relaxed);
        Self {
            live,
            calls: CALLS.load(Ordering::Relaxed),
            bytes: BYTES.load(Ordering::Relaxed),
        }
    }

    pub(super) fn finish(self, iterations: usize) -> serde_json::Value {
        let calls = CALLS.load(Ordering::Relaxed) - self.calls;
        let bytes = BYTES.load(Ordering::Relaxed) - self.bytes;
        let peak_growth = PEAK.load(Ordering::Relaxed).saturating_sub(self.live);
        let retained_growth = LIVE.load(Ordering::Relaxed).saturating_sub(self.live);
        serde_json::json!({
            "calls_per_iteration":calls as f64 / iterations as f64,
            "requested_bytes_per_iteration":bytes as f64 / iterations as f64,
            "peak_live_growth_bytes":peak_growth,
            "retained_live_growth_bytes":retained_growth,
        })
    }
}
