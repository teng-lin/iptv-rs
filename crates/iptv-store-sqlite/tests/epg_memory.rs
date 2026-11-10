//! Heap of the guide, measured with a counting allocator (this thread's
//! Rust allocations; SQLite's own heap is reported separately).
//!
//! ```sh
//! cargo test --release -p iptv-store-sqlite --test epg_memory -- --nocapture
//! cargo test --release -p iptv-store-sqlite --test epg_memory -- --ignored --nocapture
//! ```
//!
//! The first runs the 1000-channel guide (~83 programmes per channel, as
//! `make-iptv-fixtures.py --channels 1000 --days-back 2 --days-ahead 1`
//! writes it); the second the script's default (5000 channels, 9 days,
//! 1.2M programmes).

mod epg_synth;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use epg_synth::Shape;
use iptv_epg::{GuideStore, StoreOptions};
use iptv_guide::{EpgSchedule, GuideChannel};

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

fn add(n: isize) {
    // `try_with`: allocations during thread teardown are not counted.
    let _ = LIVE.try_with(|l| {
        let v = l.get() + n;
        l.set(v);
        let _ = PEAK.try_with(|p| p.set(p.get().max(v)));
    });
}

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        add(l.size() as isize);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        add(l.size() as isize);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        add(-(l.size() as isize));
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        add(new as isize - l.size() as isize);
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Live and peak bytes allocated by `f` on this thread, and its result.
fn measure<T>(f: impl FnOnce() -> T) -> (T, usize, usize) {
    let base = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(base));
    let out = f();
    let live = LIVE.with(Cell::get) - base;
    let peak = PEAK.with(Cell::get) - base;
    (out, live.max(0) as usize, peak.max(0) as usize)
}

fn mib(b: usize) -> f64 {
    b as f64 / f64::from(1 << 20)
}

fn schedule(shape: Shape, store: GuideStore) -> EpgSchedule {
    let channels = (0..shape.channels)
        .map(|n| GuideChannel {
            key: epg_synth::key(n),
            name: format!("Synth {n:04}"),
            catchup_days: 0,
        })
        .collect();
    EpgSchedule::new(vec![("All".into(), channels)], store, epg_synth::NOW)
}

struct Report {
    programmes: u64,
    live: usize,
    peak: usize,
}

/// The import path as the app runs it today: everything in memory, with
/// descriptions.
fn in_memory_import(shape: Shape) -> Report {
    let ((sched, programmes), live, peak) = measure(|| {
        let (guide, programmes) = epg_synth::import(shape, StoreOptions::default());
        (schedule(shape, guide), programmes)
    });
    eprintln!(
        "epg memory {}ch: in-memory import of {programmes} programmes: live {:.1} MiB, peak {:.1} MiB (store estimate {:.1} MiB)",
        shape.channels,
        mib(live),
        mib(peak),
        mib(sched.store().heap_bytes()),
    );
    Report {
        programmes,
        live,
        peak,
    }
}

#[test]
fn thousand_channel_guide() {
    let r = in_memory_import(Shape::THOUSAND);
    assert!(r.programmes > 80_000, "{}", r.programmes);
    assert!(r.peak >= r.live);
}

#[test]
#[ignore = "large: run with --release --ignored"]
fn default_fixture_guide() {
    let r = in_memory_import(Shape::DEFAULT_FIXTURE);
    assert!(r.programmes > 1_000_000, "{}", r.programmes);
}
