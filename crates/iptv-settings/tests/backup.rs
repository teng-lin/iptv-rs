//! Backup JSON: round trip, stability, forward/backward compatibility and
//! migrations. All ids and values are synthetic.

use iptv_settings::backup::{FORMAT, VERSION};
use iptv_settings::{Entry, KeyBinding, LoadError, PressKind, Settings, Target, Value, keys};
use serde_json::json;

const G: &Target = &Target::Global;

fn sample() -> Settings {
    let mut st = Settings::new();
    st.set("guide.autoplay", Value::Bool(true)).unwrap();
    st.set("history.recent_count", Value::Int(15)).unwrap();
    st.set("epg.time_offset", Value::ms(-30 * 60_000)).unwrap();
    st.set("appearance.accent", Value::colour("teal")).unwrap();
    st.set("appearance.font_size", Value::choice("large"))
        .unwrap();
    st.set("logos.folder", Value::text("/storage/logos"))
        .unwrap();
    st.set(
        "remote.key_overrides",
        Value::KeyBindings(vec![
            KeyBinding::new(None, "red", PressKind::Short, "show_guide"),
            KeyBinding::new(Some("player"), "ok", PressKind::Long, "show_menu"),
        ]),
    )
    .unwrap();
    st.set_in(
        "channels.group_sort",
        &Target::playlist("pl-a"),
        Value::choice("name"),
    )
    .unwrap();
    st.set_in(
        "general.user_agent",
        &Target::playlist("pl-b"),
        Value::text("SyntheticAgent/1.0"),
    )
    .unwrap();
    st.set_in(
        "channels.channel_sort",
        &Target::group("pl-a", "grp-1"),
        Value::choice("manual"),
    )
    .unwrap();
    st
}

fn load(doc: serde_json::Value) -> (Settings, iptv_settings::LoadReport) {
    Settings::from_json(&doc.to_string()).expect("loads")
}

fn entry(target: Target, key: &str) -> Entry {
    Entry {
        target,
        key: key.to_owned(),
    }
}

#[test]
fn round_trip_preserves_every_value_type_and_level() {
    let st = sample();
    let (back, report) = Settings::from_json(&st.to_json_string()).unwrap();
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(back, st);
    assert_eq!(
        back.choice(keys::CHANNEL_SORT, &Target::group("pl-a", "grp-1")),
        "manual"
    );
}

#[test]
fn serialisation_is_stable_and_minimal() {
    let st = sample();
    let text = st.to_json_string();
    assert_eq!(text, st.clone().to_json_string());
    let (back, _) = Settings::from_json(&text).unwrap();
    assert_eq!(back.to_json_string(), text);

    // Insertion order does not matter.
    let mut other = Settings::new();
    other
        .set_in(
            "channels.channel_sort",
            &Target::group("pl-a", "grp-1"),
            Value::choice("manual"),
        )
        .unwrap();
    other.set("guide.autoplay", Value::Bool(true)).unwrap();
    let mut reverse = Settings::new();
    reverse.set("guide.autoplay", Value::Bool(true)).unwrap();
    reverse
        .set_in(
            "channels.channel_sort",
            &Target::group("pl-a", "grp-1"),
            Value::choice("manual"),
        )
        .unwrap();
    assert_eq!(other.to_json_string(), reverse.to_json_string());

    // Defaults are never written.
    assert_eq!(
        Settings::new().to_json(),
        json!({"format": FORMAT, "version": VERSION, "global": {}, "playlists": {}, "groups": {}})
    );
}

#[test]
fn values_use_the_documented_json_shapes() {
    let doc = sample().to_json();
    assert_eq!(doc["global"]["guide.autoplay"], json!(true));
    assert_eq!(doc["global"]["epg.time_offset"], json!(-1_800_000));
    assert_eq!(doc["global"]["appearance.accent"], json!("teal"));
    assert_eq!(
        doc["global"]["remote.key_overrides"],
        json!([
            {"key": "red", "press": "short", "action": "show_guide"},
            {"context": "player", "key": "ok", "press": "long", "action": "show_menu"}
        ])
    );
    assert_eq!(
        doc["playlists"]["pl-a"]["channels.group_sort"],
        json!("name")
    );
    assert_eq!(
        doc["groups"]["pl-a"]["grp-1"]["channels.channel_sort"],
        json!("manual")
    );
}

#[test]
fn unknown_keys_are_kept_reported_and_written_back() {
    let doc = json!({
        "format": FORMAT,
        "version": VERSION,
        "global": {"guide.autoplay": true, "future.hologram_mode": {"depth": 3}},
        "playlists": {"pl-a": {"future.per_playlist": 1}},
        "groups": {"pl-a": {"grp-1": {"future.per_group": "x"}}},
        "profiles": [{"name": "Profile A"}]
    });
    let (st, report) = load(doc.clone());
    assert!(st.bool(keys::AUTOPLAY, G));
    assert_eq!(
        report.unknown,
        vec![
            entry(Target::Global, "future.hologram_mode"),
            entry(Target::playlist("pl-a"), "future.per_playlist"),
            entry(Target::group("pl-a", "grp-1"), "future.per_group"),
        ]
    );
    assert_eq!(report.unknown_members, ["profiles"]);
    assert_eq!(st.to_json(), doc);

    // Resetting everything drops them.
    let mut st = st;
    st.reset_all();
    assert_eq!(st, Settings::new());
}

#[test]
fn numbers_outside_a_changed_range_are_clamped() {
    let doc = json!({
        "format": FORMAT,
        "version": VERSION,
        "global": {
            "player.info_panel_timeout": 120_000,   // above max (30 s)
            "history.recent_count": 0,              // below min (1)
            "guide.switch_delay": 1_100,            // off the 250 ms grid
            "appearance.ui_transparency": 95        // above max (80)
        }
    });
    let (st, report) = load(doc);
    assert_eq!(st.duration_ms(keys::INFO_PANEL_TIMEOUT, G), 30_000);
    assert_eq!(st.int(keys::RECENT_COUNT, G), 1);
    assert_eq!(st.duration_ms(keys::SWITCH_DELAY, G), 1_000);
    assert_eq!(st.get("appearance.ui_transparency"), Ok(Value::Int(80)));
    assert_eq!(report.clamped.len(), 4);
    assert!(report.rejected.is_empty());
}

#[test]
fn invalid_values_are_rejected_but_kept_until_overwritten() {
    let doc = json!({
        "format": FORMAT,
        "version": VERSION,
        "global": {
            "appearance.font_size": "gigantic",     // variant this build lacks
            "guide.autoplay": "yes",                // wrong JSON type
            "player.info_panel_timeout": 4.5,       // not a whole number
            "general.udp_proxy": "no-port"          // malformed
        },
        "playlists": {"pl-a": {"appearance.accent": "red"}} // global-only key
    });
    let (mut st, report) = load(doc);
    let rejected: Vec<&str> = report
        .rejected
        .iter()
        .map(|(e, _)| e.key.as_str())
        .collect();
    assert_eq!(
        rejected,
        [
            "appearance.font_size",
            "general.udp_proxy",
            "guide.autoplay",
            "player.info_panel_timeout",
            "appearance.accent"
        ]
    );
    // Defaults apply…
    assert_eq!(st.choice(keys::FONT_SIZE, G), "medium");
    assert!(!st.bool(keys::AUTOPLAY, G));
    // …and the raw entries survive a save (a newer build may understand them).
    let saved = st.to_json();
    assert_eq!(saved["global"]["appearance.font_size"], json!("gigantic"));
    assert_eq!(
        saved["playlists"]["pl-a"]["appearance.accent"],
        json!("red")
    );
    // Setting the key replaces the preserved entry.
    st.set("appearance.font_size", Value::choice("small"))
        .unwrap();
    assert_eq!(
        st.to_json()["global"]["appearance.font_size"],
        json!("small")
    );
    // Resetting it drops the preserved entry too.
    st.reset("guide.autoplay").unwrap();
    assert!(st.to_json()["global"].get("guide.autoplay").is_none());
}

#[test]
fn version_1_documents_are_migrated() {
    let v1 = json!({
        "format": FORMAT,
        "version": 1,
        "global": {"player.panel_timeout_secs": 9, "guide.autoplay": true}
    });
    let (st, report) = load(v1);
    assert_eq!(report.version, 1);
    assert_eq!(report.migrations, [1]);
    assert!(report.unknown.is_empty());
    assert_eq!(st.duration_ms(keys::CONTROLS_PANEL_TIMEOUT, G), 9_000);
    assert!(st.bool(keys::AUTOPLAY, G));

    // The result saves as the current version and reloads without
    // migrating again (safe on repeated startup).
    let saved = st.to_json();
    assert_eq!(saved["version"], json!(VERSION));
    assert!(saved["global"].get("player.panel_timeout_secs").is_none());
    let (again, report) = load(saved);
    assert!(report.is_clean());
    assert_eq!(again, st);
}

#[test]
fn migration_keeps_a_value_already_under_the_new_key() {
    let v1 = json!({
        "format": FORMAT,
        "version": 1,
        "global": {"player.panel_timeout_secs": 9, "player.controls_panel_timeout": 5_000}
    });
    let (st, _) = load(v1);
    assert_eq!(st.duration_ms(keys::CONTROLS_PANEL_TIMEOUT, G), 5_000);
}

#[test]
fn migrated_values_are_validated_like_any_other() {
    // 90 s exceeds the controls timeout's 60 s maximum: clamped after migrating.
    let v1 = json!({"format": FORMAT, "version": 1, "global": {"player.panel_timeout_secs": 90}});
    let (st, report) = load(v1);
    assert_eq!(st.duration_ms(keys::CONTROLS_PANEL_TIMEOUT, G), 60_000);
    assert_eq!(
        report.clamped,
        [entry(Target::Global, "player.controls_panel_timeout")]
    );
}

#[test]
fn newer_documents_load_best_effort() {
    let doc = json!({
        "format": FORMAT,
        "version": VERSION + 5,
        "global": {"guide.autoplay": true, "future.key": 1}
    });
    let (st, report) = load(doc);
    assert!(report.newer);
    assert!(report.migrations.is_empty());
    assert!(st.bool(keys::AUTOPLAY, G));
    assert_eq!(report.unknown, [entry(Target::Global, "future.key")]);
}

#[test]
fn malformed_documents_fail_to_load() {
    let cases = [
        ("not json", LoadError::NotADocument(String::new())),
        ("[1, 2]", LoadError::NotADocument(String::new())),
        (r#"{"version": 2}"#, LoadError::WrongFormat),
        (
            r#"{"format": "other.app", "version": 2}"#,
            LoadError::WrongFormat,
        ),
        (r#"{"format": "iptv-rs.settings"}"#, LoadError::BadVersion),
        (
            r#"{"format": "iptv-rs.settings", "version": 0}"#,
            LoadError::BadVersion,
        ),
        (
            r#"{"format": "iptv-rs.settings", "version": "2"}"#,
            LoadError::BadVersion,
        ),
        (
            r#"{"format": "iptv-rs.settings", "version": 2, "global": []}"#,
            LoadError::BadLayer("global".into()),
        ),
        (
            r#"{"format": "iptv-rs.settings", "version": 2, "groups": {"p": {"g": 1}}}"#,
            LoadError::BadLayer("groups.p.g".into()),
        ),
    ];
    for (text, expected) in cases {
        let err = Settings::from_json(text).expect_err(text);
        match (&err, &expected) {
            (LoadError::NotADocument(_), LoadError::NotADocument(_)) => {}
            _ => assert_eq!(err, expected, "{text}"),
        }
    }
}

#[test]
fn a_missing_layer_is_empty() {
    let (st, report) = load(json!({"format": FORMAT, "version": VERSION}));
    assert!(report.is_clean());
    assert_eq!(st, Settings::new());
}
