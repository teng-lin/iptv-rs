//! The playlists screen: the stored playlists (name, type, channel count,
//! last update or last failure, the one on screen marked), "Refresh all"
//! (with two or more) and "Add playlist". OK (or Menu, or a tap) on a
//! playlist opens its actions: show it, refresh it now, rename it, change
//! its provider details (link, server and login, portal and MAC address:
//! checked by importing them, kept only if that works), its User-Agent,
//! its guide link (or use only the provider's guide), delete it after a
//! confirmation that states what goes. `state.rs` and `details.rs` own
//! focus and flow; `data::library` does the work.
//!
//! Remote: Up/Down move, OK acts, Menu opens a playlist's actions, Back
//! closes the open sheet, then the screen. Touch and mouse: every row and
//! action is tappable; a tap beside a sheet closes it. Text entry uses the
//! wizard's sheet (`wizard_view::entry`). Guide links are shown with their
//! user info and query values hidden, and a link carrying them is never
//! put into the text field. Wording is iptv-rs's own.

mod details;
mod details_card;
mod state;
mod words;

use iptv_nav::Action;
use iptv_store_sqlite::SourceId;
use nsh_core::prelude::*;

pub use state::Screen as Open;
use state::{Act, EditKind, Effect, Item, Key, Layer, Screen};

use crate::data::{self, Activity, Playlist};
use crate::theme::{FOCUS_ALPHA, RADIUS, RADIUS_LARGE, RESTING_ALPHA, TEXT_SECONDARY_ALPHA};
use crate::wizard_view::entry::{self, TextSheet};
use crate::{Ui, ViewMode, tint};

const WHITE: u32 = 0xFFFFFF;
/// Placeholder error tone (light red of the placeholder palette).
const ERROR: u32 = 0xEF9A9A;
const ROW_H: f32 = 76.0;
const CARD_W: f32 = 580.0;

// --- Actions -------------------------------------------------------------------

/// Opens the screen (the navigation rail's Playlists).
pub fn open(ui: Ui) {
    if is_open(ui) || crate::wizard_view::is_open(ui) {
        return;
    }
    let screen = Screen::new(load(), ui.library.shown.get_untracked());
    ui.playlists.set(Some(screen));
    if ui.mode.get_untracked() == ViewMode::Fullscreen {
        ui.mode.set(ViewMode::Guide);
    }
}

pub fn close(ui: Ui) {
    if ui.playlists.with_untracked(Option::is_some) {
        ui.playlists.set(None);
        dismiss_keyboard();
    }
}

pub fn is_open(ui: Ui) -> bool {
    ui.playlists.with(Option::is_some)
}

fn load() -> Vec<Playlist> {
    data::playlists().unwrap_or_else(|e| {
        log::warn!("iptv playlists: the store is unavailable: {e}");
        Vec::new()
    })
}

fn state<R>(ui: Ui, f: impl FnOnce(&Screen) -> R) -> Option<R> {
    ui.playlists.with(|s| s.as_ref().map(f))
}

/// Changes the state and carries out what it answers.
fn update(ui: Ui, f: impl FnOnce(&mut Screen) -> Option<Effect>) {
    let mut effect = None;
    ui.playlists.update(|s| {
        if let Some(s) = s {
            effect = f(s);
        }
    });
    if let Some(effect) = effect {
        perform(ui, effect);
    }
}

fn editing(ui: Ui) -> bool {
    state(ui, |s| s.editing().is_some()).unwrap_or(false)
}

/// A remote action while the screen is open.
pub fn key(ui: Ui, action: Action) {
    let key = match action {
        Action::MoveUp => Key::Up,
        Action::MoveDown => Key::Down,
        Action::Select => Key::Ok,
        Action::Back => Key::Back,
        Action::ShowMenu => Key::Menu,
        _ => return,
    };
    if editing(ui) {
        // The input has the keys while focused; these come once it lost it.
        if matches!(key, Key::Ok | Key::Back) {
            commit(ui);
        }
        return;
    }
    update(ui, |s| s.key(key));
}

/// The platform's Back: ends text entry, closes a sheet, then the screen.
pub fn back(ui: Ui) -> bool {
    key(ui, Action::Back);
    true
}

fn commit(ui: Ui) {
    dismiss_keyboard();
    update(ui, Screen::commit);
}

fn cancel(ui: Ui) {
    dismiss_keyboard();
    update(ui, |s| {
        s.cancel_edit();
        None
    });
}

fn perform(ui: Ui, effect: Effect) {
    log::info!("iptv playlists: {}", effect_name(&effect));
    let result = match effect {
        Effect::Add => {
            crate::wizard_view::open_add(ui);
            Ok(())
        }
        Effect::Show(id) => {
            close(ui);
            data::switch_to(ui, id);
            Ok(())
        }
        Effect::Refresh(id) => {
            data::refresh(ui, id);
            Ok(())
        }
        Effect::Rename(id, name) => data::rename(ui, id, &name),
        Effect::SetGuide(id, link) => data::set_guide_link(ui, id, link),
        Effect::Delete(id) => data::delete(ui, id),
        Effect::RefreshAll => {
            data::refresh_all(ui);
            Ok(())
        }
        Effect::OpenDetails(id) => {
            if let Some(source) = data::source_details(id) {
                update(ui, |s| {
                    s.open_details(source);
                    None
                });
            }
            Ok(())
        }
        Effect::TryDetails(id, source) => {
            data::try_details(ui, id, source);
            Ok(())
        }
        Effect::SetUserAgent(id, agent) => {
            data::set_user_agent(ui, id, agent);
            Ok(())
        }
        Effect::Close => {
            close(ui);
            Ok(())
        }
    };
    if let Err(e) = result {
        log::warn!("iptv playlists: the store refused it: {e}");
    }
}

/// For the log: the effect without names or links.
fn effect_name(effect: &Effect) -> String {
    match effect {
        Effect::Add => "add".to_owned(),
        Effect::Show(id) => format!("show {}", id.0),
        Effect::Refresh(id) => format!("refresh {}", id.0),
        Effect::Rename(id, _) => format!("rename {}", id.0),
        Effect::SetGuide(id, link) => format!("guide link of {}: set {}", id.0, link.is_some()),
        Effect::Delete(id) => format!("delete {}", id.0),
        Effect::RefreshAll => "refresh all".to_owned(),
        Effect::OpenDetails(id) => format!("details of {}", id.0),
        Effect::TryDetails(id, _) => format!("try new details of {}", id.0),
        Effect::SetUserAgent(id, agent) => {
            format!("User-Agent of {}: set {}", id.0, agent.is_some())
        }
        Effect::Close => "close".to_owned(),
    }
}

// --- View ----------------------------------------------------------------------

fn secondary() -> Rgba {
    tint(WHITE, TEXT_SECONDARY_ALPHA)
}

fn focus_bg(ui: Ui, focused: bool) -> Rgba {
    if focused && ui.keys_shown() {
        tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
    } else {
        tint(WHITE, 0)
    }
}

pub fn layer(ui: Ui) -> impl IntoElement {
    // What is stored changed (refreshed, renamed, deleted, added): reload.
    effect(move || {
        ui.library.version.get();
        if ui.playlists.with_untracked(Option::is_some) {
            let rows = load();
            ui.playlists.update(|s| {
                if let Some(s) = s {
                    s.set_rows(rows);
                }
            });
        }
    });
    // New details checked: saved closes the form, refused keeps it.
    effect(move || {
        let ended = ui.library.activity.with(|a| {
            a.iter()
                .filter_map(|(id, act)| match act {
                    Activity::Done(Ok(_)) => Some((*id, true)),
                    Activity::Done(Err(_)) | Activity::Rejected(_) => Some((*id, false)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        untracked(|| {
            ui.playlists.update(|s| {
                if let Some(s) = s {
                    ended
                        .iter()
                        .for_each(|(id, saved)| s.details_checked(*id, *saved));
                }
            });
        });
    });
    let editing = computed(move || editing(ui));
    when(
        move || is_open(ui),
        move || {
            view()
                .absolute()
                .inset_0()
                .pt_safe_area()
                .px(px(48.0))
                .py(px(24.0))
                .gap(px(12.0))
                .bg(move || rgb(ui.theme.with(|t| t.background.window)))
                .child(header(ui))
                .child(
                    text(move || hint(ui))
                        .text_size(px(13.0))
                        .text_color(secondary()),
                )
                .child(list(ui))
                .child(sheet(ui))
                .child(when(move || editing.get(), move || edit_sheet(ui)))
        },
    )
}

fn hint(ui: Ui) -> &'static str {
    let layer = state(ui, Screen::layer).unwrap_or(Layer::List);
    match (ui.keys_shown(), layer) {
        (true, Layer::List) => "▲▼ choose   OK actions   Back close",
        (true, Layer::Actions { .. }) => "▲▼ choose   OK do it   Back close the sheet",
        (true, Layer::Confirm { .. }) => "▲▼ choose   OK confirm   Back keep it",
        (true, Layer::Details { .. }) => "▲▼ choose   OK change or check   Back leave unchanged",
        (false, _) => "Tap a playlist for what you can do with it",
    }
}

fn header(ui: Ui) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            text("Playlists")
                .flex_1()
                .text_size(px(22.0))
                .text_color(rgb(WHITE)),
        )
        .child(
            text("Close")
                .px(px(16.0))
                .py(px(8.0))
                .rounded(px(RADIUS))
                .text_size(px(14.0))
                .text_color(rgb(WHITE))
                .bg(tint(WHITE, RESTING_ALPHA * 2))
                .on_press(move || {
                    ui.touched();
                    close(ui);
                }),
        )
}

fn list(ui: Ui) -> impl IntoElement {
    let rows = element_ref();
    let count = move || state(ui, |s| s.items().len()).unwrap_or(0);
    let focused_index =
        computed(move || state(ui, |s| s.items().iter().position(|i| *i == s.focus())).flatten());
    effect(move || {
        if let Some(i) = focused_index.get()
            && ui.keys_shown()
        {
            rows.scroll_to_index(i as u32, percentage(0.5), px(0.0), false);
        }
    });
    virtual_list(count, move |at| row(ui, at.index(), focused_index))
        .estimated_item_size(px(ROW_H))
        .element_ref(&rows)
        .flex_1()
}

fn row(ui: Ui, index: usize, focused_index: Memo<Option<usize>>) -> impl IntoElement {
    let playlist = move || state(ui, |s| s.rows().get(index).cloned()).flatten();
    let item = move || state(ui, |s| s.items().get(index).copied()).flatten();
    let refreshing = move || {
        ui.library.activity.with(|a| {
            a.values()
                .filter(|act| matches!(act, Activity::Running(_) | Activity::Checking(_)))
                .count()
        })
    };
    let shown = move || playlist().is_some_and(|p| ui.library.shown.get() == Some(p.id));
    let activity = move |id: SourceId| ui.library.activity.with(|a| a.get(&id).cloned());
    let status = move || match (playlist(), item()) {
        (Some(p), _) => words::status(&p, activity(p.id).as_ref(), crate::time::wall_unix()),
        (None, Some(Item::RefreshAll)) => (words::refreshing(refreshing()), false),
        _ => (String::new(), false),
    };
    let title = move || match (playlist(), item()) {
        (Some(p), _) => p.name,
        (None, Some(Item::RefreshAll)) => "Refresh all playlists".to_owned(),
        _ => "+ Add playlist".to_owned(),
    };
    let summary = move || match (playlist(), item()) {
        (Some(p), _) => words::summary(&p),
        (None, Some(Item::RefreshAll)) => {
            "Each one downloads again; the one on screen updates in place".to_owned()
        }
        _ => "M3U link or file, Xtream Codes login, Stalker portal".to_owned(),
    };
    view()
        .h(px(ROW_H))
        .px(px(16.0))
        .flex_row()
        .items_center()
        .gap(px(14.0))
        .rounded(px(RADIUS))
        .bg(move || focus_bg(ui, focused_index.get() == Some(index)))
        .on_press(move || {
            ui.touched();
            if let Some(tapped) = item() {
                update(ui, |s| s.tap(tapped));
            }
        })
        .child(view().w(px(8.0)).h(px(40.0)).rounded(px(4.0)).bg(move || {
            if shown() {
                rgb(ui.theme.with(|t| t.accent.main))
            } else {
                tint(WHITE, 0)
            }
        }))
        .child(
            view()
                .flex_1()
                .gap(px(2.0))
                .child(
                    text(title)
                        .number_of_lines(1)
                        .text_size(px(17.0))
                        .text_color(rgb(WHITE)),
                )
                .child(
                    text(summary)
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .text_color(secondary()),
                )
                .child(
                    text(move || status().0)
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .text_color(move || if status().1 { rgb(ERROR) } else { secondary() }),
                ),
        )
        .child(
            text(move || if shown() { "On screen" } else { "" }.to_owned())
                .text_size(px(13.0))
                .text_color(move || rgb(ui.theme.with(|t| t.accent.light))),
        )
}

/// The open sheet (actions or delete confirmation) over the list; rebuilt
/// only when another sheet opens.
fn sheet(ui: Ui) -> impl IntoElement {
    // Which sheet, not where its focus is.
    let open = computed(move || match state(ui, Screen::layer) {
        Some(Layer::Actions { id, .. }) => Some((Sheet::Actions, id)),
        Some(Layer::Confirm { id, .. }) => Some((Sheet::Confirm, id)),
        Some(Layer::Details { id }) => Some((Sheet::Details, id)),
        Some(Layer::List) | None => None,
    });
    dynamic(move || {
        let shown = open.get();
        untracked(move || match shown {
            Some((Sheet::Actions, id)) => backdrop(ui, actions_card(ui, id).into_element()),
            Some((Sheet::Confirm, id)) => backdrop(ui, confirm_card(ui, id).into_element()),
            Some((Sheet::Details, id)) => backdrop(ui, details_card::card(ui, id).into_element()),
            None => view(),
        })
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sheet {
    Actions,
    Confirm,
    Details,
}

fn backdrop(ui: Ui, card: AnyElement) -> View {
    view()
        .absolute()
        .inset_0()
        .items_center()
        .justify_center()
        .child(
            view()
                .absolute()
                .inset_0()
                .bg(tint(0x000000, 0xA0))
                .on_press(move || {
                    ui.touched();
                    update(ui, Screen::dismiss);
                }),
        )
        .child(card)
}

fn card(ui: Ui) -> View {
    view()
        .w(px(CARD_W))
        .px(px(28.0))
        .py(px(24.0))
        .gap(px(6.0))
        .rounded(px(RADIUS_LARGE))
        .bg(move || rgb(ui.theme.with(|t| t.background.panel)))
        // Taps on the card stay on it.
        .on_press(|| {})
}

fn button(
    ui: Ui,
    label: impl IntoReactive<SharedString>,
    focused: impl Fn() -> bool + 'static,
    color: u32,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    text(label)
        .px(px(16.0))
        .py(px(10.0))
        .rounded(px(RADIUS))
        .text_size(px(16.0))
        .text_color(rgb(color))
        .bg(move || focus_bg(ui, focused()))
        .on_press(move || {
            ui.touched();
            on_press();
        })
}

fn actions_card(ui: Ui, id: SourceId) -> impl IntoElement {
    let playlist = move || state(ui, |s| s.row(id).cloned()).flatten();
    let running = move || {
        ui.library.activity.with(|a| {
            matches!(
                a.get(&id),
                Some(Activity::Running(_) | Activity::Checking(_))
            )
        })
    };
    let acts = state(ui, |s| s.acts(id)).unwrap_or_default();
    let notice = move || state(ui, |s| s.notice()).flatten();
    let act_row = move |act: Act| {
        let focused =
            move || state(ui, |s| s.layer() == Layer::Actions { id, focus: act }).unwrap_or(false);
        let label = move || {
            let shown = ui.library.shown.get() == Some(id);
            let kind = playlist().map(|p| p.kind);
            words::act(act, kind, shown, running()).to_owned()
        };
        let color = if act == Act::Delete { ERROR } else { WHITE };
        button(ui, label, focused, color, move || {
            update(ui, |s| s.act(id, act));
        })
    };
    card(ui)
        .child(
            text(move || playlist().map(|p| p.name).unwrap_or_default())
                .number_of_lines(1)
                .text_size(px(22.0))
                .text_color(rgb(WHITE)),
        )
        .child(
            text(move || playlist().map(|p| words::summary(&p)).unwrap_or_default())
                .text_size(px(14.0))
                .text_color(secondary()),
        )
        .child(
            text(move || {
                playlist()
                    .map(|p| words::guide_line(&p))
                    .unwrap_or_default()
            })
            .number_of_lines(1)
            .text_size(px(14.0))
            .text_color(secondary()),
        )
        .child(
            text(move || notice().map_or("", words::notice).to_owned())
                .text_size(px(14.0))
                .text_color(rgb(ERROR))
                .h(move || px(if notice().is_some() { 20.0 } else { 0.0 })),
        )
        .child(view().h(px(6.0)))
        .children(acts.into_iter().map(act_row))
}

fn confirm_card(ui: Ui, id: SourceId) -> impl IntoElement {
    let (title, body) = state(ui, |s| {
        let Some(p) = s.row(id) else {
            return (String::new(), String::new());
        };
        let next = s
            .rows()
            .iter()
            .rev()
            .find(|r| r.id != id)
            .map(|r| r.name.as_str());
        let shown = ui.library.shown.get_untracked() == Some(id);
        (
            format!("Delete “{}”?", p.name),
            words::delete_consequences(p, next, shown),
        )
    })
    .unwrap_or_default();
    let focused = move |delete: bool| {
        move || state(ui, |s| s.layer() == Layer::Confirm { id, delete }).unwrap_or(false)
    };
    card(ui)
        .child(text(title).text_size(px(22.0)).text_color(rgb(WHITE)))
        .child(text(body).text_size(px(15.0)).text_color(secondary()))
        .child(view().h(px(8.0)))
        .child(button(ui, "Keep it", focused(false), WHITE, move || {
            update(ui, |s| s.confirm(id, false));
        }))
        .child(button(ui, "Delete", focused(true), ERROR, move || {
            update(ui, |s| s.confirm(id, true));
        }))
}

fn edit_text(ui: Ui) -> String {
    state(ui, |s| s.editing().map(|e| e.text.clone()))
        .flatten()
        .unwrap_or_default()
}

fn set_edit_text(ui: Ui, text: &str) {
    ui.playlists.update(|s| {
        if let Some(s) = s {
            s.set_text(text);
        }
    });
}

/// The text sheet for a rename, a User-Agent, a guide link or a provider
/// detail; made when entry starts.
fn edit_sheet(ui: Ui) -> impl IntoElement {
    let Some((kind, prefilled, has_link)) = ui.playlists.with_untracked(|s| {
        let s = s.as_ref()?;
        let e = s.editing()?;
        let has_link = s.row(e.id).is_some_and(|r| r.guide_link.is_some());
        Some((e.kind, e.prefilled, has_link))
    }) else {
        return view();
    };
    let mut masked = false;
    let (label, placeholder, url) = match kind {
        EditKind::Rename => ("Playlist name", "How the playlist is listed", false),
        EditKind::UserAgent => (
            "User-Agent",
            "Empty: the app's own; some providers ask for a particular one",
            false,
        ),
        EditKind::Param(p) => {
            masked = p == details::Param::Password;
            (
                words::param(p),
                words::param_placeholder(p, prefilled),
                p.is_link(),
            )
        }
        // A link with an account in it is not shown: typing replaces it.
        EditKind::GuideLink if has_link && !prefilled => (
            "Guide link (XMLTV)",
            "A new link; left empty, the current one stays",
            true,
        ),
        EditKind::GuideLink => (
            "Guide link (XMLTV)",
            "http://…/guide.xml; empty: only the provider's guide",
            true,
        ),
    };
    entry::text_sheet(
        ui,
        TextSheet {
            label,
            placeholder,
            url,
            masked,
            text: edit_text,
            set_text: set_edit_text,
            commit,
            cancel,
        },
    )
}

#[cfg(test)]
mod tests {
    use iptv_providers::SecretUrl;

    use super::*;

    #[test]
    fn the_log_never_names_playlists_or_links() {
        let link = SecretUrl::new("http://guide.example.test/x.xml?password=p");
        let text = effect_name(&Effect::SetGuide(SourceId(3), Some(link)));
        assert_eq!(text, "guide link of 3: set true");
        let text = effect_name(&Effect::Rename(SourceId(3), "Private name".into()));
        assert!(!text.contains("Private"));
    }
}
