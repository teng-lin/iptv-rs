//! The favourites editor overlay for one group (from the menu's "Manage
//! favourites…"): a star per channel, OK toggles, Left removes the whole
//! group, Right adds it, Back leaves; persistent hints and, for touch, a
//! tap per row plus "Add all" / "Remove all" buttons.

use iptv_core::editor::FavoritesEditor;
use nsh_core::prelude::*;

use crate::theme::{FOCUS_ALPHA, RADIUS, RESTING_ALPHA, TEXT_SECONDARY_ALPHA};
use crate::{Ui, tint};

const WHITE: u32 = 0xFFFFFF;
const ROW_H: f32 = 52.0;

/// Opens the editor on the focused group.
pub fn open(ui: Ui) {
    let s = crate::schedule();
    let group = ui.focus.with(|f| f.group);
    let first = s.group_start(group);
    let end = if group + 1 < s.group_count() {
        s.group_start(group + 1)
    } else {
        s.channel_count()
    };
    ui.editor.set(Some(FavoritesEditor::new(
        group,
        first,
        end.saturating_sub(first),
    )));
}

pub fn close(ui: Ui) {
    ui.editor.set(None);
}

pub fn move_focus(ui: Ui, step: isize) {
    ui.editor
        .update(|e| e.iter_mut().for_each(|e| e.move_focus(step)));
}

fn with_favorites(
    ui: Ui,
    f: impl FnOnce(&FavoritesEditor, &mut std::collections::BTreeSet<usize>),
) {
    if let Some(editor) = ui.editor.get() {
        ui.favorites.update(|fav| f(&editor, fav));
    }
}

pub fn toggle(ui: Ui) {
    with_favorites(ui, |e, fav| e.toggle(fav));
}

pub fn clear_group(ui: Ui) {
    with_favorites(ui, |e, fav| e.clear_group(fav));
}

pub fn set_group(ui: Ui) {
    with_favorites(ui, |e, fav| e.set_group(fav));
}

pub fn layer(ui: Ui) -> impl IntoElement {
    let open = move || ui.editor.with(Option::is_some);
    let list = element_ref();
    let focus = computed(move || ui.editor.with(|e| e.as_ref().map(|e| e.focus)));
    effect(move || {
        if let Some(i) = focus.get() {
            list.scroll_to_index(i as u32, percentage(0.5), px(0.0), false);
        }
    });
    let count = move || ui.editor.with(|e| e.as_ref().map_or(0, |e| e.len));
    let title = move || {
        ui.editor.with(|e| {
            e.as_ref().map_or(String::new(), |e| {
                let starred = ui.favorites.with(|f| e.starred(f));
                format!(
                    "Favourites · {} · {starred} of {}",
                    crate::schedule().group_name(e.group),
                    e.len
                )
            })
        })
    };
    let button = move |label: &'static str, act: fn(Ui)| {
        text(label)
            .px(px(16.0))
            .py(px(8.0))
            .rounded(px(RADIUS))
            .text_size(px(14.0))
            .text_color(rgb(WHITE))
            .bg(tint(WHITE, RESTING_ALPHA * 2))
            .on_press(move || {
                ui.touched();
                act(ui);
            })
    };
    let row = move |at: Position| {
        let channel = move || ui.editor.with(|e| e.as_ref().map(|e| e.first + at.index()));
        let starred = move || channel().is_some_and(|ch| ui.favorites.with(|f| f.contains(&ch)));
        let focused = move || ui.keys_shown() && focus.get() == Some(at.index());
        view()
            .h(px(ROW_H))
            .px(px(16.0))
            .flex_row()
            .items_center()
            .gap(px(16.0))
            .rounded(px(RADIUS))
            .bg(move || {
                if focused() {
                    tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
                } else {
                    tint(WHITE, 0)
                }
            })
            .on_press(move || {
                ui.touched();
                ui.editor
                    .update(|e| e.iter_mut().for_each(|e| e.focus = at.index()));
                toggle(ui);
            })
            .child(
                text(move || if starred() { "★" } else { "☆" })
                    .w(px(24.0))
                    .text_size(px(20.0))
                    .text_color(move || {
                        if starred() {
                            rgb(ui.theme.with(|t| t.accent.light))
                        } else {
                            tint(WHITE, TEXT_SECONDARY_ALPHA)
                        }
                    }),
            )
            .child(
                text(move || {
                    channel().map_or(String::new(), |ch| crate::schedule().channel_name(ch))
                })
                .number_of_lines(1)
                .text_size(px(16.0))
                .text_color(rgb(WHITE)),
            )
    };
    view()
        .absolute()
        .inset_0()
        .pt_safe_area()
        .px(px(48.0))
        .py(px(24.0))
        .gap(px(12.0))
        .bg(move || rgb(ui.theme.with(|t| t.background.window)))
        .opacity(move || if open() { 1.0_f32 } else { 0.0 })
        .pointer_events(move || {
            if open() {
                PointerEvents::Auto
            } else {
                PointerEvents::None
            }
        })
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .child(
                    text(title)
                        .flex_1()
                        .text_size(px(20.0))
                        .text_color(rgb(WHITE)),
                )
                .child(button("Add all", set_group))
                .child(button("Remove all", clear_group))
                .child(button("Done", close)),
        )
        .child(
            text(move || {
                if ui.keys_shown() {
                    "OK add or remove   ◀ remove all in group   ▶ add all in group   Back done"
                } else {
                    "Tap a channel to add or remove it"
                }
            })
            .text_size(px(13.0))
            .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA)),
        )
        .child(
            virtual_list(count, row)
                .estimated_item_size(px(ROW_H))
                .element_ref(&list)
                .flex_1(),
        )
}
