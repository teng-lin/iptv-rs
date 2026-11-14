//! Readers on a second connection while a writer replaces a catalog (WAL).

use crate::common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use common::*;
use iptv_providers::SecretUrl;
use iptv_store_sqlite::{ChannelQuery, GuideSource, OpenOptions, Store, StoreError};

#[test]
fn reader_sees_the_last_commit_while_a_write_is_pending() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let mut writer = Store::open(&path).unwrap();
    assert_eq!(writer.journal_mode().unwrap(), "wal");
    let src = writer.add_source(&xtream_source("Xt")).unwrap();
    replace(&mut writer, src, &catalog(10, 2, XT_PASSWORD));

    let mut reader = Store::open_with(
        &path,
        &OpenOptions {
            busy_timeout: Duration::from_millis(50),
            ..Default::default()
        },
    )
    .unwrap();

    // A second writer holds the write lock with uncommitted deletes.
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute_batch(
        "BEGIN IMMEDIATE;
         DELETE FROM channel_groups;
         DELETE FROM channels;",
    )
    .unwrap();

    // Reads proceed on the last committed snapshot, without waiting.
    let all = ChannelQuery::default();
    assert_eq!(reader.channel_count(src, &all).unwrap(), 10);
    assert_eq!(reader.load_channels(src, &all).unwrap().len(), 10);
    // Writes wait for the lock, then fail as busy instead of hanging.
    let id = catalog(1, 1, XT_PASSWORD).channels[0].id.clone();
    match reader.set_favourite(src, &id, true) {
        Err(StoreError::Sqlite(rusqlite::Error::SqliteFailure(e, _))) => {
            assert_eq!(e.code, rusqlite::ErrorCode::DatabaseBusy);
        }
        other => panic!("expected busy, got {other:?}"),
    }

    raw.execute_batch("ROLLBACK").unwrap();
    replace(&mut writer, src, &catalog(4, 1, XT_PASSWORD));
    assert_eq!(reader.channel_count(src, &all).unwrap(), 4);
}

/// The app's reader connection: it shares the writer's key, reads the
/// last commit while another connection holds the write lock, and refuses
/// every write, also through methods that take `&self`.
#[test]
fn a_reader_reads_during_a_held_write_and_cannot_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let key = iptv_store_sqlite::DatabaseKey::from_bytes([0x5a; 32]);
    let mut writer = Store::open_encrypted(&path, &key).unwrap();
    let src = writer.add_source(&xtream_source("Xt")).unwrap();
    replace(&mut writer, src, &catalog(10, 2, XT_PASSWORD));
    let reader = writer.open_reader().unwrap();
    assert_eq!(
        reader.schema_version().unwrap(),
        iptv_store_sqlite::SCHEMA_VERSION
    );

    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.pragma_update(None, "key", format!("x'{}'", "5a".repeat(32)))
        .unwrap();
    raw.execute_batch("BEGIN IMMEDIATE; DELETE FROM channel_groups; DELETE FROM channels;")
        .unwrap();
    let all = ChannelQuery::default();
    let started = std::time::Instant::now();
    assert_eq!(reader.channel_count(src, &all).unwrap(), 10);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the read waited for the writer: {:?}",
        started.elapsed()
    );
    raw.execute_batch("ROLLBACK").unwrap();

    // `query_only`: SQLite refuses the write whatever the method's receiver.
    let refused = |result: iptv_store_sqlite::Result<()>| match result {
        Err(StoreError::Sqlite(rusqlite::Error::SqliteFailure(e, _))) => {
            assert_eq!(e.code, rusqlite::ErrorCode::ReadOnly);
        }
        other => panic!("expected a read-only refusal, got {other:?}"),
    };
    refused(reader.mark_private_documents_unsealed());
    let mut reader = reader;
    let id = catalog(1, 1, XT_PASSWORD).channels[0].id.clone();
    refused(reader.set_favourite(src, &id, true));

    // It sees what the writer commits afterwards.
    replace(&mut writer, src, &catalog(4, 1, XT_PASSWORD));
    assert_eq!(reader.channel_count(src, &all).unwrap(), 4);
    assert!(
        Store::open_in_memory().unwrap().open_reader().is_err(),
        "an in-memory store has no second connection"
    );
}

/// Two refreshes save at once: a write that reads first (whether the
/// source exists) waits for the other writer's commit and goes on, instead
/// of failing as "database is locked" on a snapshot that commit made
/// stale (WAL's `SQLITE_BUSY_SNAPSHOT`, which no busy timeout waits out).
#[test]
fn writes_that_read_first_wait_for_another_writers_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let mut first = Store::open(&path).unwrap();
    let src = first.add_source(&xtream_source("Xt")).unwrap();
    let mut second = Store::open(&path).unwrap();
    // Another writer holds the lock and commits a change a moment later.
    let hold = |path: &std::path::Path| {
        let raw = rusqlite::Connection::open(path).unwrap();
        raw.execute_batch(
            "BEGIN IMMEDIATE;
             UPDATE sources SET updated_at = updated_at + 1;",
        )
        .unwrap();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            raw.execute_batch("COMMIT").unwrap();
        })
    };
    let other = hold(&path);
    second
        .update_source(src, &xtream_source("Renamed"))
        .unwrap();
    other.join().unwrap();
    let other = hold(&path);
    second.set_guide_sources(src, &[]).unwrap();
    other.join().unwrap();
    assert_eq!(first.source(src).unwrap().unwrap().source.name, "Renamed");
}

#[test]
fn concurrent_reads_never_see_a_partial_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let mut writer = Store::open(&path).unwrap();
    let src = writer.add_source(&xtream_source("Xt")).unwrap();
    let small = catalog(2_000, 20, XT_PASSWORD);
    let large = catalog(3_000, 30, "other-pass");
    replace(&mut writer, src, &small);

    let done = Arc::new(AtomicBool::new(false));
    let start = Arc::new(Barrier::new(2));
    let reader = {
        let (done, start, path) = (done.clone(), start.clone(), path.clone());
        thread::spawn(move || {
            let reader = Store::open(&path).unwrap();
            start.wait();
            let mut reads = 0u32;
            let mut seen = std::collections::BTreeSet::new();
            while !done.load(Ordering::Acquire) || reads < 5 {
                let count = reader.channel_count(src, &ChannelQuery::default()).unwrap();
                assert!(count == 2_000 || count == 3_000, "partial catalog: {count}");
                let page = reader
                    .load_channels(
                        src,
                        &ChannelQuery {
                            offset: 1_990,
                            limit: Some(20),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                assert!(
                    page.len() == 10 || page.len() == 20,
                    "torn page: {}",
                    page.len()
                );
                seen.insert(count);
                reads += 1;
            }
            (reads, seen)
        })
    };
    start.wait();
    for round in 0..10 {
        let next = if round % 2 == 0 { &large } else { &small };
        replace(&mut writer, src, next);
    }
    done.store(true, Ordering::Release);
    let (reads, seen) = reader.join().unwrap();
    assert!(reads >= 5);
    assert!(!seen.is_empty());
}

/// Two refreshes saving their playlists' guide sources at once (the app's
/// "Refresh all"), each on its own connection: neither fails as locked. A
/// write that reads first in a deferred transaction cannot wait for the
/// lock once another commit made its snapshot stale (`SQLITE_BUSY` at once,
/// whatever the busy timeout), so such writes take the lock up front.
#[test]
fn concurrent_guide_source_edits_wait_for_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let mut setup = Store::open(&path).unwrap();
    let sources = [
        setup.add_source(&m3u_source("A")).unwrap(),
        setup.add_source(&m3u_source("B")).unwrap(),
    ];
    drop(setup);
    let start = Arc::new(Barrier::new(2));
    let writers: Vec<_> = sources
        .into_iter()
        .map(|src| {
            let (start, path) = (start.clone(), path.clone());
            thread::spawn(move || {
                let mut store = Store::open(&path).unwrap();
                let guides = [GuideSource {
                    feed: iptv_epg::SourceId(1),
                    provider_index: Some(0),
                    url: SecretUrl::new("http://guide.example.test/g.xml"),
                    name: None,
                    enabled: true,
                    shift_minutes: 0,
                }];
                start.wait();
                for _ in 0..300 {
                    store.set_guide_sources(src, &guides).unwrap();
                }
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
}
