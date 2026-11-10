//! The guide grid: a time ruler, one virtualized row per channel of the list
//! on screen (`lists.rs`) with its programmes across the full remaining
//! width, and a now-line. Rows are numbered by their place in the list;
//! `at.index()` is a row, the catalog channel comes from the list.
//!
//! The look: programmes are rounded cells on the window colour; the focused
//! programme is a light pill with dark text, the rest of its row lighter
//! cells with white titles, every other row dimmed (grey titles). Sizes are
//! dp, the same on TV, tablet and desktop (`fonts.rs` has the type).

mod channel;
mod ruler;

use iptv_core::lists::{FAVORITES, RECENT};
use nsh_core::prelude::*;

use crate::fonts::{CELL_TITLE, CONDENSED};
use crate::guide::{Pane, Program, VISIBLE};
use crate::theme::{BRIGHT_TEXT, FOCUSED_CELL, FOCUSED_TEXT, RADIUS, TEXT_SECONDARY_ALPHA};
use crate::{Ui, ViewMode, tint};

/// Row pitch; a programme's height (the rest is the gap between rows), the
/// gap between programmes, their corner radius and text inset.
pub const ROW_H: f32 = 58.0;
const CELL_H: f32 = 53.0;
const CELL_GAP: f32 = 3.0;
const CELL_RADIUS: f32 = 4.0;
const CELL_INSET: f32 = 16.0;
/// The channel column: its share of the window's width, within bounds.
const CHANNEL_SHARE: f32 = 0.40;
const CHANNEL_MIN: f32 = 260.0;
const CHANNEL_MAX: f32 = 640.0;
/// The now line's width.
const NOW_W: f32 = 2.0;

/// Cells a row can hold: VISIBLE / shortest programme, plus one.
const MAX_CELLS: usize = 12;
const WHITE: u32 = 0xFFFFFF;

/// The channel column's width now (reactive: the window's size).
pub fn channel_width() -> f32 {
    (window().width.0 * CHANNEL_SHARE).clamp(CHANNEL_MIN, CHANNEL_MAX)
}

/// An opaque guide colour; over full-screen video (overlay) at the
/// overlay's alpha (`appearance.ui_transparency`).
pub(crate) fn fill(ui: Ui, rgb_hex: u32) -> Rgba {
    let alpha = match ui.mode.get() {
        ViewMode::Overlay => crate::settings::overlay_alpha(&crate::settings::read(ui)),
        ViewMode::Guide | ViewMode::Fullscreen => 0xFF,
    };
    tint(rgb_hex, alpha)
}

pub fn guide(ui: Ui, rows: ElementRef) -> impl IntoElement {
    // The programme area's width: the window always spans all of it.
    let width = signal(0.0_f32);
    let scale = computed(move || width.get() / VISIBLE as f32);
    let window = computed(move || ui.focus.with(|f| f.window));
    let now = computed(move || ui.focus.with(|f| f.now));
    let focused = computed(move || {
        ui.guide_data();
        let view = crate::lists::view(ui);
        ui.focus
            .with(|f| (f.pane == Pane::Guide).then(|| (f.channel, f.program(&view).start)))
    });
    // Read where used: a memo of it went stale on window resizes.
    let column: fn() -> f32 = channel_width;
    // Rows are numbered by place: the list's length is its widest number.
    let number_w = computed(move || crate::numbers::column_width(ui.list.with(|l| l.len())));
    let now_x = computed(move || {
        let at = now.get() - window.get();
        let rows = ui.list.with(|l| !l.is_empty());
        (rows && (0..=VISIBLE).contains(&at)).then(|| at as f32 * scale.get())
    });
    scroll_to_focus_on_new_list(ui, rows);
    view()
        .flex_1()
        .bg(move || ui.surface(ui.theme.with(|t| t.background.window)))
        .child(ruler::ruler(ui, column, window, now, now_x, width))
        .child(
            view()
                .flex_1()
                .overflow_hidden()
                .child(
                    virtual_list(
                        move || ui.list.with(|l| l.len()),
                        move |at| row(ui, at, column, window, scale, focused, number_w),
                    )
                    .estimated_item_size(px(ROW_H))
                    .element_ref(&rows)
                    .on_viewable_items_change(move |v: &ViewableItems| {
                        ui.visible_rows.set(v.viewable)
                    })
                    .flex_1(),
                )
                .child(
                    // Now, hidden while the window does not show it (or no rows).
                    view()
                        .absolute()
                        .top(px(0.0))
                        .h_full()
                        .w(px(NOW_W))
                        .opacity(move || if now_x.get().is_some() { 1.0_f32 } else { 0.0 })
                        .left(move || px(column() + now_x.get().unwrap_or(0.0) - NOW_W / 2.0))
                        .bg(move || fill(ui, ui.theme.with(|t| t.guide().now_line)))
                        .pointer_events(PointerEvents::None),
                )
                .child(empty_state(ui))
                .child(hint_card(ui)),
        )
}

/// Over an empty list: what it would hold and how to fill it.
fn empty_state(ui: Ui) -> impl IntoElement {
    let empty = move || ui.list.with(|l| l.is_empty());
    let message = move || match ui.list.with(|l| l.entry()) {
        FAVORITES => {
            "No favourite channels yet.\nHold OK (or press Menu) on a channel and choose \
             “Add to favourites”, or star channels with “Manage favourites…”."
        }
        RECENT => "Channels you watch will appear here.",
        _ => "This group has no channels.",
    };
    view()
        .absolute()
        .inset_0()
        .items_center()
        .justify_center()
        .px(px(40.0))
        .opacity(move || if empty() { 1.0_f32 } else { 0.0 })
        .pointer_events(PointerEvents::None)
        .child(
            text(message)
                .text_size(px(16.0))
                .text_center()
                .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA)),
        )
}

/// After a group was chosen: what the keys do here, until a key or Back.
fn hint_card(ui: Ui) -> impl IntoElement {
    let shown = move || ui.focus.with(|f| f.hint) && ui.keys_shown();
    view()
        .absolute()
        .right(px(16.0))
        .bottom(px(16.0))
        .px(px(16.0))
        .py(px(12.0))
        .gap(px(4.0))
        .rounded(px(RADIUS))
        .bg(move || tint(ui.theme.with(|t| t.background.alt), 0xF0))
        .opacity(move || if shown() { 1.0_f32 } else { 0.0 })
        .pointer_events(PointerEvents::None)
        .children(
            [
                "Hold OK  ·  menu",
                "◀  ·  groups",
                "Hold ◀  ·  earlier programmes",
            ]
            .map(|line| text(line).text_size(px(13.0)).text_color(rgb(WHITE))),
        )
}

/// New guide data or a new list shows the focused row (the focus is kept
/// by identity, but the list keeps its scroll offset across a count
/// change).
fn scroll_to_focus_on_new_list(ui: Ui, list: ElementRef) {
    effect(move || {
        ui.guide_data();
        ui.list.with(|_| ());
        let row = ui.focus.with_untracked(|f| f.channel);
        list.scroll_to_index(
            row.saturating_sub(2) as u32,
            percentage(0.0),
            px(0.0),
            false,
        );
    });
}

fn row(
    ui: Ui,
    at: Position,
    column: fn() -> f32,
    window: Memo<i32>,
    scale: Memo<f32>,
    focused: Memo<Option<(usize, i32)>>,
    number_w: Memo<f32>,
) -> impl IntoElement {
    // The catalog channel behind this row of the list on screen.
    let channel = computed(move || ui.list.with(|l| l.channel(at.index())));
    let programs = computed(move || {
        ui.guide_data();
        channel.get().map_or_else(Vec::new, |ch| {
            crate::schedule().programs_between(ch, window.get(), VISIBLE)
        })
    });
    // The row with the focus: lighter cells, white titles.
    let lit = computed(move || focused.get().is_some_and(|(row, _)| row == at.index()));
    let cell = move |i: usize| {
        let program = move || programs.with(|ps| ps.get(i).copied());
        let geometry = move || {
            program().map_or((0.0, 0.0), |p: Program| {
                let from = window.get();
                let (start, end) = (p.start.max(from), p.end.min(from + VISIBLE));
                (
                    (start - from) as f32 * scale.get(),
                    (end - start) as f32 * scale.get(),
                )
            })
        };
        let is_focused = move || {
            program().is_some_and(|p| focused.get() == Some((at.index(), p.start)))
                && ui.keys_shown()
        };
        view()
            .absolute()
            .top(px((ROW_H - CELL_H) / 2.0))
            .h(px(CELL_H))
            .left(move || px(geometry().0 + CELL_GAP / 2.0))
            .w(move || px((geometry().1 - CELL_GAP).max(0.0)))
            .opacity(move || if program().is_some() { 1.0_f32 } else { 0.0 })
            .rounded(px(CELL_RADIUS))
            .px(px(CELL_INSET))
            .justify_center()
            .overflow_hidden()
            .bg(move || {
                let g = ui.theme.with(|t| t.guide());
                if is_focused() {
                    fill(ui, FOCUSED_CELL)
                } else if lit.get() {
                    fill(ui, g.row_cell)
                } else {
                    fill(ui, g.cell)
                }
            })
            .on_press(move || {
                let Some(p) = program() else { return };
                ui.touched();
                let view = crate::lists::view(ui);
                ui.focus.update(|f| f.tap(at.index(), p.start, &view));
                ui.watch();
            })
            .child(
                text(move || {
                    // Equal programme minutes on new data still re-read the title.
                    ui.guide_data();
                    match (channel.get(), program()) {
                        (Some(ch), Some(p)) => crate::schedule().title(ch, p),
                        _ => String::new(),
                    }
                })
                .number_of_lines(1)
                .text_overflow(nsh_core::EllipsizeMode::Clip)
                .font_family(CONDENSED)
                .text_size(px(CELL_TITLE))
                .text_color(move || {
                    if is_focused() {
                        rgb(FOCUSED_TEXT)
                    } else if lit.get() {
                        rgb(BRIGHT_TEXT)
                    } else {
                        tint(WHITE, TEXT_SECONDARY_ALPHA)
                    }
                }),
            )
    };
    view()
        .h(px(ROW_H))
        .flex_row()
        .child(channel::cell(ui, at, channel, column, number_w))
        .child(view().flex_1().children((0..MAX_CELLS).map(cell)))
}
