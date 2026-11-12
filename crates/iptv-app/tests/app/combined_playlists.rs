//! Combined browsing keeps identical provider keys distinct and routes writes
//! and playback to their owning source. Only generated, local playlists.
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use iptv_providers::{
    SecretUrl,
    model::{M3uSourceConfig, Source, SourceConfig},
};
use iptv_settings::{Settings, Value};
use iptv_store_sqlite::{ChannelQuery, GuideSource, SourceId, SourcePolicy, Store};
use nsh_core::{MountItem, Size, element::Root, prelude::*};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;

#[track_caller]
fn step(result: Result<(), String>) {
    result.unwrap_or_else(|error| panic!("{error}"));
}

fn seed(store: &mut Store, home: &Path, name: &str) -> SourceId {
    let playlist = format!(
        "#EXTM3U\n#EXTINF:-1 tvg-id=\"shared\" group-title=\"News\",Shared\nhttp://stream.example.test/shared\n#EXTINF:-1 group-title=\"News\",Only {name}\nhttp://stream.example.test/{name}\n"
    );
    let file = home.join(format!("{name}.m3u"));
    std::fs::write(&file, &playlist).unwrap();
    let source = store
        .add_source(&Source {
            name: name.into(),
            config: SourceConfig::M3u(M3uSourceConfig {
                location: SecretUrl::new(file.to_string_lossy()),
                user_agent: None,
                referrer: None,
            }),
        })
        .unwrap();
    let parsed = iptv_providers::m3u::parse_m3u(&playlist);
    store
        .replace_catalog(source, &parsed.groups, &parsed.channels.items, &[], &[])
        .unwrap();
    store
        .set_source_policy(
            source,
            SourcePolicy {
                refresh_on_start: false,
                ..Default::default()
            },
        )
        .unwrap();
    // The same feed/channel IDs deliberately occur in both source namespaces.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let guide = home.join(format!("{name}.xml"));
    std::fs::write(&guide, format!(
        "<tv><channel id=\"shared\"><display-name>Shared</display-name></channel><programme channel=\"shared\" start=\"{}\" stop=\"{}\"><title>{name} bulletin</title><desc>{name} synthetic programme details.</desc></programme></tv>",
        iptv_epg::format_xmltv_time(now - 1800), iptv_epg::format_xmltv_time(now + 1800)
    )).unwrap();
    store
        .set_guide_sources(
            source,
            &[GuideSource {
                feed: iptv_epg::SourceId(1001),
                provider_index: None,
                url: SecretUrl::new(guide.to_string_lossy()),
                name: Some(format!("{name} guide")),
                enabled: true,
                shift_minutes: 0,
            }],
        )
        .unwrap();
    source
}

fn playlists(app: &mut Headless) {
    step(app.wait_for("text:/^☰  /"));
    step(app.press("text:/^☰  /"));
    step(app.press_key("left"));
    step(app.press_key("left"));
    step(app.press("text:Playlists >> nth=0"));
}

fn favorite_names(path: &Path, source: SourceId) -> Vec<String> {
    Store::open(path)
        .unwrap()
        .load_channels(
            source,
            &ChannelQuery {
                favourites_only: true,
                ..Default::default()
            },
        )
        .unwrap()
        .into_iter()
        .map(|row| row.name)
        .collect()
}

#[test]
fn combined_browsing_routes_duplicate_keys_favorites_search_and_playback() {
    let Some(home) = isolated!() else { return };
    let dir = if cfg!(target_os = "macos") {
        home.path().join("Library/Application Support/iptv-rs")
    } else {
        home.path().join(".local/share/iptv-rs")
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut settings = Settings::new();
    settings
        .set("guide.stay_on_guide", Value::Bool(true))
        .unwrap();
    settings.set("guide.switch_delay", Value::secs(0)).unwrap();
    settings
        .set("general.resume_last_channel", Value::Bool(false))
        .unwrap();
    std::fs::write(dir.join("settings.json"), settings.to_json_string()).unwrap();
    let path = dir.join("iptv.sqlite");
    let mut store = Store::open(&path).unwrap();
    let alpha = seed(&mut store, home.path(), "Alpha");
    let beta = seed(&mut store, home.path(), "Beta");
    store.set_active_source(alpha).unwrap();
    drop(store);

    let mut app = Headless::start(EngineConfig {
        viewport: Size::new(px(1920.0), px(1080.0)),
        ..EngineConfig::new(Root::of::<iptv_app::App>())
    })
    .unwrap();
    step(app.wait_for_within("text:Only Alpha", Duration::from_secs(20)));
    playlists(&mut app);
    step(app.press("id:playlists-all"));
    step(app.wait_for_within("text:/^Shared · Alpha/", Duration::from_secs(20)));
    step(app.wait_for("text:/^Shared · Beta/"));
    step(app.wait_for_within("text:Alpha bulletin", Duration::from_secs(20)));
    step(app.wait_for_within("text:Beta bulletin", Duration::from_secs(20)));
    assert_eq!(app.screen().count("id:guide-channel-name").unwrap(), 4);

    // These shared rows have the same original provider key. Editing Beta
    // must not change Alpha's membership.
    step(app.press("text:/^Shared · Beta/"));
    step(app.press_key("m"));
    step(app.press("text:Add to favourites"));
    step(app.wait_until(
        "owning source favorite saved",
        Duration::from_secs(5),
        |_| favorite_names(&path, beta) == ["Shared"],
    ));
    assert!(favorite_names(&path, alpha).is_empty());

    let opened: Arc<Mutex<Vec<String>>> = Arc::default();
    let observed = opened.clone();
    app.observe(move |_, batch| {
        for item in &batch.items {
            if let MountItem::ComponentProps { props, .. } = item {
                for (key, value) in props {
                    if key.as_ref() == "source"
                        && let Some(url) = value.as_ref().and_then(|v| v["url"].as_str())
                    {
                        observed.lock().unwrap().push(url.into());
                    }
                }
            }
        }
    });
    step(app.press("text:/^Only Beta/"));
    step(
        app.wait_until("Beta stream opened", Duration::from_secs(5), |_| {
            opened
                .lock()
                .unwrap()
                .iter()
                .any(|url| url == "http://stream.example.test/Beta")
        }),
    );

    step(app.press_key("/"));
    step(app.wait_for("id:search-field"));
    step(app.type_text("id:search-field", "shared"));
    step(app.wait_for_within("text:/Shared · Alpha/", Duration::from_secs(10)));
    step(app.wait_for("text:/Shared · Beta/"));
    step(app.clear("id:search-field"));
    step(app.type_text("id:search-field", "bulletin"));
    step(app.press("text:/^Programmes/"));
    step(app.wait_for_within("text:/Alpha bulletin/", Duration::from_secs(10)));
    step(app.wait_for("text:/Beta bulletin/"));
    step(app.press("text:Close"));

    // Switching back to a source restores its ordinary names and does not
    // leak the other source's favorite, then All playlists remains available.
    playlists(&mut app);
    step(app.press("text:Alpha"));
    step(app.press("text:Show this playlist"));
    step(app.wait_for_within("text:Only Alpha", Duration::from_secs(20)));
    assert!(!app.screen().matches("text:/^Only Beta/").unwrap());
    assert!(!app.screen().matches("text:/^★ Shared/").unwrap());

    playlists(&mut app);
    step(app.press("id:playlists-all"));
    step(app.wait_for_within("text:/^Only Beta/", Duration::from_secs(20)));
    assert!(Store::open(&path).unwrap().all_playlists().unwrap());
    playlists(&mut app);
    step(app.press("text:Beta"));
    step(app.press("text:Playlist enabled: Yes"));
    assert!(
        !Store::open(&path)
            .unwrap()
            .source_policy(beta)
            .unwrap()
            .enabled
    );
    step(app.back());
    step(app.back());
    step(app.wait_for_gone("text:/^Only Beta/", Duration::from_secs(10)));
    step(app.wait_for("text:/^Only Alpha/"));
    assert_eq!(app.screen().count("id:guide-channel-name").unwrap(), 2);
    // Disabling keeps the underlying configuration and membership available
    // for re-enabling; the combined-mode choice stays persisted.
    assert_eq!(favorite_names(&path, beta), ["Shared"]);
    assert!(Store::open(&path).unwrap().all_playlists().unwrap());
}

#[test]
fn persisted_combined_mode_recovers_disabled_and_failed_sources_and_deletion() {
    let Some(home) = isolated!() else { return };
    let dir = if cfg!(target_os = "macos") {
        home.path().join("Library/Application Support/iptv-rs")
    } else {
        home.path().join(".local/share/iptv-rs")
    };
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("iptv.sqlite");
    let mut store = Store::open(&path).unwrap();
    let source = seed(&mut store, home.path(), "Alpha");
    store.set_all_playlists(true).unwrap();
    store
        .set_source_policy(
            source,
            SourcePolicy {
                enabled: false,
                ..Default::default()
            },
        )
        .unwrap();
    drop(store);
    // The stored catalog remains usable even when a later refresh cannot
    // read the generated file. No network server or real source is needed.
    std::fs::remove_file(home.path().join("Alpha.m3u")).unwrap();
    let mut app = Headless::start(EngineConfig {
        viewport: Size::new(px(1920.0), px(1080.0)),
        ..EngineConfig::new(Root::of::<iptv_app::App>())
    })
    .unwrap();
    step(
        app.wait_until("empty aggregate startup", Duration::from_secs(15), |app| {
            app.count("id:guide-channel-name").unwrap() == 0
        }),
    );
    playlists(&mut app);
    step(app.press("text:Alpha"));
    step(app.press("text:Playlist enabled: No"));
    step(app.back());
    step(app.back());
    step(app.wait_for_within("text:/^Only Alpha · Alpha/", Duration::from_secs(15)));
    step(app.wait_until(
        "failed refresh recorded with cache retained",
        Duration::from_secs(15),
        |_| {
            Store::open(&path)
                .unwrap()
                .import_status(source)
                .unwrap()
                .last_error
                .is_some()
        },
    ));
    assert_eq!(app.screen().count("id:guide-channel-name").unwrap(), 2);
    playlists(&mut app);
    step(app.press("text:Alpha"));
    step(app.press("text:Delete"));
    step(app.press("text:Delete >> nth=0"));
    step(app.back());
    step(app.wait_until(
        "deleted source removed from aggregate",
        Duration::from_secs(10),
        |app| app.count("id:guide-channel-name").unwrap() == 0,
    ));
    assert!(Store::open(&path).unwrap().sources().unwrap().is_empty());
    assert!(Store::open(&path).unwrap().all_playlists().unwrap());
    playlists(&mut app);
    step(app.wait_for("text:+ Add playlist"));
}

#[test]
fn leaving_combined_mode_cancels_a_missing_startup_channel_before_manual_return() {
    let Some(home) = isolated!() else { return };
    let dir = home.path().join(if cfg!(target_os = "macos") {
        "Library/Application Support/iptv-rs"
    } else {
        ".local/share/iptv-rs"
    });
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("iptv.sqlite");
    let mut store = Store::open(&path).unwrap();
    let alpha = seed(&mut store, home.path(), "Alpha");
    let beta = seed(&mut store, home.path(), "Beta");
    store.set_all_playlists(true).unwrap();
    store
        .set_combined_last_channel("source:99999:missing-generated-channel")
        .unwrap();
    drop(store);
    let mut app = Headless::start(EngineConfig {
        viewport: Size::new(px(1920.0), px(1080.0)),
        ..EngineConfig::new(Root::of::<iptv_app::App>())
    })
    .unwrap();
    step(app.wait_for_within("text:/^Only Beta · Beta/", Duration::from_secs(20)));
    playlists(&mut app);
    step(app.press("text:Alpha"));
    step(app.press("text:Show this playlist"));
    step(app.wait_for_within("text:Only Alpha", Duration::from_secs(20)));
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.active_source().unwrap(), Some(alpha));
    let channel = store.load_channels(beta, &ChannelQuery::default()).unwrap()[0]
        .channel
        .id
        .clone();
    store
        .set_combined_last_channel(&format!("source:{}:{}", beta.0, channel.as_str()))
        .unwrap();
    drop(store);
    let opened: Arc<Mutex<Vec<String>>> = Arc::default();
    let observed = opened.clone();
    app.observe(move |_, batch| {
        for item in &batch.items {
            if let MountItem::ComponentProps { props, .. } = item {
                for (key, value) in props {
                    if key.as_ref() == "source"
                        && let Some(url) = value.as_ref().and_then(|v| v["url"].as_str())
                    {
                        observed.lock().unwrap().push(url.into());
                    }
                }
            }
        }
    });
    playlists(&mut app);
    step(app.press("id:playlists-all"));
    step(app.wait_for_within("text:/^Only Beta · Beta/", Duration::from_secs(20)));
    step(app.wait_for_within("text:Beta bulletin", Duration::from_secs(20)));
    assert!(
        opened.lock().unwrap().is_empty(),
        "manual All playlists entry must not resume the old startup request"
    );
}
