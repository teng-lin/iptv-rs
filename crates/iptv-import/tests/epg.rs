mod common;

use common::synth::{self, NOW, SimResponse};
use common::{Events, FakeFetch, Reply, block_on};
use iptv_epg::{ChannelKey, Override, Overrides, SourceId, StoreOptions};
use iptv_import::{
    CancelToken, EpgImport, EpgSourceConfig, FailureKind, ImportError, NoProgress, ParseProblem,
    Phase, Step, import_epg, import_m3u_text,
};
use iptv_providers::SecretUrl;
use iptv_providers::model::Channel;

const HOUR: i64 = 3600;

fn channels(n: usize) -> Vec<Channel> {
    let text = synth::m3u(
        n,
        4,
        Some("http://127.0.0.1:8089/iptv/guide.xml.gz"),
        "http://127.0.0.1:8089",
    );
    import_m3u_text(text.as_bytes(), &mut NoProgress, &CancelToken::new())
        .unwrap()
        .channels
}

fn source(id: u32, path: &str) -> EpgSourceConfig {
    EpgSourceConfig::new(
        SourceId(id),
        SecretUrl::new(format!("http://127.0.0.1:8089{path}")),
    )
}

fn run(
    fetch: &FakeFetch,
    sources: &[EpgSourceConfig],
    chans: &[Channel],
) -> Result<EpgImport, ImportError> {
    block_on(import_epg(
        fetch,
        sources,
        chans,
        &Overrides::new(),
        NOW,
        &mut NoProgress,
        &CancelToken::new(),
    ))
}

fn key(c: &Channel) -> ChannelKey {
    ChannelKey::new(c.id.as_str())
}

#[test]
fn header_url_tvg_guide_matches_by_tvg_id() {
    let chans = channels(200);
    let text = synth::m3u(
        200,
        4,
        Some("http://127.0.0.1:8089/iptv/guide.xml.gz"),
        "http://127.0.0.1:8089",
    );
    let catalog = import_m3u_text(text.as_bytes(), &mut NoProgress, &CancelToken::new()).unwrap();
    let sources = catalog.epg_sources(1);
    assert_eq!(sources.len(), 1);

    let guide = synth::gzip(
        synth::xmltv(
            &synth::m3u_epg_channels(200),
            NOW - 7 * 24 * HOUR,
            NOW + 2 * 24 * HOUR,
            HOUR,
        )
        .as_bytes(),
    );
    let fetch = FakeFetch::body(guide);
    let mut events = Events::default();
    let epg = block_on(import_epg(
        &fetch,
        &sources,
        &chans,
        &Overrides::new(),
        NOW,
        &mut events,
        &CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(epg.matches.channels, 200);
    assert_eq!(epg.matches.by_tvg_id, 200);
    assert_eq!(epg.guide.bound_count(), 200);
    let stats = epg.sources[0].result.as_ref().unwrap();
    assert_eq!(stats.parse.programmes, 200 * 9 * 24);
    let now = epg.guide.programme_at(&key(&chans[7]), NOW + 60).unwrap();
    assert_eq!(now.start(), NOW);
    assert_eq!(now.title(), format!("Show 7-{}", 7 * 24));
    assert!(
        events
            .0
            .iter()
            .any(|p| p.phase == Phase::Guide(SourceId(1)) && p.items > 0)
    );
    assert!(events.0.iter().any(|p| p.phase == Phase::Matching));
}

#[test]
fn priorities_names_overrides_and_partial_failure() {
    let chans = channels(10);
    // Source 1 (low priority): ids for all ten. Source 2 (high): names only
    // for the first five, under different ids. Source 3: 404.
    let low = synth::xmltv(&synth::m3u_epg_channels(10), NOW - HOUR, NOW + HOUR, HOUR);
    let high_channels: Vec<(String, String)> = (0..5)
        .map(|i| (format!("hi-{i}"), format!("Synthetic {i}")))
        .collect();
    let high = synth::xmltv(&high_channels, NOW - HOUR, NOW + HOUR, HOUR / 2);
    let fetch = FakeFetch::new(move |_, req| match req.path.as_str() {
        "/low.xml" => SimResponse::ok(low.clone()).into(),
        "/high.xml" => SimResponse::ok(high.clone()).into(),
        _ => SimResponse::status(404).into(),
    });
    let sources = [
        source(1, "/low.xml").with_priority(1),
        source(2, "/high.xml").with_priority(5),
        source(3, "/missing.xml"),
    ];
    let mut overrides = Overrides::new();
    overrides.insert(key(&chans[9]), Override::NoEpg);
    overrides.insert(
        key(&chans[8]),
        Override::Epg {
            source: Some(SourceId(1)),
            epg_id: "ch2.example".into(),
        },
    );
    let epg = block_on(import_epg(
        &fetch,
        &sources,
        &chans,
        &overrides,
        NOW,
        &mut NoProgress,
        &CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(epg.sources.len(), 3);
    assert_eq!(
        epg.sources[2].result,
        Err(ImportError::HttpStatus {
            step: Step::Guide(SourceId(3)),
            status: 404
        })
    );
    // Channel 0: high-priority source by name wins over tvg-id in the low one.
    assert_eq!(
        epg.guide.binding(&key(&chans[0])),
        Some((SourceId(2), "hi-0"))
    );
    assert_eq!(
        epg.guide.binding(&key(&chans[6])),
        Some((SourceId(1), "ch6.example"))
    );
    assert_eq!(
        epg.guide.binding(&key(&chans[8])),
        Some((SourceId(1), "ch2.example"))
    );
    assert_eq!(epg.guide.binding(&key(&chans[9])), None);
    assert_eq!(epg.matches.disabled, 1);
    assert_eq!(epg.matches.by_override, 1);
}

#[test]
fn every_source_failing_is_an_error() {
    let chans = channels(3);
    let fetch = FakeFetch::new(|_, req| match req.path.as_str() {
        "/html" => SimResponse::ok("<!doctype html><p>nope</p>").into(),
        "/empty" => SimResponse::ok("").into(),
        _ => SimResponse::ok(vec![0x1f, 0x8b, 8, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9]).into(),
    });
    let err = run(&fetch, &[source(1, "/html")], &chans).unwrap_err();
    assert_eq!(
        err,
        ImportError::Parse {
            step: Step::Guide(SourceId(1)),
            problem: ParseProblem::NotXmltv
        }
    );
    let err = run(&fetch, &[source(1, "/empty")], &chans).unwrap_err();
    assert_eq!(err.kind(), FailureKind::Empty);
    let err = run(&fetch, &[source(1, "/gz")], &chans).unwrap_err();
    assert!(
        matches!(
            err,
            ImportError::Parse {
                problem: ParseProblem::Decode(_),
                ..
            }
        ),
        "{err:?}"
    );
    let err = run(&fetch, &[source(4, "/html"), source(5, "/empty")], &chans).unwrap_err();
    assert_eq!(
        err.step(),
        Some(Step::Guide(SourceId(4))),
        "first failure reported"
    );
}

#[test]
fn no_sources_is_an_empty_guide_not_an_error() {
    let epg = run(&FakeFetch::body(""), &[], &channels(5)).unwrap();
    assert_eq!(epg.guide.bound_count(), 0);
    assert_eq!(epg.matches.unmatched, 5);
}

#[test]
fn retention_drops_old_programmes() {
    let chans = channels(2);
    let body = synth::xmltv(
        &synth::m3u_epg_channels(2),
        NOW - 7 * 24 * HOUR,
        NOW + 24 * HOUR,
        HOUR,
    );
    let mut cfg = source(1, "/g.xml");
    cfg.store = StoreOptions {
        store_descriptions: false,
        past_days_to_keep: Some(1),
    };
    let epg = run(&FakeFetch::body(body), &[cfg], &chans).unwrap();
    let ingest = epg.sources[0].result.as_ref().unwrap().ingest;
    assert_eq!(ingest.kept, 2 * 48);
    assert!(ingest.dropped_retention > 0);
    let p = epg.guide.programme_at(&key(&chans[0]), NOW).unwrap();
    assert_eq!(p.desc(), None);
}

#[test]
fn cancellation_stops_the_guide() {
    let chans = channels(50);
    let body = synth::xmltv(&synth::m3u_epg_channels(50), NOW - 7 * 24 * HOUR, NOW, HOUR);
    let fetch = FakeFetch::body(body);
    let cancel = CancelToken::new();
    let c = cancel.clone();
    let mut on = |p: &iptv_import::Progress| {
        if p.bytes > 0 {
            c.cancel();
        }
    };
    let err = block_on(import_epg(
        &fetch,
        &[source(1, "/g")],
        &chans,
        &Overrides::new(),
        NOW,
        &mut on,
        &cancel,
    ));
    assert_eq!(err.unwrap_err(), ImportError::Cancelled);

    let cancel = CancelToken::new();
    let c = cancel.clone();
    let fetch = FakeFetch::new(move |_, _| Reply::Hang(Some(c.clone())));
    let err = block_on(import_epg(
        &fetch,
        &[source(1, "/g")],
        &chans,
        &Overrides::new(),
        NOW,
        &mut NoProgress,
        &cancel,
    ));
    assert_eq!(err.unwrap_err(), ImportError::Cancelled);
}

#[test]
fn source_user_agent_is_sent() {
    let fetch = FakeFetch::body(synth::xmltv(
        &synth::m3u_epg_channels(1),
        NOW,
        NOW + HOUR,
        HOUR,
    ));
    let mut cfg = source(1, "/g.xml");
    cfg.user_agent = Some("SyntheticEpg/2".into());
    run(&fetch, &[cfg], &channels(1)).unwrap();
    assert_eq!(
        fetch.requests()[0].header("User-Agent"),
        Some("SyntheticEpg/2")
    );
}
