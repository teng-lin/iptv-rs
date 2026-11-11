//! Synthetic end-to-end editing; all storage is inside a temporary HOME.
use iptv_providers::{
    SecretUrl,
    model::{M3uSourceConfig, Source, SourceConfig},
};
use iptv_store_sqlite::{ChannelQuery, SourcePolicy, Store};
use nsh_core::{Size, element::Root, prelude::*};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use std::time::Duration;

const PLAYLIST: &str = "#EXTM3U\n#EXTINF:-1 group-title=\"News\",Local A\nhttp://stream.example.test/a\n#EXTINF:-1 group-title=\"News\",Local B\nhttp://stream.example.test/b\n#EXTINF:-1 group-title=\"Sport\",Local C\nhttp://stream.example.test/c\n";
fn step(result: Result<(), String>) {
    result.unwrap_or_else(|e| panic!("{e}"))
}
fn rail(app: &mut Headless) {
    step(app.press("text:☰  All channels"));
    step(app.press_key("left"));
    step(app.press_key("left"));
}
#[test]
fn organization_playlist_policies_and_remote_bindings_are_editable_and_persisted() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: this binary has one test, before app threads start.
    unsafe { std::env::set_var("HOME", home.path()) };
    let dir = if cfg!(target_os = "macos") {
        home.path().join("Library/Application Support/iptv-rs")
    } else {
        home.path().join(".local/share/iptv-rs")
    };
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("iptv.sqlite");
    let playlist_path = home.path().join("synthetic.m3u");
    std::fs::write(&playlist_path, PLAYLIST).unwrap();
    let m = iptv_providers::m3u::parse_m3u(PLAYLIST);
    let mut store = Store::open(&path).unwrap();
    let source = store
        .add_source(&Source {
            name: "Synthetic library".into(),
            config: SourceConfig::M3u(M3uSourceConfig {
                location: SecretUrl::new(playlist_path.to_string_lossy()),
                user_agent: None,
                referrer: None,
            }),
        })
        .unwrap();
    store
        .replace_catalog(source, &m.groups, &m.channels.items, &[], &[])
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
    drop(store);
    let mut app = Headless::start(EngineConfig {
        viewport: Size::new(px(1920.0), px(1080.0)),
        ..EngineConfig::new(Root::of::<iptv_app::App>())
    })
    .unwrap();
    step(app.wait_for_within("text:Local A", Duration::from_secs(20)));
    rail(&mut app);
    step(app.press("text:Playlists >> nth=0"));
    step(app.press("text:Synthetic library"));
    // Channel editor is available by remote; hidden items remain in this manager.
    for _ in 0..6 {
        step(app.press_key("down"));
    }
    step(app.press_key("enter"));
    step(app.wait_for("text:Organize channels"));
    step(app.press("id:organization-row-0"));
    step(app.press("text:Hide item"));
    assert!(
        Store::open(&path)
            .unwrap()
            .load_channels(
                source,
                &ChannelQuery {
                    include_hidden: true,
                    ..Default::default()
                }
            )
            .unwrap()[0]
            .hidden
    );
    step(app.press("id:organization-row-0"));
    step(app.press("text:Show item"));
    step(app.press("id:organization-row-2"));
    step(app.press("text:Move to first"));
    assert_eq!(
        Store::open(&path)
            .unwrap()
            .load_channels(source, &ChannelQuery::default())
            .unwrap()[0]
            .name,
        "Local C"
    );
    step(app.back());
    step(app.wait_for_gone("id:organization-screen", Duration::from_secs(2)));
    // Group editor uses stable provider identities and supports recovery too.
    step(app.press_key("down"));
    step(app.press_key("enter"));
    step(app.wait_for("text:Organize groups"));
    step(app.press("id:organization-row-0"));
    step(app.press("text:Hide item"));
    assert!(Store::open(&path).unwrap().library_groups(source).unwrap()[0].hidden);
    step(app.press("id:organization-row-0"));
    step(app.press("text:Show item"));
    step(app.back());
    // Back to actions, refresh interval and startup setting are persisted.
    for _ in 0..3 {
        step(app.press_key("up"));
    }
    step(app.press_key("enter"));
    assert_eq!(
        Store::open(&path)
            .unwrap()
            .source_policy(source)
            .unwrap()
            .refresh_seconds,
        3600
    );
    step(app.press_key("down"));
    step(app.press_key("enter"));
    assert!(
        Store::open(&path)
            .unwrap()
            .source_policy(source)
            .unwrap()
            .refresh_on_start
    );
    step(app.press_key("up"));
    step(app.press_key("up"));
    step(app.press_key("enter"));
    assert!(
        !Store::open(&path)
            .unwrap()
            .source_policy(source)
            .unwrap()
            .enabled
    );
    step(app.press_key("enter"));
    assert!(
        Store::open(&path)
            .unwrap()
            .source_policy(source)
            .unwrap()
            .enabled
    );
    // Show the enabled playlist, open settings via the guide's Menu.
    for _ in 0..3 {
        step(app.press_key("up"));
    }
    step(app.press_key("enter"));
    step(app.wait_for_within("text:Local C", Duration::from_secs(20)));
    rail(&mut app);
    step(app.press("text:Settings >> nth=0"));
    step(app.press("text:Remote control"));
    step(app.press("text:Edit remote bindings"));
    step(app.wait_for("text:Remote bindings"));
    step(app.press("id:remote-Action")); // Toggle favourite -> Nothing.
    step(app.press("id:remote-Save"));
    let json = std::fs::read_to_string(dir.join("settings.json")).unwrap();
    let (settings, _) = iptv_settings::Settings::from_json(&json).unwrap();
    let bindings = settings.key_bindings(
        iptv_settings::keys::KEY_OVERRIDES,
        &iptv_settings::Target::Global,
    );
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].action, "ignore");
    // Replacing the same key requires confirmation; Back keeps the previous value.
    step(app.press("id:remote-Action"));
    step(app.press("id:remote-Save"));
    step(app.wait_for("text:Replace the existing binding?"));
    step(app.back());
    step(app.press("id:remote-Clear"));
    let json = std::fs::read_to_string(dir.join("settings.json")).unwrap();
    let (settings, _) = iptv_settings::Settings::from_json(&json).unwrap();
    assert!(
        settings
            .key_bindings(
                iptv_settings::keys::KEY_OVERRIDES,
                &iptv_settings::Target::Global
            )
            .is_empty()
    );
}
