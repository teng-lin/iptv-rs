//! Migrations from every historical schema, with data in it.
//!
//! `tests/fixtures/schema_vN.sql` is the exact DDL a build at version N
//! wrote (a `schema_dump`); `tests/fixtures/data_vN.sql` is synthetic data
//! in that shape. Each test builds an old file from those fixtures, opens it
//! with the current build and checks the data through the public API.
//!
//! Adding migration N+1: run
//! `SCHEMA_SNAPSHOT_OUT=tests/fixtures/schema_vN+1.sql cargo test -p iptv-store-sqlite --test migrations -- --ignored write_current_schema`
//! (relative to the crate), write `data_vN.sql` for the now historical
//! version N, and add it to `HISTORY` below.

mod common;

use std::path::Path;

use iptv_providers::model::{SourceConfig, SourceKind};
use iptv_providers::{ContentId, GroupId};
use iptv_store_sqlite::migrate::schema_dump;
use iptv_store_sqlite::{
    ChannelQuery, ChannelScope, ImportErrorKind, SCHEMA_VERSION, SourceId, Store,
};

#[test]
#[ignore = "tool: writes the current schema snapshot to $SCHEMA_SNAPSHOT_OUT"]
fn write_current_schema() {
    let out = std::env::var("SCHEMA_SNAPSHOT_OUT").expect("set SCHEMA_SNAPSHOT_OUT");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.db");
    drop(Store::open(&path).unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    std::fs::write(out, schema_dump(&conn).unwrap()).unwrap();
}

#[test]
fn latest_snapshot_is_the_fresh_schema() {
    let snapshot = std::fs::read_to_string(format!(
        "{}/tests/fixtures/schema_v{SCHEMA_VERSION}.sql",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("snapshot the latest schema (see module docs)");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.db");
    drop(Store::open(&path).unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(schema_dump(&conn).unwrap(), snapshot);
}

/// A historical version: its schema snapshot and the data files to load
/// into it, in order.
struct Version {
    version: u32,
    data: &'static [&'static str],
}

const HISTORY: &[Version] = &[
    Version {
        version: 1,
        data: &["data_v1.sql"],
    },
    Version {
        version: 2,
        data: &["data_v1.sql", "data_v2.sql"],
    },
];

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Writes a file exactly as a build at `v.version` left it.
fn build_old_file(path: &Path, v: &Version) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(&fixture(&format!("schema_v{}.sql", v.version)))
        .unwrap();
    for version in 1..=v.version {
        conn.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, 'historical', 0)",
            [version],
        )
        .unwrap();
    }
    conn.pragma_update(None, "user_version", v.version).unwrap();
    for data in v.data {
        conn.execute_batch(&fixture(data)).unwrap();
    }
}

fn fresh_schema() -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.db");
    drop(Store::open(&path).unwrap());
    schema_dump(&rusqlite::Connection::open(&path).unwrap()).unwrap()
}

const SRC: SourceId = SourceId(1);

fn id(s: &str) -> ContentId {
    ContentId::from_stored(s)
}

/// Data every version has (v1 provider data).
fn check_provider_data(store: &Store) {
    let panel = store.source(SRC).unwrap().unwrap();
    match &panel.source.config {
        SourceConfig::Xtream(c) => {
            assert_eq!(c.server_url, "http://panel.example.com:8080");
            assert_eq!(c.username.expose(), "fixture-user");
            assert_eq!(c.password.expose(), "fixture-pass");
        }
        other => panic!("wrong kind {other:?}"),
    }
    let list = store.source(SourceId(2)).unwrap().unwrap();
    assert_eq!(list.source.kind(), SourceKind::M3u);
    assert_eq!(store.sources().unwrap().len(), 2);

    let status = store.import_status(SRC).unwrap();
    assert_eq!(status.last_success_at, Some(1_700_000_100));
    assert_eq!(status.counts.channels, 3);
    assert_eq!(status.last_error, Some(ImportErrorKind::Timeout));

    let all = store
        .load_channels(
            SRC,
            &ChannelQuery {
                include_hidden: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        all.iter()
            .map(|r| r.channel.name.as_str())
            .collect::<Vec<_>>(),
        ["Fixture News", "Fixture Sport", "Fixture Radio"]
    );
    assert_eq!(all[0].channel.catch_up.as_ref().unwrap().days, 2);
    assert_eq!(all[2].channel.epg_shift_minutes, Some(60));
    assert!(all[2].channel.radio);
    let news = GroupId::from_stored("xt:live-group:1");
    let in_news = ChannelQuery {
        scope: ChannelScope::Group(news),
        include_hidden: true,
        ..Default::default()
    };
    assert_eq!(store.channel_count(SRC, &in_news).unwrap(), 2);
    assert_eq!(store.groups(SRC, None).unwrap().len(), 3);
    assert_eq!(
        store.load_vod(SRC, None, 0, None).unwrap()[0].year,
        Some(2019)
    );
    assert_eq!(
        store.load_series(SRC, None, 0, None).unwrap()[0].name,
        "Fixture Show"
    );
    assert_eq!(
        store.search_channels(Some(SRC), "sport", 5).unwrap().len(),
        1
    );
}

/// Overlays written by v2 builds.
fn check_overlays(store: &Store) {
    let visible = store.load_channels(SRC, &ChannelQuery::default()).unwrap();
    assert_eq!(
        visible.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["My News", "Fixture Sport"]
    );
    assert_eq!(visible[1].number, Some(10));
    assert!(visible[1].blocked);
    let favourites = store
        .load_channels(
            SRC,
            &ChannelQuery {
                favourites_only: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        favourites
            .iter()
            .map(|r| r.channel.id.as_str())
            .collect::<Vec<_>>(),
        ["xt:live:102", "xt:live:101"]
    );
    let orphans = store.orphaned_overlays(Some(SRC)).unwrap();
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].channel_id, id("xt:live:999"));
    assert!(orphans[0].favourite && orphans[0].customised);
    assert_eq!(
        store.search_channels(Some(SRC), "my news", 5).unwrap()[0]
            .channel
            .id,
        id("xt:live:101")
    );
}

#[test]
fn every_historical_version_migrates_with_its_data() {
    let fresh = fresh_schema();
    for v in HISTORY {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("v{}.db", v.version));
        build_old_file(&path, v);

        let store = Store::open(&path).unwrap_or_else(|e| panic!("v{}: {e}", v.version));
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        let migrated = schema_dump(&rusqlite::Connection::open(&path).unwrap()).unwrap();
        assert_eq!(
            migrated, fresh,
            "v{} migrated schema differs from fresh",
            v.version
        );
        check_provider_data(&store);
        if v.version >= 2 {
            check_overlays(&store);
        }
        drop(store);

        // Opening again (every app start) changes nothing.
        let again = Store::open(&path).unwrap();
        check_provider_data(&again);
        if v.version >= 2 {
            check_overlays(&again);
        }
    }
}

#[test]
fn overlays_survive_the_first_refresh_after_upgrading() {
    let v = HISTORY.iter().find(|v| v.version == 2).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    build_old_file(&path, v);
    let mut store = Store::open(&path).unwrap();
    let mut catalog = store
        .load_channels(
            SRC,
            &ChannelQuery {
                include_hidden: true,
                ..Default::default()
            },
        )
        .unwrap()
        .into_iter()
        .map(|r| r.channel)
        .collect::<Vec<_>>();
    // The provider now also lists the channel that was gone.
    let mut back = catalog[0].clone();
    back.id = id("xt:live:999");
    back.name = "Returned".into();
    catalog.push(back);
    let groups = store.groups(SRC, None).unwrap();
    store
        .replace_catalog(SRC, &groups, &catalog, &[], &[])
        .unwrap();
    check_overlays_after_return(&store);
}

fn check_overlays_after_return(store: &Store) {
    assert!(store.orphaned_overlays(Some(SRC)).unwrap().is_empty());
    let returned = store.channel(SRC, &id("xt:live:999")).unwrap().unwrap();
    assert_eq!(returned.name, "Gone For Now");
    assert!(returned.favourite);
    let favourites = store
        .load_channels(
            SRC,
            &ChannelQuery {
                favourites_only: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        favourites
            .iter()
            .map(|r| r.channel.id.as_str())
            .collect::<Vec<_>>(),
        ["xt:live:102", "xt:live:999", "xt:live:101"]
    );
}
