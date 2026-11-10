mod common;

use std::rc::Rc;

use common::synth::{self, SimResponse, StalkerSim};
use common::{FakeFetch, Reply, block_on, connect_error};
use iptv_import::{
    AuthProblem, CancelToken, Catalog, FailureKind, FetchError, FetchErrorKind, ImportError,
    ImportOptions, NoProgress, ParseProblem, SourceMeta, Step,
};
use iptv_providers::StalkerErrorKind;
use iptv_providers::model::{ContentKind, MacAddress, SourceConfig, StalkerCredentials};
use iptv_providers::stalker::PortalMode;

const MAC: &str = "00:1A:79:00:00:42";

fn config() -> SourceConfig {
    SourceConfig::Stalker(StalkerCredentials {
        portal_url: "http://127.0.0.1:8091/stalker_portal/c/".into(),
        mac: MacAddress::parse(MAC).unwrap(),
        serial: None,
        device_id: None,
        device_id2: None,
        signature: None,
        signature2: None,
        login: None,
        password: None,
    })
}

fn fake(sim: StalkerSim) -> (FakeFetch, Rc<StalkerSim>) {
    let sim = Rc::new(sim);
    let s2 = sim.clone();
    (FakeFetch::new(move |_, req| s2.handle(req).into()), sim)
}

fn run_with(fetch: &FakeFetch, opts: ImportOptions) -> Result<Catalog, ImportError> {
    block_on(iptv_import::import_source(
        fetch,
        &config(),
        &opts,
        &mut NoProgress,
        &CancelToken::new(),
    ))
}

fn run(fetch: &FakeFetch) -> Result<Catalog, ImportError> {
    run_with(fetch, ImportOptions::new(synth::NOW))
}

fn session(catalog: &Catalog) -> &iptv_providers::stalker::StalkerSession {
    match &catalog.source {
        SourceMeta::Stalker { session } => session,
        _ => panic!("not stalker"),
    }
}

#[test]
fn full_portal_discovery_auth_genres_and_pages() {
    let (fetch, sim) = fake(StalkerSim::default());
    let catalog = run(&fetch).unwrap();
    assert_eq!(catalog.channels.len(), 35);
    assert_eq!(catalog.groups.len(), 4, "the '*' genre is dropped");
    assert_eq!(catalog.stats.pages, 3);
    assert_eq!(sim.handshakes(), 1);
    let s = session(&catalog);
    assert!(s.is_authorized());
    assert_eq!(s.portal().mode(), PortalMode::Full);
    assert!(
        s.portal()
            .endpoint()
            .ends_with("/stalker_portal/server/load.php")
    );
    // Probe of portal.php (404), probe of load.php, handshake, profile, genres, 3 pages.
    assert_eq!(fetch.requests().len(), 8);
    for req in fetch.requests() {
        let shown = format!("{req:?}");
        assert!(
            !shown.contains(MAC) && !shown.contains(synth::ST_TOKEN),
            "{shown}"
        );
    }
    assert!(!format!("{catalog:?}").contains(synth::ST_TOKEN));
    let ch = &catalog.channels[0];
    assert_eq!(ch.epg_id.as_deref(), Some("ch0.example"));
    assert!(catalog.groups.iter().any(|g| g.id == ch.groups[0]));
}

#[test]
fn token_free_panels_skip_the_handshake() {
    let (fetch, sim) = fake(StalkerSim {
        simple: true,
        api_path: "/stalker_portal/portal.php",
        ..Default::default()
    });
    let catalog = run(&fetch).unwrap();
    assert_eq!(catalog.channels.len(), 35);
    assert_eq!(sim.handshakes(), 0);
    assert_eq!(session(&catalog).portal().mode(), PortalMode::Simple);
}

#[test]
fn token_expiry_mid_crawl_reauthenticates_once() {
    let (fetch, sim) = fake(StalkerSim {
        expire_on_page: Some(2),
        ..Default::default()
    });
    let catalog = run(&fetch).unwrap();
    assert_eq!(catalog.channels.len(), 35);
    assert_eq!(sim.handshakes(), 2);
    assert!(catalog.warnings.is_empty(), "{:?}", catalog.warnings);
}

#[test]
fn a_failing_page_keeps_earlier_pages_with_a_warning() {
    let (fetch, _) = fake(StalkerSim {
        failing_pages: vec![2],
        ..Default::default()
    });
    let catalog = run(&fetch).unwrap();
    assert_eq!(catalog.channels.len(), 14);
    let w = catalog
        .warnings
        .iter()
        .find(|w| w.page == Some(2))
        .expect("warning for the stopped crawl");
    assert_eq!(w.step, Step::Items(ContentKind::Live));
    assert!(w.message.contains("500"), "{}", w.message);
}

#[test]
fn a_failing_first_page_is_an_error_not_empty() {
    let (fetch, _) = fake(StalkerSim {
        failing_pages: vec![1],
        ..Default::default()
    });
    let err = run(&fetch).unwrap_err();
    assert_eq!(
        err,
        ImportError::HttpStatus {
            step: Step::Items(ContentKind::Live),
            status: 500
        }
    );
    // Retried once before failing: 2 probes + handshake + profile + genres + 2 tries.
    assert_eq!(fetch.requests().len(), 7);
}

#[test]
fn portal_refusals_are_auth_failures() {
    for (status, kind) in [
        (1, StalkerErrorKind::Blocked),
        (2, StalkerErrorKind::LoginRequired),
    ] {
        let (fetch, _) = fake(StalkerSim {
            profile_status: status,
            ..Default::default()
        });
        let err = run(&fetch).unwrap_err();
        match &err {
            ImportError::Auth {
                step: Step::Authentication,
                problem: AuthProblem::Stalker(e),
            } => {
                assert_eq!(e.kind, kind);
            }
            other => panic!("{other:?}"),
        }
        if status == 1 {
            assert!(err.to_string().contains("Synthetic account disabled"));
        }
    }
}

#[test]
fn nothing_portal_like_is_not_a_portal() {
    let fetch = FakeFetch::new(|_, _| SimResponse::ok("<html>router login</html>").into());
    let err = run(&fetch).unwrap_err();
    assert_eq!(
        err,
        ImportError::Parse {
            step: Step::PortalDiscovery,
            problem: ParseProblem::NotAPortal
        }
    );
    assert_eq!(fetch.requests().len(), 2, "both candidates probed");
}

#[test]
fn discovery_aborts_on_connection_failure_but_not_on_timeout() {
    let fetch = FakeFetch::new(|_, _| connect_error());
    assert_eq!(run(&fetch).unwrap_err().kind(), FailureKind::Network);
    assert_eq!(fetch.requests().len(), 1);

    let sim = StalkerSim::default();
    let fetch = FakeFetch::new(move |_, req| {
        if req.path.ends_with("/portal.php") {
            Reply::Error(FetchError::new(FetchErrorKind::Timeout, "read timed out"))
        } else {
            sim.handle(req).into()
        }
    });
    assert_eq!(run(&fetch).unwrap().channels.len(), 35);
}

#[test]
fn an_empty_portal_is_empty() {
    let (fetch, _) = fake(StalkerSim {
        channels: 0,
        ..Default::default()
    });
    assert_eq!(
        run(&fetch).unwrap_err(),
        ImportError::Empty {
            step: Step::Items(ContentKind::Live)
        }
    );
}

#[test]
fn vod_behind_the_flag() {
    let (fetch, _) = fake(StalkerSim {
        vod: 30,
        ..Default::default()
    });
    let catalog = run(&fetch).unwrap();
    assert!(catalog.vod.is_empty());
    let (fetch, _) = fake(StalkerSim {
        vod: 30,
        ..Default::default()
    });
    let catalog = run_with(&fetch, ImportOptions::new(synth::NOW).with_vod(true, false)).unwrap();
    assert_eq!(catalog.vod.len(), 30);
    assert!(
        catalog
            .groups
            .iter()
            .any(|g| g.kind == ContentKind::Movie && g.name == "Films")
    );
    assert_eq!(catalog.stats.pages, 3 + 3);
}

#[test]
fn channel_cap_bounds_the_crawl() {
    let (fetch, _) = fake(StalkerSim {
        channels: 100,
        ..Default::default()
    });
    let mut opts = ImportOptions::new(synth::NOW);
    opts.stalker_channel_cap = 20;
    assert_eq!(run_with(&fetch, opts).unwrap().channels.len(), 20);
}
