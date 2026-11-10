//! The settings screen's state machine.

use iptv_settings::screen::{Ctx, Editor, Effect, Key, Pane, Row, Screen, Status, rows};
use iptv_settings::{Section, Settings, Value};

/// The app acts on these (a sample of each value type).
fn available(key: &str) -> bool {
    matches!(
        key,
        "general.resume_last_channel"
            | "epg.update_interval"
            | "epg.keep_past"
            | "appearance.accent"
            | "appearance.ui_transparency"
            | "catchup.seek_keys"
            | "catchup.forward_step"
            | "guide.highlight_now"
            | "guide.now_highlight_style"
            | "player.clock"
            | "player.clock_date"
            | "general.udp_proxy"
            | "remote.key_overrides"
    )
}

fn ctx(settings: &Settings) -> Ctx<'_> {
    Ctx {
        settings,
        available: &available,
    }
}

fn press(screen: &mut Screen, settings: &Settings, keys: &[Key]) -> Effect {
    let mut last = Effect::None;
    for k in keys {
        last = screen.key(*k, &ctx(settings));
    }
    last
}

/// Applies a `Set` effect like the app does.
fn apply(settings: &mut Settings, effect: Effect) {
    match effect {
        Effect::Set { key, value } => {
            settings.set(key, value).unwrap();
        }
        other => panic!("expected a value to store, got {other:?}"),
    }
}

fn keys_of(rows: &[Row]) -> Vec<&'static str> {
    rows.iter().filter_map(Row::key).collect()
}

#[test]
fn sections_choose_and_back_closes() {
    let s = Settings::new();
    let mut screen = Screen::new(Section::General);
    assert_eq!(screen.pane(), Pane::Sections);
    press(&mut screen, &s, &[Key::Up]);
    assert_eq!(screen.section(), Section::General, "no wrap at the top");
    press(&mut screen, &s, &[Key::Down, Key::Down, Key::Down]);
    assert_eq!(screen.section(), Section::Epg);
    press(
        &mut screen,
        &s,
        &[Key::PageDown, Key::PageDown, Key::PageDown, Key::PageDown],
    );
    assert_eq!(screen.section(), Section::Vod, "stops at the last");
    assert_eq!(press(&mut screen, &s, &[Key::Back]), Effect::Close);
}

#[test]
fn entering_the_rows_focuses_the_first_row_the_app_acts_on() {
    let s = Settings::new();
    let mut screen = Screen::new(Section::General);
    // General starts with commands the app cannot run yet.
    let first = rows(Section::General, &s, false);
    assert!(matches!(first[0], Row::Command(_)));
    press(&mut screen, &s, &[Key::Right]);
    assert_eq!(screen.pane(), Pane::Rows);
    assert_eq!(screen.focus(), Some("general.resume_last_channel"));
    // Left goes back to the sections; Right returns to the same row.
    press(&mut screen, &s, &[Key::Down, Key::Left]);
    assert_eq!(screen.pane(), Pane::Sections);
    press(&mut screen, &s, &[Key::Ok]);
    assert_eq!(screen.focus(), Some("general.confirm_exit"));
    // A section without any row the app acts on: its first row.
    let mut screen = Screen::new(Section::Vod);
    press(&mut screen, &s, &[Key::Ok]);
    assert_eq!(screen.focus(), Some("vod.movies_by_category"));
}

#[test]
fn rows_skip_headings_and_stop_at_the_ends() {
    let s = Settings::new();
    let all = rows(Section::Epg, &s, false);
    assert!(
        matches!(all[0], Row::Command(_)),
        "commands without a subsection first"
    );
    assert!(
        all.iter().any(|r| matches!(
            r,
            Row::Heading {
                subsection: "updates",
                ..
            }
        )),
        "subsections have headings"
    );
    let keys = keys_of(&all);
    let mut screen = Screen::new(Section::Epg);
    press(&mut screen, &s, &[Key::Right]);
    assert_eq!(screen.focus(), Some("epg.update_interval"));
    press(&mut screen, &s, &[Key::PageUp, Key::Up]);
    assert_eq!(screen.focus(), Some(keys[0]));
    press(&mut screen, &s, &[Key::PageDown; 10]);
    assert_eq!(screen.focus(), keys.last().copied());
    assert_eq!(press(&mut screen, &s, &[Key::Down]), Effect::None);
    assert_eq!(screen.focus(), keys.last().copied());
}

#[test]
fn advanced_settings_are_left_out() {
    let s = Settings::new();
    let keys = keys_of(&rows(Section::Epg, &s, false));
    assert!(!keys.contains(&"epg.full_scan"));
    assert!(keys_of(&rows(Section::Epg, &s, true)).contains(&"epg.full_scan"));
}

#[test]
fn ok_flips_a_switch() {
    let mut s = Settings::new();
    let mut screen = Screen::new(Section::General);
    let effect = press(&mut screen, &s, &[Key::Right, Key::Ok]);
    assert_eq!(
        effect,
        Effect::Set {
            key: "general.resume_last_channel",
            value: Value::Bool(false)
        }
    );
    apply(&mut s, effect);
    assert_eq!(screen.editor(), None);
    assert_eq!(
        press(&mut screen, &s, &[Key::Ok]),
        Effect::Set {
            key: "general.resume_last_channel",
            value: Value::Bool(true)
        }
    );
}

#[test]
fn a_choice_opens_a_list_on_the_current_value() {
    let s = Settings::new();
    let mut screen = Screen::new(Section::Catchup);
    press(&mut screen, &s, &[Key::Right]);
    assert_eq!(screen.focus(), Some("catchup.seek_keys"));
    press(&mut screen, &s, &[Key::Ok]);
    let Some(Editor::Choice {
        focus, selected, ..
    }) = screen.editor().cloned()
    else {
        panic!("a choice list opens");
    };
    assert_eq!((focus, selected), (0, 0), "left_right is the default");
    // Left/Right do nothing in the list; Up stops at the top.
    press(
        &mut screen,
        &s,
        &[Key::Left, Key::Right, Key::Up, Key::Down],
    );
    assert_eq!(
        press(&mut screen, &s, &[Key::Ok]),
        Effect::Set {
            key: "catchup.seek_keys",
            value: Value::choice("down_up")
        }
    );
    assert_eq!(screen.editor(), None);
    // Back closes the list without a change.
    press(&mut screen, &s, &[Key::Ok, Key::Down]);
    assert_eq!(press(&mut screen, &s, &[Key::Back]), Effect::None);
    assert_eq!((screen.editor(), screen.pane()), (None, Pane::Rows));
}

#[test]
fn colours_are_picked_as_colours() {
    let s = Settings::new();
    let mut screen = Screen::new(Section::Appearance);
    assert_eq!(
        screen.tap_row("appearance.accent", &ctx(&s)),
        Effect::None,
        "a tap opens the list"
    );
    assert_eq!(
        screen.pick(13),
        Effect::Set {
            key: "appearance.accent",
            value: Value::colour("red")
        }
    );
}

#[test]
fn a_stepper_moves_in_steps_within_range_and_keeps_on_ok() {
    let mut s = Settings::new();
    let mut screen = Screen::new(Section::Epg);
    press(&mut screen, &s, &[Key::Right, Key::Ok]);
    let hour = 3_600_000;
    let value = |screen: &Screen| match screen.editor() {
        Some(Editor::Number { value, .. }) => *value,
        other => panic!("stepper expected, got {other:?}"),
    };
    assert_eq!(value(&screen), 24 * hour);
    press(&mut screen, &s, &[Key::Right, Key::Up]);
    assert_eq!(value(&screen), 26 * hour);
    press(
        &mut screen,
        &s,
        &[Key::PageDown, Key::PageDown, Key::PageDown],
    );
    assert_eq!(value(&screen), 0, "stops at the minimum");
    screen.step(-1);
    assert_eq!(value(&screen), 0);
    for _ in 0..20 {
        screen.step(10);
    }
    assert_eq!(value(&screen), 168 * hour, "stops at the maximum");
    let effect = press(&mut screen, &s, &[Key::Left, Key::Ok]);
    assert_eq!(
        effect,
        Effect::Set {
            key: "epg.update_interval",
            value: Value::Duration(167 * hour)
        }
    );
    apply(&mut s, effect);
    // Back cancels: nothing to store, the old value stays.
    press(&mut screen, &s, &[Key::Ok, Key::Right]);
    assert_eq!(press(&mut screen, &s, &[Key::Back]), Effect::None);
    assert_eq!(screen.editor(), None);
    assert_eq!(
        s.get("epg.update_interval"),
        Ok(Value::Duration(167 * hour))
    );
}

#[test]
fn integers_step_as_integers() {
    let s = Settings::new();
    let mut screen = Screen::new(Section::Appearance);
    screen.tap_row("appearance.ui_transparency", &ctx(&s));
    screen.step(1);
    assert_eq!(
        screen.commit(),
        Effect::Set {
            key: "appearance.ui_transparency",
            value: Value::Int(25)
        }
    );
}

#[test]
fn rows_the_app_does_not_act_on_do_nothing() {
    let s = Settings::new();
    let mut screen = Screen::new(Section::General);
    let c = ctx(&s);
    // A setting, a command and a key-binding list (no editor).
    assert_eq!(screen.tap_row("general.start_on_boot", &c), Effect::None);
    assert_eq!(screen.focus(), Some("general.start_on_boot"));
    assert_eq!(screen.editor(), None);
    assert_eq!(screen.tap_row("general.backup", &c), Effect::None);
    let all = rows(Section::Remote, &s, false);
    let bindings = all
        .iter()
        .find(|r| r.key() == Some("remote.key_overrides"))
        .unwrap();
    assert_eq!(c.status(bindings), Status::Unavailable);
    let boot = rows(Section::General, &s, false)
        .into_iter()
        .find(|r| r.key() == Some("general.start_on_boot"))
        .unwrap();
    assert_eq!(c.status(&boot), Status::Unavailable);
}

#[test]
fn commands_the_app_runs_are_run() {
    let s = Settings::new();
    let runs = |key: &str| key == "epg.update_now";
    let c = Ctx {
        settings: &s,
        available: &runs,
    };
    let mut screen = Screen::new(Section::Epg);
    assert_eq!(
        screen.tap_row("epg.update_now", &c),
        Effect::Run("epg.update_now")
    );
}

#[test]
fn dependencies_hide_or_disable_rows() {
    let mut s = Settings::new();
    let style = "guide.now_highlight_style";
    assert!(keys_of(&rows(Section::Guide, &s, false)).contains(&style));
    s.set("guide.highlight_now", Value::Bool(false)).unwrap();
    assert!(!keys_of(&rows(Section::Guide, &s, false)).contains(&style));

    s.set("player.clock", Value::Bool(false)).unwrap();
    let date = rows(Section::Player, &s, false)
        .into_iter()
        .find(|r| r.key() == Some("player.clock_date"))
        .unwrap();
    assert_eq!(ctx(&s).status(&date), Status::Disabled);
    let mut screen = Screen::new(Section::Player);
    assert_eq!(screen.tap_row("player.clock_date", &ctx(&s)), Effect::None);
}

#[test]
fn focus_follows_the_row_not_its_position() {
    let mut s = Settings::new();
    s.set("guide.highlight_now", Value::Bool(false)).unwrap();
    let mut screen = Screen::new(Section::Guide);
    screen.tap_row("guide.now_line_full_height", &ctx(&s));
    // A row appears above the focused one.
    s.set("guide.highlight_now", Value::Bool(true)).unwrap();
    assert_eq!(screen.focus(), Some("guide.now_line_full_height"));
    press(&mut screen, &s, &[Key::Up]);
    assert_eq!(screen.focus(), Some("guide.now_highlight_style"));
    // The focused row disappears: movement starts from the first row.
    s.set("guide.highlight_now", Value::Bool(false)).unwrap();
    press(&mut screen, &s, &[Key::Down]);
    let keys = keys_of(&rows(Section::Guide, &s, false));
    assert_eq!(screen.focus(), Some(keys[1]));
}

#[test]
fn text_is_edited_and_kept_on_ok() {
    let s = Settings::new();
    let all = |_: &str| true;
    let c = Ctx {
        settings: &s,
        available: &all,
    };
    let mut screen = Screen::new(Section::General);
    assert_eq!(screen.tap_row("general.user_agent", &c), Effect::None);
    assert!(matches!(screen.editor(), Some(Editor::Text { .. })));
    screen.set_text("SyntheticPlayer/1.0");
    assert_eq!(
        screen.key(Key::Ok, &c),
        Effect::Set {
            key: "general.user_agent",
            value: Value::text("SyntheticPlayer/1.0")
        }
    );
    // Advanced rows are not listed, so a tap finds nothing.
    assert_eq!(screen.tap_row("general.udp_proxy", &c), Effect::None);
    assert_eq!(screen.editor(), None);
}

#[test]
fn an_invalid_text_keeps_the_editor_open() {
    let s = Settings::new();
    let all = |_: &str| true;
    let c = Ctx {
        settings: &s,
        available: &all,
    };
    let mut screen = Screen::new(Section::General);
    screen.tap_row("general.language", &c);
    screen.set_text("not a language");
    assert_eq!(screen.commit(), Effect::None);
    assert!(matches!(
        screen.editor(),
        Some(Editor::Text { invalid: true, .. })
    ));
    screen.set_text("pt-BR");
    assert!(matches!(
        screen.editor(),
        Some(Editor::Text { invalid: false, .. })
    ));
    assert_eq!(
        screen.commit(),
        Effect::Set {
            key: "general.language",
            value: Value::text("pt-BR")
        }
    );
}

#[test]
fn tapping_a_section_leaves_any_editor() {
    let s = Settings::new();
    let mut screen = Screen::new(Section::Epg);
    screen.tap_row("epg.keep_past", &ctx(&s));
    assert!(screen.editor().is_some());
    screen.tap_section(Section::Search);
    assert_eq!(
        (
            screen.section(),
            screen.pane(),
            screen.editor(),
            screen.focus()
        ),
        (Section::Search, Pane::Sections, None, None)
    );
}
