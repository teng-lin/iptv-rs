//! Search and groups in one channel picker over the multiview session.
//! The portable picker owns selection, filtering and remote focus; the
//! guide's list and the persistent player surfaces stay in place.

use iptv_core::channel_picker::{ChannelPicker, Pane};
use iptv_core::lists::{Entry, ListId, entry_count, entry_name};
use iptv_nav::Action;
use nsh_base::input;
use nsh_core::prelude::*;

use crate::fonts::CONDENSED;
use crate::list_rows::{self, Mark, ROW, ROW_TWO};
use crate::scale::Scale;
use crate::{Ui, clock, ui_kit};

pub fn is_open(ui: Ui) -> bool {
    ui.channel_picker.with(Option::is_some)
}

fn state<T>(ui: Ui, f: impl FnOnce(&ChannelPicker) -> T) -> Option<T> {
    ui.channel_picker.with(|p| p.as_ref().map(f))
}

pub fn open(ui: Ui) {
    let catalog = crate::schedule();
    let group = ListId::of(ui.list.with_untracked(|l| l.entry()), &*catalog);
    let channel = crate::multiview::pick_from(ui).and_then(|c| catalog.channel_key(c));
    ui.channel_picker
        .set(Some(ChannelPicker::open(group, channel)));
    refresh(ui);
}

pub fn close(ui: Ui) {
    ui.channel_picker.set(None);
    crate::multiview::list_closed(ui);
}

fn update(ui: Ui, f: impl FnOnce(&mut ChannelPicker)) {
    ui.channel_picker.update(|p| {
        if let Some(p) = p {
            f(p);
        }
    });
}

fn refresh(ui: Ui) {
    let catalog = crate::schedule();
    let favorites = crate::favorites::favorite_channels();
    let recent = crate::favorites::recent_channels();
    update(ui, |p| p.refresh(&*catalog, &favorites, &recent));
}

fn type_query(ui: Ui, query: &str) {
    if state(ui, |p| p.query() == query) == Some(false) {
        update(ui, |p| p.type_query(query.to_owned()));
        refresh(ui);
    }
}

fn choose(ui: Ui, row: usize) {
    update(ui, |p| p.focus_row(row));
    if let Some(channel) = state(ui, ChannelPicker::selected).flatten() {
        crate::multiview::picked(ui, channel);
        close(ui);
    }
}

fn group(ui: Ui, entry: usize, toggle: bool) {
    let catalog = crate::schedule();
    update(ui, |p| p.group(entry, &*catalog));
    if toggle && let Entry::Group(g) = Entry::at(entry) {
        crate::data::combined::toggle_playlist(ui, g);
    }
    refresh(ui);
}

pub fn key(ui: Ui, action: Action) {
    match action {
        Action::MoveUp | Action::MoveDown | Action::PageUp | Action::PageDown => {
            let by = match action {
                Action::MoveUp => -1,
                Action::MoveDown => 1,
                Action::PageUp => -8,
                _ => 8,
            };
            let catalog = crate::schedule();
            let before = state(ui, |p| p.entry(&*catalog));
            update(ui, |p| p.vertical(by, &*catalog));
            if state(ui, |p| p.entry(&*catalog)) != before {
                refresh(ui);
            }
        }
        Action::MoveLeft | Action::MoveRight => {
            update(ui, |p| p.horizontal(action == Action::MoveRight));
        }
        Action::Select => match state(ui, |p| p.pane) {
            Some(Pane::Search) => update(ui, ChannelPicker::edit),
            Some(Pane::Clear) => type_query(ui, ""),
            Some(Pane::Close) => close(ui),
            Some(Pane::Channels) => {
                if let Some(row) = state(ui, ChannelPicker::row).flatten() {
                    choose(ui, row);
                }
            }
            Some(Pane::Groups) => {
                let catalog = crate::schedule();
                if let Some(entry) = state(ui, |p| p.entry(&*catalog)) {
                    if matches!(Entry::at(entry), Entry::Group(g) if catalog.group_is_playlist(g)) {
                        group(ui, entry, true);
                    } else {
                        update(ui, ChannelPicker::results);
                    }
                }
            }
            None => {}
        },
        Action::Back => {
            let mut close_picker = false;
            update(ui, |p| close_picker = p.back());
            if close_picker {
                close(ui);
            } else {
                refresh(ui);
            }
        }
        Action::OpenSearch => update(ui, ChannelPicker::edit),
        _ => {}
    }
}

fn focused(ui: Ui, pane: Pane) -> bool {
    state(ui, |p| p.pane == pane).unwrap_or(false)
}

pub fn layer(ui: Ui) -> impl IntoElement {
    effect(move || {
        ui.data_version.get();
        if untracked(|| is_open(ui)) {
            untracked(|| refresh(ui));
        }
    });
    effect(move || {
        let picking = ui
            .multiview
            .with(|m| m.as_ref().is_some_and(|m| m.pick().is_some()));
        if !picking && ui.channel_picker.with_untracked(Option::is_some) {
            ui.channel_picker.set(None);
        }
    });
    when(move || is_open(ui), move || screen(ui))
}

fn screen(ui: Ui) -> impl IntoElement {
    let size = Scale::new();
    let rows = element_ref();
    let groups = element_ref();
    let row = computed(move || state(ui, ChannelPicker::row).flatten());
    let entry = computed(move || {
        ui.data_version.get();
        state(ui, |p| p.entry(&*crate::schedule())).unwrap_or(0)
    });
    effect(move || {
        if let Some(row) = row.get() {
            rows.scroll_to_index(row as u32, percentage(0.5), px(0.0), false);
        }
    });
    effect(move || {
        groups.scroll_to_index(entry.get() as u32, percentage(0.5), px(0.0), false);
    });
    ui_kit::screen(ui, size)
        .test_id("multiview-picker")
        .child(header(ui, size))
        .child(ui_kit::caption(size, move || {
            if ui.keys_shown() {
                "▲▼ move   ◀▶ groups / channels   OK choose   Back clear search or close"
            } else {
                "Choose a group or search its channels, then tap a channel"
            }
        }))
        .child(
            view().flex_row().flex_1().gap(move || size.px(20.0))
                .child(
                    view().w(move || size.px(240.0)).gap(move || size.px(8.0))
                        .child(ui_kit::heading(size, "Groups"))
                        .child(
                            virtual_list(
                                move || { ui.data_version.get(); entry_count(&*crate::schedule()) },
                                move |at| group_row(ui, at, size, entry),
                            ).element_ref(&groups).estimated_item_size(list_rows::pitch(size, ROW)).flex_1(),
                        ),
                )
                .child(
                    view().flex_1().gap(move || size.px(8.0))
                        .child(ui_kit::heading(size, move || {
                            ui.data_version.get();
                            let catalog = crate::schedule();
                            state(ui, |p| {
                                let count = p.channels().len();
                                let unit = if count == 1 { "channel" } else { "channels" };
                                format!("{} · {count} {unit}", entry_name(p.entry(&*catalog), &*catalog))
                            }).unwrap_or_default()
                        }))
                        .child(when(
                            move || state(ui, |p| p.channels().is_empty()).unwrap_or(false),
                            move || ui_kit::body(size, move || {
                                if state(ui, |p| p.query().trim().is_empty()).unwrap_or(true) {
                                    "No channels in this group. Choose another group."
                                } else {
                                    "No matching channels. Clear the search or choose another group."
                                }
                            }),
                        ))
                        .child(
                            virtual_list(
                                move || state(ui, |p| p.channels().len()).unwrap_or(0),
                                move |at| channel_row(ui, at, size, row),
                            ).element_ref(&rows).estimated_item_size(list_rows::pitch(size, ROW_TWO)).flex_1(),
                        ),
                ),
        )
}

fn header(ui: Ui, size: Scale) -> impl IntoElement {
    let field = element_ref();
    let editing =
        computed(move || state(ui, |p| p.keyboard && p.pane == Pane::Search).unwrap_or(false));
    effect(move || {
        if editing.get() {
            field.focus();
        } else {
            field.blur();
        }
    });
    let was_up = std::cell::Cell::new(false);
    effect(move || {
        let up = keyboard_height() > px(0.0);
        if was_up.replace(up) && !up {
            update(ui, ChannelPicker::keyboard_hidden);
        }
    });
    view()
        .flex_row()
        .items_center()
        .gap(move || size.px(12.0))
        .child(ui_kit::title(size, move || {
            ui.multiview.with(|m| {
                if m.as_ref().is_some_and(|m| {
                    matches!(m.pick(), Some(iptv_core::multiview::Pick::Replace(_)))
                }) {
                    "Change channel"
                } else {
                    "Add screen"
                }
            })
        }))
        .child(
            ui_kit::field(size, input())
                .element_ref(&field)
                .flex_1()
                .test_id("multiview-channel-search")
                .value(move || state(ui, |p| p.query().to_owned()).unwrap_or_default())
                .placeholder("Search this group by channel or number")
                .auto_capitalize(AutoCapitalize::None)
                .auto_correct(false)
                .return_key_type(ReturnKeyType::Search)
                .bg(move || ui_kit::field_fill(ui, focused(ui, Pane::Search)))
                .border(move || size.px(2.0))
                .border_color(move || ui_kit::field_border(ui, focused(ui, Pane::Search)))
                .on_change(move |text: &str| type_query(ui, text))
                .on_submit(move |_: &str| update(ui, ChannelPicker::results))
                .on_focus(move || update(ui, ChannelPicker::edit))
                .on_blur(move || update(ui, ChannelPicker::keyboard_hidden)),
        )
        .child(ui_kit::button(
            ui,
            size,
            "Clear",
            move || focused(ui, Pane::Clear),
            move || type_query(ui, ""),
        ))
        .child(ui_kit::button(
            ui,
            size,
            "Close",
            move || focused(ui, Pane::Close),
            move || close(ui),
        ))
}

fn group_row(ui: Ui, at: Position, size: Scale, entry: Memo<usize>) -> impl IntoElement {
    let mark = move || {
        Mark::of(
            ui.keys_shown() && focused(ui, Pane::Groups) && entry.get() == at.index(),
            entry.get() == at.index(),
        )
    };
    list_rows::row(size, ROW, mark)
        .on_press(move || {
            ui.touched();
            group(ui, at.index(), true);
        })
        .child(
            list_rows::name(size, move || {
                ui.data_version.get();
                crate::lists::browser_entry_name(at.index())
            })
            .text_color(move || mark().text()),
        )
}

fn channel_row(ui: Ui, at: Position, size: Scale, row: Memo<Option<usize>>) -> impl IntoElement {
    let channel = move || state(ui, |p| p.channels().get(at.index()).cloned()).flatten();
    let mark = move || {
        Mark::of(
            ui.keys_shown() && focused(ui, Pane::Channels) && row.get() == Some(at.index()),
            false,
        )
    };
    list_rows::row(size, ROW_TWO, mark)
        .on_press(move || {
            ui.touched();
            choose(ui, at.index());
        })
        .child(
            list_rows::name(size, move || {
                channel().map_or(String::new(), |c| format!("{}  {}", c.number, c.name))
            })
            .text_color(move || mark().text()),
        )
        .child(
            list_rows::second(
                size,
                move || {
                    let Some(channel) = channel() else {
                        return String::new();
                    };
                    let now = ui.focus.with(|f| f.now);
                    let catalog = crate::schedule();
                    let programme = catalog.program_at(channel.index, now);
                    format!(
                        "{}  {}",
                        clock(programme.start),
                        catalog.title(channel.index, programme)
                    )
                },
                CONDENSED,
            )
            .text_color(move || mark().secondary()),
        )
}
