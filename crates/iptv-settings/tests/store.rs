//! The settings store: validation, scoped resolution, resets, changes and
//! dependencies.

use iptv_settings::{
    Change, KeyBinding, PressKind, Section, Settings, SettingsError, Target, Value, keys,
};

const G: &Target = &Target::Global;

/// Recognises the expected error variant.
type Expect = fn(&SettingsError) -> bool;

fn change(key: &'static str, target: Target) -> Change {
    Change { key, target }
}

#[test]
fn unset_keys_resolve_to_defaults() {
    let st = Settings::new();
    assert_eq!(st.get("guide.autoplay"), Ok(Value::Bool(false)));
    assert_eq!(st.get("guide.switch_delay"), Ok(Value::ms(500)));
    assert_eq!(st.get("appearance.font_size"), Ok(Value::choice("medium")));
    assert_eq!(
        st.get("remote.key_overrides"),
        Ok(Value::KeyBindings(vec![]))
    );
    assert_eq!(st.source("guide.autoplay", G), Ok(None));
}

#[test]
fn set_reports_a_change_only_when_the_value_changes() {
    let mut st = Settings::new();
    let key = "player.info_panel_timeout";
    assert_eq!(
        st.set(key, Value::secs(6)),
        Ok(vec![change(key, Target::Global)])
    );
    assert_eq!(st.duration_ms(keys::INFO_PANEL_TIMEOUT, G), 6_000);
    assert_eq!(st.set(key, Value::secs(6)), Ok(vec![]));
    // Setting the default explicitly is a stored choice but no change.
    assert_eq!(st.set("guide.autoplay", Value::Bool(false)), Ok(vec![]));
    assert_eq!(st.stored("guide.autoplay", G), Some(&Value::Bool(false)));
}

#[test]
fn group_overrides_playlist_overrides_global() {
    let mut st = Settings::new();
    let key = "channels.channel_sort";
    let pl = Target::playlist("pl-1");
    let news = Target::group("pl-1", "news");
    let films = Target::group("pl-1", "films");
    let other = Target::group("pl-2", "news");

    st.set(key, Value::choice("name")).unwrap();
    assert_eq!(st.choice(keys::CHANNEL_SORT, &news), "name");

    st.set_in(key, &pl, Value::choice("watch_time")).unwrap();
    st.set_in(key, &news, Value::choice("manual")).unwrap();
    assert_eq!(st.choice(keys::CHANNEL_SORT, &news), "manual");
    assert_eq!(st.choice(keys::CHANNEL_SORT, &films), "watch_time");
    assert_eq!(st.choice(keys::CHANNEL_SORT, &pl), "watch_time");
    assert_eq!(st.choice(keys::CHANNEL_SORT, &other), "name");
    assert_eq!(st.choice(keys::CHANNEL_SORT, G), "name");

    assert_eq!(st.source(key, &news), Ok(Some(news.clone())));
    assert_eq!(st.source(key, &films), Ok(Some(pl.clone())));
    assert_eq!(st.source(key, &other), Ok(Some(Target::Global)));

    // Clearing an override inherits again.
    assert_eq!(st.clear_in(key, &news), Ok(vec![change(key, news.clone())]));
    assert_eq!(st.choice(keys::CHANNEL_SORT, &news), "watch_time");
}

#[test]
fn levels_beyond_a_settings_scope_are_refused_and_skipped() {
    let mut st = Settings::new();
    // Global-only.
    let err = st.set_in(
        "appearance.accent",
        &Target::playlist("p"),
        Value::colour("red"),
    );
    assert!(matches!(err, Err(SettingsError::ScopeNotAllowed { .. })));
    // Playlist scope: no group overrides.
    let err = st.set_in(
        "channels.group_sort",
        &Target::group("p", "g"),
        Value::choice("name"),
    );
    assert!(matches!(err, Err(SettingsError::ScopeNotAllowed { .. })));
    // Reading at a deeper level falls through to the levels that apply.
    st.set_in(
        "channels.group_sort",
        &Target::playlist("p"),
        Value::choice("name"),
    )
    .unwrap();
    assert_eq!(
        st.choice(keys::GROUP_SORT, &Target::group("p", "g")),
        "name"
    );
    st.set("appearance.accent", Value::colour("teal")).unwrap();
    assert_eq!(st.colour(keys::ACCENT, &Target::group("p", "g")), "teal");
}

#[test]
fn writes_are_validated_and_failed_writes_store_nothing() {
    let mut st = Settings::new();
    let before = st.clone();
    let cases: Vec<(&str, Value, Expect)> = vec![
        ("no.such_key", Value::Bool(true), |e| {
            matches!(e, SettingsError::UnknownKey(_))
        }),
        ("guide.autoplay", Value::Int(1), |e| {
            matches!(e, SettingsError::TypeMismatch { .. })
        }),
        ("guide.switch_delay", Value::Int(500), |e| {
            matches!(e, SettingsError::TypeMismatch { .. })
        }),
        ("player.info_panel_timeout", Value::secs(31), |e| {
            matches!(e, SettingsError::OutOfRange { .. })
        }),
        ("player.info_panel_timeout", Value::ms(0), |e| {
            matches!(e, SettingsError::OutOfRange { .. })
        }),
        ("guide.switch_delay", Value::ms(300), |e| {
            matches!(e, SettingsError::OffStep { .. })
        }),
        ("appearance.ui_transparency", Value::Int(7), |e| {
            matches!(e, SettingsError::OffStep { .. })
        }),
        ("appearance.font_size", Value::choice("huge"), |e| {
            matches!(e, SettingsError::UnknownVariant { .. })
        }),
        ("appearance.accent", Value::colour("dark"), |e| {
            matches!(e, SettingsError::UnknownVariant { .. })
        }),
        ("appearance.accent", Value::choice("blue"), |e| {
            matches!(e, SettingsError::TypeMismatch { .. })
        }),
        ("general.user_agent", Value::Text("x".repeat(513)), |e| {
            matches!(e, SettingsError::TooLong { .. })
        }),
        ("general.user_agent", Value::text("a\nb"), |e| {
            matches!(e, SettingsError::InvalidFormat { .. })
        }),
        (
            "general.udp_proxy",
            Value::text("proxy.example.test"),
            |e| matches!(e, SettingsError::InvalidFormat { .. }),
        ),
        (
            "general.udp_proxy",
            Value::text("proxy.example.test:0"),
            |e| matches!(e, SettingsError::InvalidFormat { .. }),
        ),
        ("general.language", Value::text("English"), |e| {
            matches!(e, SettingsError::InvalidFormat { .. })
        }),
    ];
    for (key, value, expected) in cases {
        let err = st.set(key, value.clone()).expect_err(key);
        assert!(expected(&err), "{key} {value:?}: {err}");
    }
    assert_eq!(st, before);

    st.set("general.udp_proxy", Value::text("192.0.2.10:4022"))
        .unwrap();
    st.set("general.udp_proxy", Value::text("")).unwrap();
    st.set("general.language", Value::text("pt-BR")).unwrap();
    st.set("epg.time_offset", Value::ms(-90 * 60_000)).unwrap();
}

#[test]
fn key_binding_lists_are_validated() {
    let mut st = Settings::new();
    let ok = vec![
        KeyBinding::new(None, "red", PressKind::Short, "show_guide"),
        KeyBinding::new(Some("player"), "ok", PressKind::Long, "show_menu"),
        KeyBinding::new(Some("guide"), "ok", PressKind::Long, "ignore"),
    ];
    st.set("remote.key_overrides", Value::KeyBindings(ok.clone()))
        .unwrap();
    assert_eq!(st.key_bindings(keys::KEY_OVERRIDES, G), ok);

    let dup = vec![
        KeyBinding::new(None, "red", PressKind::Short, "show_guide"),
        KeyBinding::new(None, "red", PressKind::Short, "record"),
    ];
    let bad_token = vec![KeyBinding::new(
        None,
        "Red Key",
        PressKind::Short,
        "show_guide",
    )];
    for list in [dup, bad_token] {
        let err = st.set("remote.key_overrides", Value::KeyBindings(list));
        assert!(
            matches!(err, Err(SettingsError::InvalidFormat { .. })),
            "{err:?}"
        );
    }
    assert_eq!(st.key_bindings(keys::KEY_OVERRIDES, G), ok);
}

#[test]
fn reset_one_key_clears_every_level() {
    let mut st = Settings::new();
    let key = "channels.channel_sort";
    let pl = Target::playlist("p");
    let g = Target::group("p", "g");
    st.set(key, Value::choice("name")).unwrap();
    st.set_in(key, &pl, Value::choice("manual")).unwrap();
    st.set_in(key, &g, Value::choice("playlist_order")).unwrap(); // equals the default
    let changes = st.reset(key).unwrap();
    // The group's value was already the default: no change there.
    assert_eq!(
        changes,
        vec![change(key, Target::Global), change(key, pl.clone())]
    );
    assert_eq!(st.source(key, &g), Ok(None));
    assert_eq!(st, Settings::new());
}

#[test]
fn reset_section_touches_only_that_section() {
    let mut st = Settings::new();
    st.set("player.clock", Value::Bool(false)).unwrap();
    st.set("player.clock_size", Value::choice("large")).unwrap();
    st.set("guide.autoplay", Value::Bool(true)).unwrap();
    let mut changed: Vec<&str> = st
        .reset_section(Section::Player)
        .iter()
        .map(|c| c.key)
        .collect();
    changed.sort();
    assert_eq!(changed, ["player.clock", "player.clock_size"]);
    assert_eq!(st.get("player.clock"), Ok(Value::Bool(true)));
    assert!(st.bool(keys::AUTOPLAY, G));
}

#[test]
fn reset_all_restores_a_fresh_store() {
    let mut st = Settings::new();
    st.set("guide.autoplay", Value::Bool(true)).unwrap();
    st.set_in("logos.source", &Target::playlist("p"), Value::choice("epg"))
        .unwrap();
    assert_eq!(st.reset_all().len(), 2);
    assert_eq!(st, Settings::new());
}

#[test]
fn removing_a_playlist_drops_its_overrides_and_its_groups() {
    let mut st = Settings::new();
    let key = "channels.channel_sort";
    st.set_in(key, &Target::playlist("p"), Value::choice("name"))
        .unwrap();
    st.set_in(key, &Target::group("p", "g"), Value::choice("manual"))
        .unwrap();
    st.set_in(key, &Target::group("q", "g"), Value::choice("manual"))
        .unwrap();
    st.remove_playlist("p");
    assert_eq!(st.source(key, &Target::group("p", "g")), Ok(None));
    assert_eq!(
        st.choice(keys::CHANNEL_SORT, &Target::group("q", "g")),
        "manual"
    );
    st.remove_group("q", "g");
    assert_eq!(st, Settings::new());
}

#[test]
fn visibility_follows_bool_enum_and_text_conditions() {
    let mut st = Settings::new();
    assert_eq!(st.is_visible("general.confirm_exit_window", G), Ok(true));
    st.set("general.confirm_exit", Value::Bool(false)).unwrap();
    assert_eq!(st.is_visible("general.confirm_exit_window", G), Ok(false));

    assert_eq!(st.is_visible("guide.stay_on_guide_overlay", G), Ok(false));
    st.set("guide.browse_mode", Value::choice("overlay"))
        .unwrap();
    assert_eq!(st.is_visible("guide.stay_on_guide_overlay", G), Ok(true));

    assert_eq!(st.is_visible("logos.inexact_match", G), Ok(false));
    st.set("logos.folder", Value::text("/storage/logos"))
        .unwrap();
    assert_eq!(st.is_visible("logos.inexact_match", G), Ok(true));
}

#[test]
fn any_of_conditions_and_enabled_chains() {
    let mut st = Settings::new();
    let pin = "parental.unlock_window";
    assert_eq!(st.is_visible(pin, G), Ok(false));
    st.set("parental.vod_enabled", Value::Bool(true)).unwrap();
    assert_eq!(st.is_visible(pin, G), Ok(true));

    // Enabled, not hidden: visible but locked until AFR is on.
    let only = "playback.afr_50_60_only";
    assert_eq!(st.is_visible(only, G), Ok(true));
    assert_eq!(st.is_enabled(only, G), Ok(false));
    st.set("playback.afr", Value::Bool(true)).unwrap();
    assert_eq!(st.is_enabled(only, G), Ok(true));
    st.set("playback.afr_refresh_rate", Value::Bool(false))
        .unwrap();
    assert_eq!(st.is_enabled(only, G), Ok(false));
}

#[test]
fn hidden_parents_hide_their_dependents() {
    // clock_position needs the clock; with the clock off it is hidden, and
    // a disabled setting still accepts and keeps its value.
    let mut st = Settings::new();
    st.set("player.clock", Value::Bool(false)).unwrap();
    assert_eq!(st.is_visible("player.clock_position", G), Ok(false));
    assert_eq!(st.is_enabled("player.clock_date", G), Ok(false));
    st.set("player.clock_date", Value::Bool(true)).unwrap();
    assert_eq!(st.get("player.clock_date"), Ok(Value::Bool(true)));
}

#[test]
fn dependencies_resolve_at_the_requested_level() {
    let mut st = Settings::new();
    // logos.folder is global; its dependent is evaluated with the folder's
    // global value at any playlist.
    st.set("logos.folder", Value::text("/logos")).unwrap();
    assert_eq!(
        st.is_visible("logos.inexact_match", &Target::playlist("p")),
        Ok(true)
    );
}
