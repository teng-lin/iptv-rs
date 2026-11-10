//! The provider details form on screen: one row per detail (logins and MAC
//! addresses hidden), "Check and save", and what the check is doing or why
//! it refused the new details.

use iptv_store_sqlite::SourceId;
use nsh_core::prelude::*;

use super::details::Param;
use super::{ERROR, WHITE, button, secondary, state, update, words};
use crate::data::Activity;
use crate::{Ui, rgb};

pub(super) fn card(ui: Ui, id: SourceId) -> impl IntoElement {
    let Some((kind, params)) = state(ui, |s| s.details().map(|f| (f.kind(), f.params()))).flatten()
    else {
        return view();
    };
    let checking = move || state(ui, |s| s.details().is_some_and(|f| f.checking)).unwrap_or(false);
    // The latest first: a notice from the last key, then the check.
    let status = move || {
        if let Some(n) = state(ui, |s| s.notice()).flatten() {
            return (words::notice(n).to_owned(), true);
        }
        match ui.library.activity.with(|a| a.get(&id).cloned()) {
            Some(Activity::Checking(progress)) if checking() => (
                format!("Checking: {}", crate::wizard_view::words::stage(progress)),
                false,
            ),
            Some(Activity::Rejected(f)) if !checking() => (words::rejected(f), true),
            _ => (String::new(), false),
        }
    };
    let row = move |p: Param| {
        let focused = move || state(ui, |s| s.details().is_some_and(|f| f.focus == Some(p)));
        let value = move || {
            state(ui, |s| s.details().map(|f| words::param_value(f, p)))
                .flatten()
                .unwrap_or_default()
        };
        let error = move || {
            state(ui, |s| s.details().and_then(|f| f.error))
                .flatten()
                .filter(|(at, _)| *at == p)
                .map(|(_, e)| crate::wizard_view::words::field_error(e))
        };
        view()
            .px(px(16.0))
            .py(px(8.0))
            .gap(px(2.0))
            .rounded(px(crate::theme::RADIUS))
            .bg(move || super::focus_bg(ui, focused().unwrap_or(false)))
            .on_press(move || {
                ui.touched();
                update(ui, |s| s.edit_param(p));
            })
            .child(
                text(words::param(p))
                    .text_size(px(13.0))
                    .text_color(secondary()),
            )
            .child(
                text(value)
                    .number_of_lines(1)
                    .text_size(px(16.0))
                    .text_color(rgb(WHITE)),
            )
            .child(
                text(move || error().unwrap_or("").to_owned())
                    .text_size(px(12.0))
                    .text_color(rgb(ERROR))
                    .h(move || px(if error().is_some() { 16.0 } else { 0.0 })),
            )
    };
    let check_focused =
        move || state(ui, |s| s.details().is_some_and(|f| f.focus.is_none())).unwrap_or(false);
    let check_label = move || {
        if checking() {
            "Checking…"
        } else {
            "Check and save"
        }
        .to_owned()
    };
    super::card(ui)
        .child(
            text(words::details_title(kind))
                .text_size(px(22.0))
                .text_color(rgb(WHITE)),
        )
        .child(
            text("New details are tried first: the playlist keeps its old ones and its channels unless the new ones work.")
                .text_size(px(14.0))
                .text_color(secondary()),
        )
        .child(view().h(px(4.0)))
        .children(params.iter().copied().map(row))
        .child(button(ui, check_label, check_focused, WHITE, move || {
            update(ui, |s| s.check());
        }))
        .child(
            text(move || status().0)
                .text_size(px(14.0))
                .text_color(move || if status().1 { rgb(ERROR) } else { secondary() }),
        )
}
