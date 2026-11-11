//! The remote's keys and what they do: the settings screen's read-only
//! key list, in a side panel.

use std::rc::Rc;

use iptv_settings::screen::Editor;
use nsh_core::prelude::*;

use super::{KEYS_W, backdrop, editor, entry_mark, side, title};
use crate::list_rows::{self, Mark, ROW};
use crate::scale::Scale;
use crate::settings_view::remote_keys;
use crate::settings_view::view::secondary;
use crate::theme::LIST_TEXT;
use crate::ui_kit;

/// The remote's keys and what they do (read-only).
pub(super) fn key_list(ui: crate::Ui) -> impl IntoElement {
    let size = Scale::new();
    let lines = remote_keys::key_lines();
    let count = lines.len();
    let lines = Rc::new(lines);
    let list = element_ref();
    let focus = computed(move || {
        editor(ui, |e| match e {
            Editor::List { focus, .. } => Some(*focus),
            _ => None,
        })
        .flatten()
    });
    effect(move || {
        if let Some(i) = focus.get() {
            list.scroll_to_index(i as u32, percentage(0.5), px(0.0), false);
        }
    });
    let entry = move |at: Position| {
        let lines = lines.clone();
        let line = move || lines.get(at.index()).cloned();
        let heading = {
            let line = line.clone();
            move || line().is_some_and(|l| l.is_heading())
        };
        let (key, action) = (line.clone(), line);
        let mark = move || entry_mark(ui, focus.get() == Some(at.index()));
        list_rows::row(size, ROW, mark)
            .flex_row()
            .items_center()
            .justify_start()
            .gap(move || size.px(12.0))
            .child(
                list_rows::name(size, move || key().map(|l| l.key).unwrap_or_default())
                    .flex_1()
                    .text_color(move || match (heading(), mark()) {
                        (_, Mark::Focused) => Mark::Focused.text(),
                        (true, _) => secondary(),
                        (false, _) => rgb(LIST_TEXT),
                    }),
            )
            .child(
                list_rows::second(
                    size,
                    move || action().map(|l| l.action).unwrap_or_default(),
                    crate::fonts::SANS,
                )
                .text_color(move || match mark() {
                    Mark::Focused => Mark::Focused.secondary(),
                    _ => rgb(ui.theme.with(|t| t.accent.light)),
                }),
            )
    };
    view()
        .absolute()
        .inset_0()
        .flex_row()
        .justify_end()
        .child(backdrop(ui))
        .child(
            side(ui, size, KEYS_W)
                .child(title(ui, size))
                .child(
                    ui_kit::caption(size, "Use Edit remote bindings to change a key.")
                        .px(move || size.px(14.0)),
                )
                .child(
                    virtual_list(move || count, entry)
                        .estimated_item_size(list_rows::pitch(size, ROW))
                        .element_ref(&list)
                        .flex_1(),
                ),
        )
}
