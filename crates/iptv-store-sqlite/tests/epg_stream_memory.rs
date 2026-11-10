//! Memory of the streaming guide import does not grow with the guide: the
//! Rust heap (every thread, counted by a global allocator) and SQLite's
//! own heap (`sqlite3_memory_highwater`) peak at the same level for a
//! guide eight times longer. The programmes go to the stage in batches of
//! bounded size; only the channel lists stay in memory.
//!
//! One test in this binary: the counters are process-wide.

mod common;
mod stream_support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use iptv_epg::{Overrides, SourceId, StoreOptions};
use iptv_import::{CancelToken, EpgSourceConfig, NoProgress, import_epg};
use iptv_providers::SecretUrl;
use iptv_store_sqlite::{EpgFeedMeta, EpgStage, Store};
use stream_support::{Feeds, GenShape, Serve, block_on};

static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);

fn add(n: isize) {
    let v = LIVE.fetch_add(n, Ordering::Relaxed) + n;
    PEAK.fetch_max(v, Ordering::Relaxed);
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

const NOW: i64 = 1_800_000_000;
const CHANNELS: usize = 500;

/// Peak Rust heap above the start, SQLite heap high-water while staging
/// and in all (bytes), programmes stored.
fn import(days: i64) -> (usize, usize, usize, u64) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("iptv.sqlite")).unwrap();
    let source = store.add_source(&common::m3u_source("Synthetic")).unwrap();
    let chans = stream_support::generated_playlist(CHANNELS);
    let shape = GenShape {
        channels: CHANNELS,
        days,
        from: NOW - days * 12 * 3600,
    };
    let fetch = Feeds::new().serve("g.xml", Serve::Generated(shape));
    let mut config = EpgSourceConfig::new(
        SourceId(1),
        SecretUrl::new(format!("{}g.xml", stream_support::BASE)),
    );
    config.store = StoreOptions::default();
    let configs = [config];

    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    // SAFETY: plain counter reads of the bundled SQLite.
    unsafe { rusqlite::ffi::sqlite3_memory_highwater(1) };
    let mut stage = EpgStage::create_in(dir.path()).unwrap();
    let epg = block_on(import_epg(
        &fetch,
        &configs,
        &chans,
        &Overrides::new(),
        NOW,
        &mut stage,
        &mut NoProgress,
        &CancelToken::new(),
    ))
    .unwrap();
    let staging = unsafe { rusqlite::ffi::sqlite3_memory_highwater(0) } as usize;
    let feeds = EpgFeedMeta::from_import(&epg, &configs, NOW);
    let counts = store
        .replace_epg(source, &mut stage, &epg, &feeds, 0)
        .unwrap();
    drop(stage);
    let peak = (PEAK.load(Ordering::Relaxed) - base).max(0) as usize;
    let sqlite = unsafe { rusqlite::ffi::sqlite3_memory_highwater(0) } as usize;
    (peak, staging, sqlite, counts.programmes)
}

#[test]
fn import_memory_does_not_grow_with_the_guide() {
    let mib = |b: usize| b as f64 / f64::from(1 << 20);
    let (short_heap, short_staging, short_sqlite, short_n) = import(1);
    let (long_heap, long_staging, long_sqlite, long_n) = import(8);
    eprintln!(
        "{short_n} programmes: heap {:.1} MiB, SQLite {:.1} MiB staging, {:.1} MiB in all; \
         {long_n} programmes: heap {:.1} MiB, SQLite {:.1} MiB staging, {:.1} MiB in all",
        mib(short_heap),
        mib(short_staging),
        mib(short_sqlite),
        mib(long_heap),
        mib(long_staging),
        mib(long_sqlite)
    );
    assert_eq!(long_n, 8 * short_n);
    assert_eq!(short_n, CHANNELS as u64 * 24);
    // Batches, parser buffers and the channel lists: a few MiB at most,
    // whatever the guide's length.
    assert!(long_heap < 12 << 20, "{long_heap}");
    assert!(
        long_heap < short_heap + (1 << 20),
        "{short_heap} -> {long_heap}"
    );
    // Page caches bound SQLite: 4 MiB for the stage while downloading,
    // 32 MiB for the store while copying.
    assert!(long_staging < 8 << 20, "{long_staging}");
    assert!(long_sqlite < 48 << 20, "{long_sqlite}");
}
