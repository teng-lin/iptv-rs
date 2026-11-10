//! The guide grid: a time bar, one virtualized row per channel of the list
//! on screen (`lists.rs`) with its programmes across the full remaining
//! width, and a now-line. Rows are numbered by their place in the list;
//! `at.index()` is a row, the catalog channel comes from the list.

use iptv_core::lists::{FAVORITES, RECENT};
use nsh_core::prelude::*;

use crate::guide::{Airing, Pane, Program, STEP, VISIBLE};
use crate::theme::{FOCUS_ALPHA, RADIUS, RESTING_ALPHA, TEXT_SECONDARY_ALPHA, TEXT_SIZE};
use crate::{Ui, channel_name, clock, tint};

pub const ROW_H: f32 = 56.0;
pub const NAME_W: f32 = 250.0;
/// The channel number before the logo.
const NUMBER_W: f32 = 30.0;
/// The logo box: missing or broken logos leave it empty.
const LOGO_W: f32 = 40.0;
const LOGO_H: f32 = 28.0;

/// Shown where a channel has no logo (an image needs a source).
fn no_logo() -> String {
    "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='1' height='1'/%3E"
        .to_owned()
}
/// Cells a row can hold: VISIBLE / shortest programme, plus one.
const MAX_CELLS: usize = 12;
const WHITE: u32 = 0xFFFFFF;

pub fn guide(ui: Ui, rows: ElementRef) -> impl IntoElement {
    // The programme area's width: the window always spans all of it.
    let width = signal(0.0_f32);
    let scale = computed(move || width.get() / VISIBLE as f32);
    let window = computed(move || ui.focus.with(|f| f.window));
    let now = computed(move || ui.focus.with(|f| f.now));
    let focused = computed(move || {
        ui.data_version.get();
        let view = crate::lists::view(ui);
        ui.focus
            .with(|f| (f.pane == Pane::Guide).then(|| (f.channel, f.program(&view).start)))
    });
    scroll_to_focus_on_new_list(ui, rows);
    view()
        .flex_1()
        .bg(move || ui.surface(ui.theme.with(|t| t.background.window)))
        .child(time_bar(ui, window, now, scale, width))
        .child(
            view()
                .flex_1()
                .overflow_hidden()
                .child(
                    virtual_list(
                        move || ui.list.with(|l| l.len()),
                        move |at| row(ui, at, window, now, scale, focused),
                    )
                    .estimated_item_size(px(ROW_H))
                    .element_ref(&rows)
                    .on_viewable_items_change(move |v: &ViewableItems| {
                        ui.visible_rows.set(v.viewable)
                    })
                    .flex_1(),
                )
                .child(
                    // Now, hidden while the window does not show it.
                    view()
                        .absolute()
                        .top(px(0.0))
                        .h_full()
                        .w(px(2.0))
                        .opacity(move || {
                            let at = now.get() - window.get();
                            if (0..=VISIBLE).contains(&at) {
                                1.0_f32
                            } else {
                                0.0
                            }
                        })
                        .left(move || px(NAME_W + (now.get() - window.get()) as f32 * scale.get()))
                        .bg(move || rgb(ui.theme.with(|t| t.accent.main)))
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
        ui.data_version.get();
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

fn time_bar(
    ui: Ui,
    window: Memo<i32>,
    now: Memo<i32>,
    scale: Memo<f32>,
    width: Signal<f32>,
) -> impl IntoElement {
    let shift = move |label: &'static str, minutes: i32| {
        text(label)
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(RADIUS))
            .bg(tint(WHITE, RESTING_ALPHA))
            .text_color(rgb(WHITE))
            .text_size(px(13.0))
            .on_press(move || {
                ui.touched();
                ui.focus.update(|f| f.shift_window(minutes));
            })
    };
    let tick = move |i: i32| {
        // The epoch comes with the data; now decides which day is "today".
        text(move || {
            ui.data_version.get();
            now.get();
            clock(window.get() + i * STEP)
        })
        .absolute()
        .left(move || px(i as f32 * STEP as f32 * scale.get() + 6.0))
        .text_size(px(12.0))
        .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA))
    };
    view()
        .h(px(36.0))
        .flex_row()
        .items_center()
        .child(
            view()
                .w(px(NAME_W))
                .flex_row()
                .gap(px(8.0))
                .px(px(12.0))
                .child(shift("‹ 30m", -STEP))
                .child(shift("30m ›", STEP)),
        )
        .child(
            view()
                .flex_1()
                .h_full()
                .justify_center()
                .on_layout(move |b: &Bounds<Points>| width.set(b.size.width.0))
                .children((0..VISIBLE / STEP).map(tick)),
        )
}

fn row(
    ui: Ui,
    at: Position,
    window: Memo<i32>,
    now: Memo<i32>,
    scale: Memo<f32>,
    focused: Memo<Option<(usize, i32)>>,
) -> impl IntoElement {
    // The catalog channel behind this row of the list on screen.
    let channel = computed(move || ui.list.with(|l| l.channel(at.index())));
    let programs = computed(move || {
        ui.data_version.get();
        channel.get().map_or_else(Vec::new, |ch| {
            crate::schedule().programs_between(ch, window.get(), VISIBLE)
        })
    });
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
        let airing = move || program().map(|p| p.airing(now.get()));
        view()
            .absolute()
            .top(px(3.0))
            .h(px(ROW_H - 6.0))
            .left(move || px(geometry().0 + 1.0))
            .w(move || px((geometry().1 - 2.0).max(0.0)))
            .opacity(move || if program().is_some() { 1.0_f32 } else { 0.0 })
            .rounded(px(RADIUS))
            .px(px(10.0))
            .justify_center()
            .overflow_hidden()
            .bg(move || {
                if is_focused() {
                    tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
                } else if airing() == Some(Airing::Live) {
                    tint(WHITE, RESTING_ALPHA * 2)
                } else {
                    tint(WHITE, RESTING_ALPHA)
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
                    ui.data_version.get();
                    match (channel.get(), program()) {
                        (Some(ch), Some(p)) => crate::schedule().title(ch, p),
                        _ => String::new(),
                    }
                })
                .number_of_lines(1)
                .text_size(px(TEXT_SIZE))
                .text_color(move || {
                    let alpha = if airing() == Some(Airing::Ended) {
                        TEXT_SECONDARY_ALPHA
                    } else {
                        0xFF
                    };
                    tint(WHITE, alpha)
                }),
            )
    };
    let playing = move || channel.get().is_some() && ui.playing.get() == channel.get();
    let logo = move || {
        ui.data_version.get();
        channel
            .get()
            .and_then(|ch| crate::schedule().channel_logo(ch))
            .unwrap_or_else(no_logo)
    };
    view()
        .h(px(ROW_H))
        .flex_row()
        .child(
            view()
                .w(px(NAME_W))
                .px(px(8.0))
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .on_press(move || {
                    ui.touched();
                    let view = crate::lists::view(ui);
                    ui.focus.update(|f| {
                        let now = f.now;
                        f.tap(at.index(), now, &view)
                    });
                    ui.watch();
                })
                .child(
                    // The place in the list on screen.
                    text(move || (at.index() + 1).to_string())
                        .w(px(NUMBER_W))
                        .number_of_lines(1)
                        .text_size(px(TEXT_SIZE))
                        .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA)),
                )
                .child(
                    view()
                        .w(px(LOGO_W))
                        .h(px(LOGO_H))
                        .items_center()
                        .justify_center()
                        .child(
                            image(logo)
                                .w(px(LOGO_W))
                                .h(px(LOGO_H))
                                .object_fit(ResizeMode::Contain),
                        ),
                )
                .child(
                    text(move || {
                        // Cells are made once per recycled slot: the index
                        // alone does not change when the data does.
                        ui.data_version.get();
                        let Some(ch) = channel.get() else {
                            return String::new();
                        };
                        let star = ui.favorites.with(|f| f.contains(&ch));
                        format!("{}{}", if star { "★ " } else { "" }, channel_name(ch))
                    })
                    .flex_shrink(1.0)
                    .number_of_lines(1)
                    .text_size(px(TEXT_SIZE))
                    .text_color(move || {
                        if playing() {
                            rgb(ui.theme.with(|t| t.accent.light))
                        } else {
                            rgb(WHITE)
                        }
                    }),
                )
                .child(
                    // The playing marker, after the name.
                    text(move || if playing() { "▶" } else { "" })
                        .text_size(px(12.0))
                        .text_color(move || rgb(ui.theme.with(|t| t.accent.light))),
                ),
        )
        .child(view().flex_1().children((0..MAX_CELLS).map(cell)))
}
