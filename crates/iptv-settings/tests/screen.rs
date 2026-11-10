//! The settings screen's state machine.

use iptv_settings::screen::{
    Ctx, Editor, Effect, Key, Offer, Page, Pane, Part, Row, Screen, Status, rows,
};
use iptv_settings::{Section, Settings, Value};

static PAGES: &[Page] = &[
    Page {
        key: "general",
        parts: &[Part::Section(Section::General)],
    },
    Page {
        key: "playlists",
        parts: &[Part::Keys(&["playlists.update_all"])],
    },
    Page {
        key: "epg",
        parts: &[Part::Section(Section::Epg)],
    },
    Page {
        key: "appearance",
        parts: &[
            Part::Section(Section::Appearance),
            Part::Section(Section::Guide),
        ],
    },
    Page {
        key: "playback",
        parts: &[
            Part::Section(Section::Player),
            Part::Section(Section::Catchup),
        ],
    },
    Page {
        key: "remote",
        parts: &[Part::Section(Section::Remote)],
    },
    Page {
        key: "other",
        parts: &[Part::Section(Section::Search), Part::Section(Section::Vod)],
    },
];

/// What the app offers (a sample of each kind and value type).
fn offer(key: &str) -> Offer {
    match key {
        "general.resume_last_channel"
        | "epg.update_interval"
        | "epg.keep_past"
        | "epg.update_now"
        | "epg.clear"
        | "appearance.accent"
        | "appearance.ui_transparency"
        | "catchup.seek_keys"
        | "catchup.forward_step"
        | "guide.highlight_now"
        | "guide.now_highlight_style"
        | "player.clock"
        | "player.clock_date"
        | "remote.customize"
        | "remote.key_overrides"
        | "search.favorites_first" => Offer::Active,
        "general.about" => Offer::ReadOnly,
        "general.start_on_boot" | "general.start_on_wake" | "search.prefer_voice" => Offer::Hidden,
        k if k.starts_with("vod.") => Offer::Later,
        _ => Offer::NotYet,
    }
}

fn everything(_: &str) -> Offer {
    Offer::Active
}

fn ctx(settings: &Settings) -> Ctx<'_> {
    Ctx {
        settings,
        offer: &offer,
    }
}

fn page(key: &str) -> &'static Page {
    PAGES.iter().find(|p| p.key == key).unwrap()
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

fn row(settings: &Settings, page_key: &str, key: &str) -> Row {
    rows(page(page_key), &ctx(settings))
        .into_iter()
        .find(|r| r.key() == Some(key))
        .unwrap()
}

#[test]
fn pages_choose_and_back_closes() {
    let s = Settings::new();
    let mut screen = Screen::new(PAGES, "nothing");
    assert_eq!((screen.page().key, screen.pane()), ("general", Pane::Pages));
    press(&mut screen, &s, &[Key::Up]);
    assert_eq!(screen.page().key, "general", "no wrap at the top");
    press(&mut screen, &s, &[Key::Down, Key::Down]);
    assert_eq!(screen.page().key, "epg");
    press(&mut screen, &s, &[Key::PageDown, Key::PageDown]);
    assert_eq!(screen.page().key, "other", "stops at the last");
    assert_eq!(press(&mut screen, &s, &[Key::Back]), Effect::Close);
    assert_eq!(Screen::new(PAGES, "remote").page_index(), 5);
}

#[test]
fn rows_leave_out_what_is_not_offered() {
    let s = Settings::new();
    let general = keys_of(&rows(page("general"), &ctx(&s)));
    assert!(!general.contains(&"general.start_on_boot"));
    assert!(general.contains(&"general.confirm_exit"));
    assert!(
        !general.contains(&"general.confirm_exit_window"),
        "advanced"
    );
    let other = keys_of(&rows(page("other"), &ctx(&s)));
    assert!(!other.contains(&"search.prefer_voice"));
    // Named rows are shown in their order.
    assert_eq!(
        keys_of(&rows(page("playlists"), &ctx(&s))),
        ["playlists.update_all"]
    );
}

#[test]
fn headings_name_subsections_and_sections_of_shared_pages() {
    let s = Settings::new();
    let epg = rows(page("epg"), &ctx(&s));
    assert!(
        matches!(epg[0], Row::Command(_)),
        "no heading without a subsection"
    );
    assert!(epg.contains(&Row::Heading {
        section: Section::Epg,
        subsection: Some("updates")
    }));
    // Search has no subsections: on a shared page its name heads its rows.
    let other = rows(page("other"), &ctx(&s));
    assert_eq!(
        other[0],
        Row::Heading {
            section: Section::Search,
            subsection: None
        }
    );
}

#[test]
fn statuses_follow_the_offer() {
    let mut s = Settings::new();
    let c = ctx(&s);
    assert_eq!(c.status(&row(&s, "epg", "epg.keep_past")), Status::Active);
    assert_eq!(c.status(&row(&s, "epg", "epg.time_offset")), Status::NotYet);
    assert_eq!(c.status(&row(&s, "other", "vod.sort")), Status::Later);
    assert_eq!(
        c.status(&row(&s, "general", "general.about")),
        Status::ReadOnly
    );
    // An active key-binding list has no editor: read-only.
    let bindings = row(&s, "remote", "remote.key_overrides");
    assert_eq!(c.status(&bindings), Status::ReadOnly);
    // Switched off by another setting.
    s.set("player.clock", Value::Bool(false)).unwrap();
    let date = row(&s, "playback", "player.clock_date");
    assert_eq!(ctx(&s).status(&date), Status::Disabled);
    let mut screen = Screen::new(PAGES, "playback");
    assert_eq!(screen.tap_row("player.clock_date", &ctx(&s)), Effect::None);
    assert_eq!(screen.editor(), None);
}

#[test]
fn entering_the_rows_focuses_the_first_row_the_app_acts_on() {
    let s = Settings::new();
    let mut screen = Screen::new(PAGES, "general");
    // General starts with commands the app cannot run yet.
    assert!(matches!(
        rows(page("general"), &ctx(&s))[0],
        Row::Command(_)
    ));
    press(&mut screen, &s, &[Key::Right]);
    assert_eq!(screen.pane(), Pane::Rows);
    assert_eq!(screen.focus(), Some("general.resume_last_channel"));
    // Left goes back to the pages; Right returns to the same row.
    press(&mut screen, &s, &[Key::Down, Key::Left]);
    assert_eq!(screen.pane(), Pane::Pages);
    press(&mut screen, &s, &[Key::Ok]);
    assert_eq!(screen.focus(), Some("general.confirm_exit"));
    // A page without any row the app acts on: its first row.
    let mut screen = Screen::new(PAGES, "playlists");
    press(&mut screen, &s, &[Key::Ok]);
    assert_eq!(screen.focus(), Some("playlists.update_all"));
}

#[test]
fn rows_skip_headings_and_stop_at_the_ends() {
    let s = Settings::new();
    let keys = keys_of(&rows(page("epg"), &ctx(&s)));
    let mut screen = Screen::new(PAGES, "epg");
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
fn ok_flips_a_switch() {
    let mut s = Settings::new();
    let mut screen = Screen::new(PAGES, "general");
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
    let mut screen = Screen::new(PAGES, "playback");
    screen.tap_row("catchup.seek_keys", &ctx(&s));
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
    let mut screen = Screen::new(PAGES, "appearance");
    assert_eq!(screen.tap_row("appearance.accent", &ctx(&s)), Effect::None);
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
    let mut screen = Screen::new(PAGES, "epg");
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
    let mut screen = Screen::new(PAGES, "appearance");
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
    let mut screen = Screen::new(PAGES, "general");
    let c = ctx(&s);
    assert_eq!(screen.tap_row("general.language", &c), Effect::None);
    assert_eq!(screen.focus(), Some("general.language"));
    assert_eq!(screen.editor(), None);
    assert_eq!(screen.tap_row("general.backup", &c), Effect::None);
    assert_eq!(
        screen.tap_row("general.about", &c),
        Effect::None,
        "read-only"
    );
    let mut screen = Screen::new(PAGES, "other");
    assert_eq!(screen.tap_row("vod.sort", &c), Effect::None, "later");
}

#[test]
fn commands_run_after_a_confirmation_when_asked() {
    let s = Settings::new();
    let mut screen = Screen::new(PAGES, "epg");
    assert_eq!(
        screen.tap_row("epg.update_now", &ctx(&s)),
        Effect::Run("epg.update_now")
    );
    // Clearing the guide asks first; Back keeps everything.
    assert_eq!(screen.tap_row("epg.clear", &ctx(&s)), Effect::None);
    assert!(matches!(screen.editor(), Some(Editor::Confirm { .. })));
    assert_eq!(
        press(&mut screen, &s, &[Key::Left, Key::Back]),
        Effect::None
    );
    assert_eq!(screen.editor(), None);
    screen.tap_row("epg.clear", &ctx(&s));
    assert_eq!(press(&mut screen, &s, &[Key::Ok]), Effect::Run("epg.clear"));
    assert_eq!(screen.editor(), None);
}

#[test]
fn a_read_only_list_scrolls_and_closes() {
    let s = Settings::new();
    let mut screen = Screen::new(PAGES, "remote");
    assert_eq!(
        screen.tap_row("remote.customize", &ctx(&s)),
        Effect::Run("remote.customize")
    );
    screen.open_list("remote.customize", 3);
    press(&mut screen, &s, &[Key::Down; 5]);
    assert!(matches!(
        screen.editor(),
        Some(Editor::List { focus: 2, .. })
    ));
    press(&mut screen, &s, &[Key::Ok]);
    assert_eq!(screen.editor(), None);
    screen.open_list("remote.customize", 3);
    press(&mut screen, &s, &[Key::Back]);
    assert_eq!((screen.editor(), screen.pane()), (None, Pane::Rows));
}

#[test]
fn focus_follows_the_row_not_its_position() {
    let mut s = Settings::new();
    s.set("guide.highlight_now", Value::Bool(false)).unwrap();
    let mut screen = Screen::new(PAGES, "appearance");
    screen.tap_row("guide.now_line_full_height", &ctx(&s));
    // A row appears above the focused one.
    s.set("guide.highlight_now", Value::Bool(true)).unwrap();
    assert_eq!(screen.focus(), Some("guide.now_line_full_height"));
    press(&mut screen, &s, &[Key::Up]);
    assert_eq!(screen.focus(), Some("guide.now_highlight_style"));
    // The focused row disappears: movement starts from the first row.
    s.set("guide.highlight_now", Value::Bool(false)).unwrap();
    press(&mut screen, &s, &[Key::Down]);
    let keys = keys_of(&rows(page("appearance"), &ctx(&s)));
    assert_eq!(screen.focus(), Some(keys[1]));
}

#[test]
fn text_is_kept_only_when_valid() {
    let s = Settings::new();
    let c = Ctx {
        settings: &s,
        offer: &everything,
    };
    let mut screen = Screen::new(PAGES, "general");
    assert_eq!(screen.tap_row("general.user_agent", &c), Effect::None);
    screen.set_text("SyntheticPlayer/1.0");
    assert_eq!(
        screen.key(Key::Ok, &c),
        Effect::Set {
            key: "general.user_agent",
            value: Value::text("SyntheticPlayer/1.0")
        }
    );
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
fn tapping_a_page_leaves_any_editor() {
    let s = Settings::new();
    let mut screen = Screen::new(PAGES, "epg");
    screen.tap_row("epg.keep_past", &ctx(&s));
    assert!(screen.editor().is_some());
    screen.tap_page(6);
    assert_eq!(
        (
            screen.page().key,
            screen.pane(),
            screen.editor(),
            screen.focus()
        ),
        ("other", Pane::Pages, None, None)
    );
}
