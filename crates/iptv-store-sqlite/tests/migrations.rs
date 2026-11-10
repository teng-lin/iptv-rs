//! Migrations from every historical schema, with data in it.
//!
//! `tests/fixtures/schema_vN.sql` is the exact DDL a build at version N
//! wrote (a `schema_dump`); `tests/fixtures/data_vN.sql` is synthetic data
//! in that shape. Each test builds an old file from those fixtures, opens it
//! with the current build and checks the data through the public API.
//!
//! Adding migration N+1: run
//! `cargo test -p iptv-store-sqlite --lib -- --ignored write_missing_schema_snapshots`
//! (writes `schema_vN+1.sql`, never overwrites), write `data_vN+1.sql` with
//! synthetic data in the new shape, and add it to `HISTORY` below.

mod common;

use std::path::Path;

use iptv_epg::{ChannelKey, GuideStore, MatchIndex, Overrides, PlaylistChannel, SourceBuilder};
use iptv_import::EpgSourceConfig;
use iptv_providers::SecretUrl;
use iptv_providers::model::{ContentKind, SourceConfig, SourceKind};
use iptv_providers::{ContentId, GroupId};
use iptv_store_sqlite::migrate::schema_dump;
use iptv_store_sqlite::{
    ChannelOrder, ChannelQuery, ChannelScope, EpgFeedMeta, EpgOverride, EpgWindow, ImportErrorKind,
    SCHEMA_VERSION, SourceId, SourceSettings, Store,
};

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
    Version {
        version: 3,
        data: &["data_v1.sql", "data_v2.sql", "data_v3.sql"],
    },
    Version {
        version: 4,
        data: &["data_v1.sql", "data_v2.sql", "data_v3.sql", "data_v4.sql"],
    },
    Version {
        version: 5,
        data: &[
            "data_v1.sql",
            "data_v2.sql",
            "data_v3.sql",
            "data_v4.sql",
            "data_v5.sql",
        ],
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
                order: ChannelOrder::Provider,
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
    let catch_up = all[0].channel.catch_up.as_ref().unwrap();
    // Written before correction_secs existed and while days was required.
    assert_eq!((catch_up.days, catch_up.correction_secs), (Some(2), 0));
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

/// Overlays written by v2 builds (and what v3 data changes about them).
fn check_overlays(store: &Store, version: u32) {
    let visible = store.load_channels(SRC, &ChannelQuery::default()).unwrap();
    let expected: &[&str] = if version >= 3 {
        // v3 data adds a manual order of the all-channels list.
        &["Fixture Sport", "My News"]
    } else {
        &["My News", "Fixture Sport"]
    };
    assert_eq!(
        visible.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        expected
    );
    let sport = visible.iter().find(|r| r.name == "Fixture Sport").unwrap();
    assert_eq!(sport.number, Some(10));
    assert!(sport.blocked);
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
    let gone = orphans
        .iter()
        .find(|o| o.channel_id == id("xt:live:999"))
        .unwrap();
    assert!(gone.favourite && gone.customised);
    assert_eq!(gone.ordered && gone.in_custom_group, version >= 3);
    assert_eq!(orphans.len(), if version >= 3 { 2 } else { 1 });
    assert_eq!(
        store.search_channels(Some(SRC), "my news", 5).unwrap()[0]
            .channel
            .id,
        id("xt:live:101")
    );
    let news = store.channel(SRC, &id("xt:live:101")).unwrap().unwrap();
    if version < 3 {
        assert_eq!(news.epg_override, None, "v3 columns default to automatic");
    }
}

/// Organisation data written by v3 builds.
fn check_organisation(store: &Store) {
    let news = store.channel(SRC, &id("xt:live:101")).unwrap().unwrap();
    assert_eq!(
        news.epg_override,
        Some(EpgOverride::Xmltv {
            epg_source: "epg-fixture".into(),
            epg_channel: "alt.news.example".into()
        })
    );
    let groups = store.custom_groups(SRC).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(
        (groups[0].name.as_str(), groups[0].channel_count),
        ("Evening", 2)
    );
    let members = store
        .load_channels(
            SRC,
            &ChannelQuery {
                scope: ChannelScope::Custom(groups[0].id),
                include_hidden: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        members
            .iter()
            .map(|r| r.channel.id.as_str())
            .collect::<Vec<_>>(),
        ["xt:live:103", "xt:live:101"]
    );
    let recent = store.recent(Some(SRC), None, 10).unwrap();
    assert_eq!(
        recent
            .iter()
            .map(|h| (h.item_id.as_str(), h.watched_at))
            .collect::<Vec<_>>(),
        [
            ("xt:live:101", 1_700_000_900),
            ("xt:movie:5", 1_700_000_800)
        ]
    );
    assert_eq!(recent[1].position_secs, Some(1200));
    assert_eq!(
        store
            .recent(None, Some(ContentKind::Live), 10)
            .unwrap()
            .len(),
        1
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
            check_overlays(&store, v.version);
        }
        if v.version >= 3 {
            check_organisation(&store);
        }
        check_guide(&store, v.version);
        check_settings(&store, v.version);
        drop(store);

        // Opening again (every app start) changes nothing.
        let again = Store::open(&path).unwrap();
        check_provider_data(&again);
        if v.version >= 2 {
            check_overlays(&again, v.version);
        }
        if v.version >= 3 {
            check_organisation(&again);
        }
        check_guide(&again, v.version);
        check_settings(&again, v.version);
    }
}

/// Per-source settings and the shown source, written by v5 builds; none
/// before (older builds kept a typed guide address only with the guide).
fn check_settings(store: &Store, version: u32) {
    let typed = store.source_settings(SRC).unwrap();
    let removed = store.source_settings(SourceId(2)).unwrap();
    if version < 5 {
        assert_eq!((typed, removed), (None, None));
        assert_eq!(store.active_source().unwrap(), None);
        return;
    }
    let url = typed.unwrap().epg_url.unwrap();
    assert_eq!(url.expose(), "http://epg.example.com/typed.xml");
    assert_eq!(removed, Some(SourceSettings { epg_url: None }));
    assert_eq!(store.active_source().unwrap(), Some(SourceId(2)));
}

/// Guide data written by v4 builds; none before.
fn check_guide(store: &Store, version: u32) {
    let guide = store.epg_guide(SRC).unwrap();
    if version < 4 {
        assert_eq!(guide, None);
        assert_eq!(store.epg_guide(SourceId(2)).unwrap(), None);
        return;
    }
    let guide = guide.unwrap();
    assert_eq!(guide.replaced_at, Some(1_700_001_000));
    assert_eq!(guide.match_fingerprint, Some(4242));
    assert_eq!(
        (guide.bound_channels, guide.ends_at),
        (3, Some(1_700_014_400))
    );
    assert_eq!(guide.last_error, None);
    let configs = guide.configs();
    assert_eq!(configs.len(), 2);
    assert_eq!(configs[0].priority, 2, "highest priority first");
    assert!(configs[0].url.expose().contains("xmltv.php"));
    assert_eq!(configs[0].store.past_days_to_keep, Some(2));
    assert_eq!(configs[1].user_agent.as_deref(), Some("Fixture/1"));
    assert_eq!(
        (
            configs[1].time.assumed_offset_minutes,
            configs[1].time.shift_minutes
        ),
        (60, -30)
    );
    assert!(!configs[1].store.store_descriptions);
    assert_eq!(
        guide.feeds[0].meta.etag.as_deref(),
        Some("\"fixture-etag\"")
    );
    assert_eq!(guide.feeds[0].programme_count, 3);
    let state = guide.refresh_state();
    assert_eq!(state.feeds, configs);
    assert!(!state.last_attempt_failed);

    let failed = store.epg_guide(SourceId(2)).unwrap().unwrap();
    assert_eq!(failed.last_error, Some(ImportErrorKind::Timeout));
    assert_eq!(failed.replaced_at, None);
    assert!(failed.feeds.is_empty());

    let g = store
        .load_epg(SRC, &EpgWindow::from(1_700_000_000, 0))
        .unwrap();
    assert_eq!(g.bound_count(), 3);
    let news = ChannelKey::new("xt:live:101");
    let titles: Vec<String> = g
        .programmes(&news, 0, i64::MAX)
        .map(|p| p.title().to_owned())
        .collect();
    assert_eq!(
        titles,
        ["Morning Fixture", "Fixture Midday", "Fixture Evening"]
    );
    let midday = g.programme_at(&news, 1_700_004_000).unwrap();
    assert_eq!(midday.season_episode(), (Some(1), Some(4)));
    assert_eq!(
        (midday.sub_title(), midday.rating()),
        (Some("Part two"), Some("12"))
    );
    assert_eq!(midday.desc(), None, "descriptions load on demand");
    // The radio channel shares the news EPG channel (an override).
    let radio = ChannelKey::new("xt:live:103");
    assert_eq!(
        g.programme_at(&radio, 1_700_000_100).unwrap().title(),
        "Morning Fixture"
    );
    assert_eq!(
        store
            .epg_description(SRC, &radio, 1_700_003_600)
            .unwrap()
            .as_deref(),
        Some("An invented midday programme.")
    );
    let sport = ChannelKey::new("xt:live:102");
    assert_eq!(
        g.programme_at(&sport, 1_700_000_100).unwrap().icon(),
        Some("http://img.example.com/m.png")
    );
}

/// A small guide matched against the fixture channels.
fn tiny_guide() -> (GuideStore, EpgSourceConfig) {
    let cfg = EpgSourceConfig::new(
        iptv_epg::SourceId(1),
        SecretUrl::new("http://epg.example.com/guide.xml"),
    );
    let mut b = SourceBuilder::new(cfg.id, 0, cfg.store, 1_700_000_000);
    let xml = format!(
        "<tv><channel id=\"news.example\"><display-name>Fixture News</display-name></channel>{}</tv>",
        "<programme channel=\"news.example\" start=\"20231114221320 +0000\" stop=\"20231114231320 +0000\"><title>After Upgrade</title><desc>New data.</desc></programme>"
    );
    let parsed = iptv_epg::parse_xmltv(xml.as_bytes(), Default::default()).unwrap();
    parsed.channels.into_iter().for_each(|c| b.push_channel(c));
    parsed
        .programmes
        .into_iter()
        .for_each(|p| b.push_programme(p));
    let src = b.finish();
    let mut index = MatchIndex::new();
    index.add_source(src.id(), src.priority(), src.channels());
    let key = ChannelKey::new("xt:live:101");
    let matched = index.match_channels(
        [PlaylistChannel {
            key: &key,
            tvg_id: Some("news.example"),
            name: "Fixture News",
            tvg_name: None,
        }],
        &Overrides::new(),
    );
    let mut guide = GuideStore::new();
    guide.replace_source(src);
    guide.set_matches(matched.matches);
    (guide, cfg)
}

/// The first guide refresh after upgrading (from any version) replaces the
/// stored guide and touches nothing else.
#[test]
fn first_guide_refresh_after_upgrading() {
    for v in HISTORY {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        build_old_file(&path, v);
        let mut store = Store::open(&path).unwrap();
        let (guide, cfg) = tiny_guide();
        let feeds = [EpgFeedMeta {
            config: cfg,
            fetched_at: 1_700_002_000,
            etag: None,
            last_modified: None,
            bytes: 10,
        }];
        let counts = store.replace_epg(SRC, &guide, &feeds, 7).unwrap();
        assert_eq!((counts.channels, counts.programmes), (1, 1));
        let stored = store.epg_guide(SRC).unwrap().unwrap();
        assert_eq!((stored.feeds.len(), stored.match_fingerprint), (1, Some(7)));
        let g = store
            .load_epg(SRC, &EpgWindow::from(1_700_000_000, 1))
            .unwrap();
        assert_eq!(g.bound_count(), 1, "v{}: old bindings replaced", v.version);
        check_provider_data(&store);
        if v.version >= 2 {
            check_overlays(&store, v.version);
        }
        if v.version >= 3 {
            check_organisation(&store);
        }
        if v.version >= 4 {
            // The other source's failed attempt is untouched.
            let other = store.epg_guide(SourceId(2)).unwrap().unwrap();
            assert_eq!(other.last_error, Some(ImportErrorKind::Timeout));
        }
    }
}

#[test]
fn overlays_survive_the_first_refresh_after_upgrading() {
    for v in HISTORY.iter().filter(|v| v.version >= 2) {
        first_refresh_after_upgrading(v);
    }
}

fn first_refresh_after_upgrading(v: &Version) {
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
    if v.version >= 3 {
        let evening = &store.custom_groups(SRC).unwrap()[0];
        assert_eq!(
            evening.channel_count, 3,
            "the returning member counts again"
        );
        let order = store.load_channels(SRC, &ChannelQuery::default()).unwrap();
        assert_eq!(
            order
                .iter()
                .map(|r| r.channel.id.as_str())
                .collect::<Vec<_>>(),
            ["xt:live:102", "xt:live:999", "xt:live:101"],
            "the returning channel regains its manual place"
        );
    }
}

fn check_overlays_after_return(store: &Store) {
    assert!(
        store
            .orphaned_overlays(Some(SRC))
            .unwrap()
            .iter()
            .all(|o| o.channel_id != id("xt:live:999"))
    );
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

/// After upgrading from any version, the playlists screen can save a guide
/// address and the shown source, and deleting a source takes them along.
#[test]
fn playlist_settings_after_upgrading() {
    for v in HISTORY {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        build_old_file(&path, v);
        let mut store = Store::open(&path).unwrap();
        let url = SecretUrl::new("http://epg.example.com/changed.xml");
        store.set_source_epg_url(SRC, Some(&url)).unwrap();
        store.set_active_source(SRC).unwrap();
        drop(store);
        let mut store = Store::open(&path).unwrap();
        assert_eq!(
            store.source_settings(SRC).unwrap().unwrap().epg_url,
            Some(url),
            "v{}",
            v.version
        );
        assert_eq!(store.active_source().unwrap(), Some(SRC));
        // The other source still has what it had.
        check_guide(&store, v.version);
        assert!(store.delete_source(SRC).unwrap());
        assert_eq!(store.active_source().unwrap(), None);
        assert_eq!(store.sources().unwrap().len(), 1);
        assert!(store.source(SourceId(2)).unwrap().is_some());
    }
}
