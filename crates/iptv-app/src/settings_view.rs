//! The settings screen (Long OK menu → Settings): the pages on the left
//! ([`PAGES`], in the reference's order), the chosen page's rows with
//! their current values on the right, the focused row's description below,
//! and the editors and dialogs (`editors`). State and rules live in
//! `iptv_settings::screen`; this draws it, maps remote keys and taps to
//! it, stores what it answers through `crate::settings` (saved at once,
//! applied where the app reads it) and runs its commands.
//!
//! Rows the app does not act on yet are dimmed and say so ("later" for
//! recordings, reminders, movies and series, parental control); rows
//! another setting switches off say which; device-level features of TV
//! boxes are left out (`settings::offer`). Remote: Up/Down move, Right/OK
//! enter a page, OK changes a row, Left/Back go back, Back on the pages
//! closes. Touch and mouse: tap a page, tap a row. Wording is iptv-rs's
//! own (`words`). Also the short notices of the app (`notify`), such as
//! the second Back that leaves (`may_leave`).

mod editors;
mod remote_keys;
mod words;

use std::cell::Cell;
use std::time::{Duration, Instant};

use iptv_nav::Action;
use iptv_settings::schema::{Effect as DependencyEffect, Test, ValueType};
use iptv_settings::screen::{Ctx, Editor, Effect, Key, Page, Pane, Part, Row, Screen, Status};
use iptv_settings::{Section, Settings, Target, Value, keys};
use nsh_core::prelude::*;

use crate::data::stored::{self, GuideStatus};
use crate::theme::{FOCUS_ALPHA, RADIUS, RESTING_ALPHA, TEXT_DISABLED_ALPHA, TEXT_SECONDARY_ALPHA};
use crate::{Ui, tint};

const WHITE: u32 = 0xFFFFFF;
/// Titles of rows the app does not act on.
const DIM_ALPHA: u8 = 0x80;
const PANEL_W: f32 = 760.0;
const PAGES_W: f32 = 190.0;
const PAGE_H: f32 = 38.0;
const ROW_H: f32 = 52.0;
const HEADING_H: f32 = 34.0;
const NOTICE_FOR: Duration = Duration::from_millis(2_500);

/// The pages, in the order the reference shows its settings.
pub static PAGES: &[Page] = &[
    Page {
        key: "general",
        parts: &[
            Part::Section(Section::General),
            Part::Section(Section::History),
        ],
    },
    // The playlists screen (another workstream) is the whole page.
    Page {
        key: "playlists",
        parts: &[Part::Keys(&["playlists.manage"])],
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
            Part::Section(Section::Channels),
            Part::Section(Section::Logos),
        ],
    },
    Page {
        key: "playback",
        parts: &[
            Part::Section(Section::Player),
            Part::Section(Section::Playback),
            Part::Section(Section::Catchup),
        ],
    },
    Page {
        key: "remote",
        parts: &[Part::Section(Section::Remote)],
    },
    Page {
        key: "parental",
        parts: &[Part::Section(Section::Parental)],
    },
    Page {
        key: "other",
        parts: &[
            Part::Section(Section::Search),
            Part::Section(Section::Recordings),
            Part::Section(Section::Vod),
        ],
    },
    Page {
        key: "about",
        parts: &[Part::Keys(&["general.about"])],
    },
];

thread_local! {
    /// The page shown last: the screen opens on it again.
    static LAST: Cell<&'static str> = const { Cell::new("general") };
    /// The notice line's text (made with the layer, under the app's owner).
    static NOTICE: Cell<Option<Signal<Option<String>>>> = const { Cell::new(None) };
    /// When Back last asked whether to leave.
    static LEAVE_ASKED: Cell<Option<Instant>> = const { Cell::new(None) };
}

// --- State ---------------------------------------------------------------------

pub fn open(ui: Ui) {
    if !is_open(ui) {
        ui.settings_screen
            .set(Some(Screen::new(PAGES, LAST.with(Cell::get))));
    }
}

pub fn close(ui: Ui) {
    if let Some(page) = ui
        .settings_screen
        .with_untracked(|s| s.as_ref().map(|s| s.page().key))
    {
        LAST.with(|l| l.set(page));
    }
    let typing = editing_text(ui);
    ui.settings_screen.set(None);
    if typing {
        dismiss_keyboard();
    }
}

/// The screen is shown and takes the remote.
pub fn is_open(ui: Ui) -> bool {
    ui.settings_screen.with(Option::is_some)
}

fn editing_text(ui: Ui) -> bool {
    ui.settings_screen.with_untracked(|s| {
        s.as_ref()
            .is_some_and(|s| matches!(s.editor(), Some(Editor::Text { .. })))
    })
}

fn screen_key(action: Action) -> Option<Key> {
    Some(match action {
        Action::MoveUp => Key::Up,
        Action::MoveDown => Key::Down,
        Action::MoveLeft => Key::Left,
        Action::MoveRight => Key::Right,
        Action::Select => Key::Ok,
        Action::Back => Key::Back,
        Action::PageUp | Action::ChannelUp => Key::PageUp,
        Action::PageDown | Action::ChannelDown => Key::PageDown,
        _ => return None,
    })
}

/// A remote action while the screen is shown. While text is typed, the
/// field has the keys; OK and Back arrive once it let them go.
pub fn key(ui: Ui, action: Action) {
    if let Some(key) = screen_key(action) {
        update(ui, |screen, ctx| screen.key(key, ctx));
    }
}

/// Back: closes an editor, leaves the rows, then closes the screen.
pub fn back(ui: Ui) -> bool {
    key(ui, Action::Back);
    true
}

fn with_ctx<R>(f: impl FnOnce(&Ctx) -> R) -> R {
    let settings = crate::settings::get();
    f(&Ctx {
        settings: &settings,
        offer: &crate::settings::offer,
    })
}

/// Changes the screen with what the app knows now, then carries out what
/// it answers.
fn update(ui: Ui, f: impl FnOnce(&mut Screen, &Ctx) -> Effect) {
    let typing = editing_text(ui);
    let mut effect = Effect::None;
    with_ctx(|ctx| {
        ui.settings_screen.update(|s| {
            if let Some(s) = s {
                effect = f(s, ctx);
            }
        });
    });
    if typing && !editing_text(ui) {
        dismiss_keyboard();
    }
    match effect {
        Effect::None => {}
        Effect::Set { key, value } => {
            if let Err(e) = crate::settings::set(ui, key, value) {
                log::warn!("iptv settings: {e}");
            }
        }
        Effect::Run(key) => run(ui, key),
        Effect::Close => close(ui),
    }
}

/// Tells the rows to read their status again.
fn refresh(ui: Ui) {
    ui.settings.update(|v| *v += 1);
}

/// Runs a command (`settings::COMMANDS`).
fn run(ui: Ui, key: &'static str) {
    match key {
        "epg.update_now" => {
            if stored::update_now(ui, move || refresh(ui)) {
                refresh(ui);
            } else {
                notify("This playlist has no guide source switched on");
            }
        }
        // The playlist on screen's guide sources, over the settings screen.
        "epg.manage_sources" | "epg.add_source" => match stored::shown() {
            Some(playlist) if key == "epg.add_source" => {
                crate::epg_sources_view::open_add(ui, playlist);
            }
            Some(playlist) => crate::epg_sources_view::open(ui, playlist),
            None => notify("No playlist on screen"),
        },
        "epg.clear" => {
            let cleared = stored::clear(ui, move |deleted| {
                notify(if deleted {
                    "Guide data deleted"
                } else {
                    "The guide data could not be deleted"
                });
                refresh(ui);
            });
            if !cleared {
                notify("No playlist on screen");
            }
        }
        "remote.customize" => {
            let len = remote_keys::key_lines().len();
            ui.settings_screen.update(|s| {
                if let Some(s) = s {
                    s.open_list(key, len);
                }
            });
        }
        // The playlists screen, over the settings screen.
        "playlists.manage" => crate::playlists_view::open(ui),
        other => log::info!("iptv settings: {other} is not implemented yet"),
    }
}

fn state<R>(ui: Ui, f: impl FnOnce(&Screen) -> R) -> Option<R> {
    ui.settings_screen.with(|s| s.as_ref().map(f))
}

fn status(settings: &Settings, row: &Row) -> Status {
    Ctx {
        settings,
        offer: &crate::settings::offer,
    }
    .status(row)
}

/// Why a row does not do what it says, or what to expect from it.
fn note(settings: &Settings, row: &Row) -> String {
    let restart = match row {
        Row::Setting(s) => s.flags.requires_restart,
        Row::Command(c) => c.requires_restart,
        Row::Heading { .. } => false,
    };
    match (status(settings, row), row) {
        (Status::NotYet, _) => "Not available yet".to_owned(),
        (Status::Later, _) => "In a later version".to_owned(),
        (Status::Disabled, Row::Setting(s)) => {
            // The first switch that is off where it needs to be on.
            let blocker = s
                .depends
                .iter()
                .filter(|d| d.effect == DependencyEffect::Enabled)
                .flat_map(|d| d.any_of.iter())
                .find(|w| w.test == Test::IsTrue && settings.get(w.key) == Ok(Value::Bool(false)));
            match blocker {
                Some(w) => format!("Turn on “{}” first", words::label(w.key)),
                None => "Turned off by another setting".to_owned(),
            }
        }
        (Status::Active, _) if restart => "Takes effect after a restart".to_owned(),
        _ => String::new(),
    }
}

/// How many rows of `page` the app acts on, of how many.
fn counts(page: &Page) -> (usize, usize) {
    with_ctx(|ctx| {
        let rows = iptv_settings::screen::rows(page, ctx);
        let keyed = rows.iter().filter(|r| r.key().is_some());
        let active = keyed
            .clone()
            .filter(|r| ctx.status(r) == Status::Active)
            .count();
        (active, keyed.count())
    })
}

/// Whether `page` has rows that show something (active or read-only).
fn lit_page(page: &Page) -> bool {
    with_ctx(|ctx| {
        iptv_settings::screen::rows(page, ctx).iter().any(|r| {
            r.key().is_some() && matches!(ctx.status(r), Status::Active | Status::ReadOnly)
        })
    })
}

/// What a command row shows on its right.
fn command_value(key: &str) -> String {
    match key {
        "epg.update_now" => match stored::guide_status() {
            GuideStatus::NoSource => "No playlist".to_owned(),
            GuideStatus::Updating => "Updating…".to_owned(),
            GuideStatus::Empty => "Not downloaded".to_owned(),
            GuideStatus::Updated { at, channels } => {
                format!("Updated {} · {channels} channels", time_label(at))
            }
            GuideStatus::Failed { at, .. } => format!("Failed {}", time_label(at)),
        },
        "epg.manage_sources" => sources_value(),
        "general.about" => format!("Version {}", env!("CARGO_PKG_VERSION")),
        _ => String::new(),
    }
}

/// The "Guide sources" row's value: how many, switched on, and failed at
/// their last download.
fn sources_value() -> String {
    let Some(playlist) = stored::shown() else {
        return "No playlist".to_owned();
    };
    let lines = stored::source_lines(playlist);
    let on = lines.iter().filter(|l| l.source.enabled).count();
    let failed = lines
        .iter()
        .filter(|l| l.source.enabled && l.result.as_ref().is_some_and(|r| r.error.is_some()))
        .count();
    let mut value = match (lines.len(), on) {
        (0, _) => "None".to_owned(),
        (1, 1) => "1 source".to_owned(),
        (all, on) if on == all => format!("{all} sources"),
        (all, on) => format!("{on} of {all} on"),
    };
    if failed > 0 {
        value += &format!(" · {failed} failed");
    }
    value
}

/// A past time as the guide writes it (`14:05`, `Tue 10 · 14:05`).
fn time_label(unix: i64) -> String {
    iptv_core::clock::label(unix, crate::time::wall_unix(), crate::time::offset_minutes)
}

/// Shows `text` briefly at the bottom of the screen.
pub fn notify(text: &str) {
    let Some(notice) = NOTICE.with(Cell::get) else {
        return;
    };
    notice.set(Some(text.to_owned()));
    let shown = text.to_owned();
    let _ = set_timeout(NOTICE_FOR, move || {
        if notice.with_untracked(|n| n.as_deref() == Some(shown.as_str())) {
            notice.set(None);
        }
    });
}

/// Whether a Back press may leave the app (`general.confirm_exit`): the
/// first press asks for a second one within `general.confirm_exit_window`.
pub fn may_leave() -> bool {
    let s = crate::settings::get();
    let at = &Target::Global;
    if !s.bool(keys::CONFIRM_EXIT, at) {
        return true;
    }
    let window = Duration::from_millis(
        u64::try_from(s.duration_ms(keys::CONFIRM_EXIT_WINDOW, at)).unwrap_or(0),
    );
    let now = Instant::now();
    let asked = LEAVE_ASKED.with(|a| a.replace(Some(now)));
    if second_press(asked, now, window) {
        LEAVE_ASKED.with(|a| a.set(None));
        return true;
    }
    notify("Press Back again to leave");
    false
}

fn second_press(asked: Option<Instant>, now: Instant, window: Duration) -> bool {
    asked.is_some_and(|t| now.duration_since(t) <= window)
}

// --- View ----------------------------------------------------------------------

fn accent_fill(ui: Ui) -> Rgba {
    tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
}

fn secondary() -> Rgba {
    tint(WHITE, TEXT_SECONDARY_ALPHA)
}

/// The settings screen and the notice line.
pub fn layer(ui: Ui) -> impl IntoElement {
    let notice = signal(None::<String>);
    NOTICE.with(|n| n.set(Some(notice)));
    view()
        .absolute()
        .inset_0()
        .pointer_events(PointerEvents::BoxNone)
        .child(when(move || is_open(ui), move || screen(ui)))
        .child(when(
            move || notice.with(Option::is_some),
            move || {
                view()
                    .absolute()
                    .left(px(0.0))
                    .w_full()
                    .bottom(px(28.0))
                    .items_center()
                    .pointer_events(PointerEvents::None)
                    .child(
                        text(move || notice.get().unwrap_or_default())
                            .px(px(18.0))
                            .py(px(10.0))
                            .rounded(px(RADIUS))
                            .text_size(px(15.0))
                            .text_color(rgb(WHITE))
                            .bg(tint(0x000000, 0xC0)),
                    )
            },
        ))
}

fn screen(ui: Ui) -> impl IntoElement {
    let rows = computed(move || {
        let settings = crate::settings::read(ui);
        state(ui, |s| {
            s.rows(&Ctx {
                settings: &settings,
                offer: &crate::settings::offer,
            })
        })
        .unwrap_or_default()
    });
    let editing = computed(move || state(ui, |s| s.editor().is_some()).unwrap_or(false));
    view()
        .absolute()
        .inset_0()
        .flex_row()
        // The guide (or video) stays visible, dimmed; a tap there closes.
        .child(view().flex_1().bg(tint(0x000000, 0x90)).on_press(move || {
            ui.touched();
            close(ui);
        }))
        .child(
            view()
                .w(px(PANEL_W))
                .pt_safe_area()
                .px(px(20.0))
                .py(px(16.0))
                .gap(px(10.0))
                .bg(move || rgb(ui.theme.with(|t| t.background.panel)))
                .child(header(ui))
                .child(
                    view()
                        .flex_1()
                        .flex_row()
                        .gap(px(14.0))
                        .child(pages(ui))
                        .child(rows_list(ui, rows)),
                )
                .child(footer(ui)),
        )
        .child(when(move || editing.get(), move || editors::layer(ui)))
}

fn header(ui: Ui) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(16.0))
        .child(text("Settings").text_size(px(20.0)).text_color(rgb(WHITE)))
        .child(
            text(move || hint(ui))
                .flex_1()
                .number_of_lines(1)
                .text_size(px(13.0))
                .text_color(secondary()),
        )
}

fn hint(ui: Ui) -> &'static str {
    if !ui.keys_shown() {
        return "Tap a page, then a setting to change it";
    }
    let at = state(ui, |s| (s.pane(), s.editor().cloned())).unwrap_or((Pane::Pages, None));
    match at {
        (_, Some(Editor::Choice { .. })) => "▲▼ choose   OK keep   Back cancel",
        (_, Some(Editor::Number { .. })) => "◀▶ change   OK keep   Back cancel",
        (_, Some(Editor::Text { .. })) => "Type, then OK to keep   Back cancel",
        (_, Some(Editor::Confirm { .. })) => "OK go ahead   Back cancel",
        (_, Some(Editor::List { .. })) => "▲▼ scroll   OK Back close",
        (Pane::Pages, None) => "▲▼ page   ▶ OK its settings   Back close",
        (Pane::Rows, None) => "▲▼ move   OK change   ◀ Back pages",
    }
}

fn pages(ui: Ui) -> impl IntoElement {
    let list = element_ref();
    let chosen = computed(move || state(ui, Screen::page_index));
    effect(move || {
        if let Some(i) = chosen.get() {
            list.scroll_to_index(i as u32, percentage(0.5), px(0.0), false);
        }
    });
    let entry = move |at: Position| {
        let page = move || PAGES.get(at.index());
        let is_chosen = move || chosen.get() == Some(at.index());
        let focused = move || {
            ui.keys_shown()
                && is_chosen()
                && state(ui, |s| s.pane() == Pane::Pages && s.editor().is_none()) == Some(true)
        };
        text(move || page().map_or("", |p| words::page(p.key)))
            .h(px(PAGE_H))
            .px(px(14.0))
            .pt(px(10.0))
            .rounded(px(RADIUS))
            .number_of_lines(1)
            .text_size(px(15.0))
            .text_color(move || {
                crate::settings::read(ui);
                if is_chosen() {
                    rgb(ui.theme.with(|t| t.accent.light))
                } else if page().is_some_and(lit_page) {
                    rgb(WHITE)
                } else {
                    tint(WHITE, DIM_ALPHA)
                }
            })
            .bg(move || {
                if focused() {
                    accent_fill(ui)
                } else if is_chosen() {
                    tint(WHITE, RESTING_ALPHA)
                } else {
                    tint(WHITE, 0)
                }
            })
            .on_press(move || {
                ui.touched();
                let i = at.index();
                update(ui, move |screen, _| {
                    screen.tap_page(i);
                    Effect::None
                });
            })
    };
    virtual_list(|| PAGES.len(), entry)
        .estimated_item_size(px(PAGE_H))
        .element_ref(&list)
        .w(px(PAGES_W))
}

fn rows_list(ui: Ui, rows: Memo<Vec<Row>>) -> impl IntoElement {
    let list = element_ref();
    let focused_index = computed(move || {
        let key = state(ui, |s| {
            (s.pane() == Pane::Rows).then(|| s.focus()).flatten()
        })
        .flatten()?;
        rows.with(|r| r.iter().position(|row| row.key() == Some(key)))
    });
    effect(move || {
        if let Some(i) = focused_index.get()
            && ui.keys_shown()
        {
            list.scroll_to_index(i as u32, percentage(0.5), px(0.0), false);
        }
    });
    // Another page starts at its top.
    let page = computed(move || state(ui, Screen::page_index));
    effect(move || {
        page.get();
        list.scroll_to_index(0, percentage(0.0), px(0.0), false);
    });
    virtual_list(
        move || rows.with(Vec::len),
        move |at| row_view(ui, rows, at, focused_index),
    )
    .estimated_item_size(px(ROW_H))
    .element_ref(&list)
    .flex_1()
}

/// The colour of a palette value, for its swatch.
fn swatch_colour(ty: &ValueType, variant: &str) -> Option<u32> {
    let ValueType::Colour { palette, .. } = ty else {
        return None;
    };
    let key = |name: &str| crate::settings::palette_key(name);
    match palette.family {
        "accent" => crate::theme::ACCENTS
            .iter()
            .find(|a| key(a.name) == variant)
            .map(|a| a.main),
        _ => crate::theme::BACKGROUNDS
            .iter()
            .find(|b| key(b.name) == variant)
            .map(|b| b.panel),
    }
}

fn row_view(
    ui: Ui,
    rows: Memo<Vec<Row>>,
    at: Position,
    focused_index: Memo<Option<usize>>,
) -> impl IntoElement {
    let row = move || rows.with(|r| r.get(at.index()).copied());
    let heading = move || matches!(row(), Some(Row::Heading { .. }));
    let lit = move || {
        let settings = crate::settings::read(ui);
        row().is_some_and(|r| matches!(status(&settings, &r), Status::Active | Status::ReadOnly))
    };
    let focused = move || ui.keys_shown() && focused_index.get() == Some(at.index());
    let title = move || match row() {
        Some(Row::Heading {
            section,
            subsection,
        }) => words::heading(section, subsection),
        Some(r) => r.key().map_or("", words::label),
        None => "",
    };
    let note = move || {
        let settings = crate::settings::read(ui);
        row().map(|r| note(&settings, &r)).unwrap_or_default()
    };
    let current = move || {
        let settings = crate::settings::read(ui);
        match row() {
            Some(Row::Setting(s)) => {
                let value = settings.get(s.key).unwrap_or_else(|_| s.default_value());
                let swatch = match &value {
                    Value::Colour(v) => swatch_colour(&s.ty, v),
                    _ => None,
                };
                (words::value(s, &value), swatch)
            }
            Some(Row::Command(c)) => {
                ui.data_version.get();
                ui.library.version.get();
                (command_value(c.key), None)
            }
            _ => (String::new(), None),
        }
    };
    view()
        .h(move || px(if heading() { HEADING_H } else { ROW_H }))
        .px(px(14.0))
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .rounded(px(RADIUS))
        .bg(move || {
            if focused() {
                accent_fill(ui)
            } else {
                tint(WHITE, 0)
            }
        })
        .on_press(move || {
            ui.touched();
            if let Some(key) = row().and_then(|r| r.key()) {
                update(ui, |screen, ctx| screen.tap_row(key, ctx));
            }
        })
        .child(
            view()
                .flex_1()
                .gap(px(2.0))
                .child(
                    text(title)
                        .number_of_lines(1)
                        .pt(move || px(if heading() { 8.0 } else { 0.0 }))
                        .text_size(move || px(if heading() { 12.0 } else { 15.0 }))
                        .text_color(move || {
                            if heading() {
                                secondary()
                            } else if lit() {
                                rgb(WHITE)
                            } else {
                                tint(WHITE, DIM_ALPHA)
                            }
                        }),
                )
                .child(when(
                    move || !note().is_empty(),
                    move || {
                        text(note)
                            .number_of_lines(1)
                            .text_size(px(12.0))
                            .text_color(secondary())
                    },
                )),
        )
        .child(
            view()
                .w(px(14.0))
                .h(px(14.0))
                .rounded(px(3.0))
                .bg(move || current().1.map_or(tint(WHITE, 0), rgb))
                .opacity(move || if current().1.is_some() { 1.0_f32 } else { 0.0 }),
        )
        .child(
            text(move || current().0)
                .number_of_lines(1)
                .text_size(px(14.0))
                .text_color(move || {
                    if lit() {
                        rgb(ui.theme.with(|t| t.accent.light))
                    } else {
                        tint(WHITE, TEXT_DISABLED_ALPHA)
                    }
                }),
        )
}

fn footer(ui: Ui) -> impl IntoElement {
    let description = move || {
        let settings = crate::settings::read(ui);
        let ctx = Ctx {
            settings: &settings,
            offer: &crate::settings::offer,
        };
        state(ui, |s| match s.focused_row(&ctx) {
            Some(Row::Setting(st)) => st.summary.replace('`', ""),
            Some(Row::Command(c)) => words::description(c.key).unwrap_or(c.summary).to_owned(),
            _ => match counts(s.page()) {
                (_, 0) | (0, 1) => String::new(),
                (0, all) => format!("{all} settings here, none available yet"),
                (active, all) => format!("{active} of {all} settings here can be changed now"),
            },
        })
        .unwrap_or_default()
    };
    text(description)
        .h(px(40.0))
        .px(px(4.0))
        .number_of_lines(2)
        .text_size(px(13.0))
        .text_color(secondary())
}

#[cfg(test)]
mod tests {
    use iptv_settings::catalogue;

    use super::*;

    fn row(page: &str, key: &str, settings: &Settings) -> Row {
        let page = PAGES.iter().find(|p| p.key == page).unwrap();
        iptv_settings::screen::rows(
            page,
            &Ctx {
                settings,
                offer: &crate::settings::offer,
            },
        )
        .into_iter()
        .find(|r| r.key() == Some(key))
        .unwrap()
    }

    #[test]
    fn rows_say_why_they_do_nothing() {
        let s = Settings::new();
        let language = row("general", "general.language", &s);
        assert_eq!(note(&s, &language), "Not available yet");
        let resume = row("general", "general.resume_last_channel", &s);
        assert_eq!(note(&s, &resume), "");
        let pin = row("parental", "parental.live_enabled", &s);
        assert_eq!(note(&s, &pin), "In a later version");
    }

    #[test]
    fn every_setting_has_a_page_or_is_not_offered() {
        // Playlist settings live on the playlists screen.
        let placed = |section: Section| {
            PAGES
                .iter()
                .any(|p| p.parts.contains(&Part::Section(section)))
        };
        for section in Section::ALL {
            if section != Section::Playlists {
                assert!(placed(section), "{section:?} has no page");
            }
        }
        for key in ["playlists.manage", "general.about"] {
            assert!(catalogue::command(key).is_some(), "{key}");
        }
        for p in PAGES {
            assert!(!words::page(p.key).is_empty(), "{}", p.key);
        }
    }

    #[test]
    fn remote_keys_map_to_the_screen() {
        assert_eq!(screen_key(Action::MoveLeft), Some(Key::Left));
        assert_eq!(screen_key(Action::Select), Some(Key::Ok));
        assert_eq!(screen_key(Action::ChannelDown), Some(Key::PageDown));
        assert_eq!(screen_key(Action::Digit(3)), None);
        assert_eq!(screen_key(Action::ShowMenu), None);
    }

    #[test]
    fn pages_count_what_they_offer() {
        let playback = PAGES.iter().find(|p| p.key == "playback").unwrap();
        let (active, _) = counts(playback);
        // Panel timeouts, channel-change info, seek keys and three steps.
        assert_eq!(active, 7);
        let parental = PAGES.iter().find(|p| p.key == "parental").unwrap();
        assert_eq!(counts(parental).0, 0);
    }

    #[test]
    fn leaving_needs_a_second_back_in_time() {
        let t = Instant::now();
        let window = Duration::from_millis(2_000);
        assert!(!second_press(None, t, window));
        assert!(second_press(
            Some(t),
            t + Duration::from_millis(1_500),
            window
        ));
        assert!(!second_press(
            Some(t),
            t + Duration::from_millis(2_500),
            window
        ));
    }
}
