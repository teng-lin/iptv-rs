//! Stored guide data: replace, windows, lazy descriptions, prune, delete.

mod common;
mod epg_synth;
mod staged;

use epg_synth::{NOW, Shape};
use iptv_epg::{
    ChannelKey, EpgRef, GuideStore, MatchKind, ProgrammeData, SourceBuilder, StoreOptions,
    XmltvProgramme,
};
use iptv_import::{EpgSourceConfig, FailureKind, GuideState};
use iptv_providers::SecretUrl;
use iptv_store_sqlite::{EpgFeedMeta, EpgWindow, ImportErrorKind, SourceId, Store};

const H: i64 = 3600;

fn store_with_sources() -> (Store, SourceId, SourceId) {
    let mut store = Store::open_in_memory().unwrap();
    store.set_clock(|| NOW);
    let a = store.add_source(&common::xtream_source("A")).unwrap();
    let b = store.add_source(&common::m3u_source("B")).unwrap();
    (store, a, b)
}

fn config(id: u32, priority: i32) -> EpgSourceConfig {
    EpgSourceConfig::new(
        iptv_epg::SourceId(id),
        SecretUrl::new(format!(
            "http://epg.example.com/{id}.xml?password={}",
            common::XT_PASSWORD
        )),
    )
    .with_priority(priority)
}

fn meta(config: EpgSourceConfig) -> EpgFeedMeta {
    EpgFeedMeta {
        config,
        fetched_at: NOW,
        etag: Some("\"v1\"".into()),
        last_modified: None,
        bytes: 1234,
    }
}

fn prog(channel: &str, start: i64, stop: i64, title: &str, desc: Option<&str>) -> XmltvProgramme {
    XmltvProgramme {
        channel: channel.into(),
        start,
        stop: Some(stop),
        title: title.into(),
        desc: desc.map(str::to_owned),
        ..XmltvProgramme::default()
    }
}

/// Two feeds: "hi" (priority 9) has `x`, "lo" (1) has `x` and `y`.
/// Keys: k1 -> x (hi wins), k2 -> y (lo), k3 -> x as well, k4 -> missing.
fn two_feed_guide(tag: &str) -> GuideStore {
    let mut hi = SourceBuilder::new(iptv_epg::SourceId(1), 9, StoreOptions::default(), NOW);
    let mut lo = SourceBuilder::new(iptv_epg::SourceId(2), 1, StoreOptions::default(), NOW);
    for i in -3..6 {
        let t = NOW + i * H;
        hi.push_programme(prog("x", t, t + H, &format!("{tag} X{i}"), Some("About X")));
        lo.push_programme(prog("x", t, t + H, "lo X", None));
        lo.push_programme(prog("y", t, t + 2 * H, &format!("{tag} Y{i}"), None));
    }
    let mut g = GuideStore::new();
    g.replace_source(hi.finish());
    g.replace_source(lo.finish());
    let cand = |s: u32, id: &str, kind| EpgRef {
        source: iptv_epg::SourceId(s),
        epg_id: id.into(),
        kind,
    };
    g.set_matches(
        [
            (
                "k1",
                vec![
                    cand(1, "x", MatchKind::TvgId),
                    cand(2, "x", MatchKind::TvgId),
                ],
            ),
            (
                "k2",
                vec![cand(1, "y", MatchKind::Name), cand(2, "y", MatchKind::Name)],
            ),
            ("k3", vec![cand(1, "x", MatchKind::Override)]),
            ("k4", vec![cand(1, "nothing", MatchKind::TvgId)]),
        ]
        .into_iter()
        .map(|(k, c)| (ChannelKey::from(k), c))
        .collect(),
    );
    g
}

fn titles(g: &GuideStore, key: &str, from: i64, to: i64) -> Vec<String> {
    g.programmes(&key.into(), from, to)
        .map(|p| p.title().to_owned())
        .collect()
}

#[test]
fn replace_then_load_round_trips_bindings_and_priorities() {
    let (mut store, a, _) = store_with_sources();
    let feeds = [meta(config(1, 9)), meta(config(2, 1))];
    let counts = staged::replace_epg(&mut store, a, &two_feed_guide("v1"), &feeds, 99).unwrap();
    // x from hi (shared by k1 and k3), y from lo: two EPG channels stored.
    assert_eq!((counts.channels, counts.bindings), (2, 3));
    assert_eq!(counts.programmes, 18);
    assert_eq!(counts.descriptions, 9);

    let g = store.load_epg(a, &EpgWindow::from(NOW, 1)).unwrap();
    assert_eq!(g.bound_count(), 3);
    assert_eq!(titles(&g, "k1", NOW, NOW + H), ["v1 X0"]);
    assert_eq!(titles(&g, "k3", NOW, NOW + H), ["v1 X0"]);
    assert_eq!(titles(&g, "k2", NOW, NOW + H), ["v1 Y0"]);
    assert_eq!(g.binding(&"k3".into()), Some((iptv_epg::SourceId(1), "x")));
    assert!(g.programme_at(&"k4".into(), NOW).is_none());

    let stored = store.epg_guide(a).unwrap().unwrap();
    assert_eq!(stored.replaced_at, Some(NOW));
    assert_eq!(stored.ends_at, Some(NOW + 7 * H));
    assert_eq!(
        stored
            .feeds
            .iter()
            .map(|f| (f.meta.config.id.0, f.channel_count, f.programme_count))
            .collect::<Vec<_>>(),
        [(1, 1, 9), (2, 1, 9)]
    );
    assert_eq!(stored.feeds[0].meta, feeds[0]);
    let state: GuideState = stored.refresh_state();
    assert_eq!(state.match_fingerprint, Some(99));
    assert_eq!(state.bound_channels, 3);
}

#[test]
fn windows_keys_and_descriptions() {
    let (mut store, a, _) = store_with_sources();
    let feeds = [meta(config(1, 9)), meta(config(2, 1))];
    staged::replace_epg(&mut store, a, &two_feed_guide("v1"), &feeds, 0).unwrap();
    // Overlapping [now - 30 min, now + 90 min): X-1 (ends at now) is out.
    let w = EpgWindow {
        from: NOW - H / 2,
        to: NOW + H + H / 2,
        keys: None,
        descriptions: false,
    };
    let g = store.load_epg(a, &w).unwrap();
    assert_eq!(titles(&g, "k1", 0, i64::MAX), ["v1 X-1", "v1 X0", "v1 X1"]);
    let only = [ChannelKey::from("k2")];
    let g = store
        .load_epg(
            a,
            &EpgWindow {
                keys: Some(&only),
                ..w
            },
        )
        .unwrap();
    assert_eq!(g.bound_count(), 1);
    assert!(g.programme_at(&"k1".into(), NOW).is_none());
    assert_eq!(titles(&g, "k2", 0, i64::MAX), ["v1 Y-1", "v1 Y0", "v1 Y1"]);

    let lean = store.load_epg(a, &w).unwrap();
    assert_eq!(lean.programme_at(&"k1".into(), NOW).unwrap().desc(), None);
    let full = store
        .load_epg(
            a,
            &EpgWindow {
                descriptions: true,
                ..w
            },
        )
        .unwrap();
    assert_eq!(
        full.programme_at(&"k1".into(), NOW).unwrap().desc(),
        Some("About X")
    );
    assert_eq!(
        store
            .epg_description(a, &"k3".into(), NOW)
            .unwrap()
            .as_deref(),
        Some("About X")
    );
    assert_eq!(store.epg_description(a, &"k2".into(), NOW).unwrap(), None);
    assert_eq!(
        store.epg_description(a, &"k1".into(), NOW + 1).unwrap(),
        None
    );
}

#[test]
fn replace_is_per_source_and_total() {
    let (mut store, a, b) = store_with_sources();
    let feeds = [meta(config(1, 9)), meta(config(2, 1))];
    staged::replace_epg(&mut store, a, &two_feed_guide("old"), &feeds, 0).unwrap();
    staged::replace_epg(&mut store, b, &two_feed_guide("other"), &feeds, 0).unwrap();
    // A refresh with only the low feed: hi's rows and k3 (hi only) go.
    let counts =
        staged::replace_epg(&mut store, a, &two_feed_guide("new"), &feeds[1..], 1).unwrap();
    assert_eq!(
        counts.skipped_bindings, 2,
        "k1/k3 were bound to the hi feed"
    );
    let g = store.load_epg(a, &EpgWindow::from(NOW, 1)).unwrap();
    assert_eq!(g.bound_count(), 1);
    assert_eq!(titles(&g, "k2", NOW, NOW + 1), ["new Y0"]);
    assert_eq!(store.epg_guide(a).unwrap().unwrap().feeds.len(), 1);
    let other = store.load_epg(b, &EpgWindow::from(NOW, 1)).unwrap();
    assert_eq!(titles(&other, "k1", NOW, NOW + 1), ["other X0"]);

    // An unknown source is an error and writes nothing.
    assert!(
        staged::replace_epg(&mut store, SourceId(77), &two_feed_guide("x"), &feeds, 0).is_err()
    );
}

#[test]
fn failures_are_recorded_and_cleared() {
    let (mut store, a, _) = store_with_sources();
    store
        .record_epg_failure(a, FailureKind::Network.into())
        .unwrap();
    let s = store.epg_guide(a).unwrap().unwrap();
    assert_eq!(
        (s.replaced_at, s.attempted_at, s.last_error),
        (None, Some(NOW), Some(ImportErrorKind::Network))
    );
    assert!(s.refresh_state().last_attempt_failed);
    let feeds = [meta(config(1, 9))];
    staged::replace_epg(&mut store, a, &two_feed_guide("v1"), &feeds, 0).unwrap();
    store
        .record_epg_failure(a, FailureKind::Parse.into())
        .unwrap();
    let s = store.epg_guide(a).unwrap().unwrap();
    assert_eq!(s.last_error, Some(ImportErrorKind::InvalidBody));
    assert_eq!(s.replaced_at, Some(NOW), "stored guide kept");
    assert_eq!(
        store
            .load_epg(a, &EpgWindow::from(NOW, 1))
            .unwrap()
            .bound_count(),
        2
    );
    staged::replace_epg(&mut store, a, &two_feed_guide("v2"), &feeds, 0).unwrap();
    assert_eq!(store.epg_guide(a).unwrap().unwrap().last_error, None);
}

#[test]
fn prune_delete_and_cascade() {
    let (mut store, a, b) = store_with_sources();
    let feeds = [meta(config(1, 9)), meta(config(2, 1))];
    staged::replace_epg(&mut store, a, &two_feed_guide("a"), &feeds, 0).unwrap();
    staged::replace_epg(&mut store, b, &two_feed_guide("b"), &feeds, 0).unwrap();
    // Ending at or before now: X-3..X-1 and Y-3..Y-1 (y's two-hour
    // programmes were cut at the next start), for both sources.
    assert_eq!(store.prune_epg(NOW).unwrap(), 12);
    let g = store.load_epg(a, &EpgWindow::from(NOW, 30)).unwrap();
    assert_eq!(titles(&g, "k1", 0, i64::MAX).len(), 6);
    assert_eq!(
        store.epg_description(a, &"k1".into(), NOW - H).unwrap(),
        None
    );
    assert!(
        store
            .epg_description(a, &"k1".into(), NOW)
            .unwrap()
            .is_some()
    );

    assert!(store.delete_epg(a).unwrap());
    assert!(!store.delete_epg(a).unwrap());
    assert_eq!(store.epg_guide(a).unwrap(), None);
    assert_eq!(
        store
            .load_epg(a, &EpgWindow::from(NOW, 1))
            .unwrap()
            .bound_count(),
        0
    );
    // Deleting the playlist source deletes its guide.
    assert!(store.delete_source(b).unwrap());
    assert_eq!(store.epg_guide(b).unwrap(), None);
}

/// Start, stop, title, description, season/episode.
type Row = (i64, i64, String, Option<String>, (Option<u16>, Option<u16>));

#[test]
fn every_field_survives_the_store() {
    let (mut store, a, _) = store_with_sources();
    let (guide, _) = epg_synth::import(
        Shape {
            channels: 40,
            days_back: 1,
            days_ahead: 1,
        },
        StoreOptions::default(),
    );
    let feeds = [meta(EpgSourceConfig::new(
        iptv_epg::SourceId(1),
        SecretUrl::new("http://epg.example.com/synthetic.xml.gz"),
    ))];
    let counts = staged::replace_epg(&mut store, a, &guide, &feeds, 0).unwrap();
    assert_eq!(counts.channels, 40);
    let back = store
        .load_epg(
            a,
            &EpgWindow {
                descriptions: true,
                ..EpgWindow::from(NOW, 30)
            },
        )
        .unwrap();
    for n in 0..40 {
        let key = epg_synth::key(n);
        let rows = |g: &GuideStore| -> Vec<Row> {
            g.programmes(&key, i64::MIN, i64::MAX)
                .map(|p| {
                    let d: ProgrammeData<'_> = p.data();
                    (
                        d.start,
                        d.stop.unwrap(),
                        d.title.to_owned(),
                        d.desc.map(str::to_owned),
                        (d.season, d.episode),
                    )
                })
                .collect()
        };
        assert_eq!(rows(&back), rows(&guide), "channel {n}");
    }
    assert!(counts.programmes > 40 * 40);
    // Retention: what the window leaves out.
    let recent = store.load_epg(a, &EpgWindow::from(NOW, 0)).unwrap();
    let first = recent
        .programmes(&epg_synth::key(0), i64::MIN, i64::MAX)
        .next()
        .unwrap();
    assert!(first.stop() > NOW && first.start() <= NOW);
}

#[test]
fn feed_rows_never_show_their_url_in_debug() {
    let (mut store, a, _) = store_with_sources();
    staged::replace_epg(
        &mut store,
        a,
        &two_feed_guide("v1"),
        &[meta(config(1, 9))],
        0,
    )
    .unwrap();
    let shown = format!("{:?}", store.epg_guide(a).unwrap());
    assert!(!shown.contains(common::XT_PASSWORD), "{shown}");
}
