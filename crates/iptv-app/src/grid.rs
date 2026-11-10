//! The guide grid: a time bar, one virtualized row per channel with its
//! programmes across the full remaining width, and a now-line.

use nsh_core::prelude::*;

use crate::guide::{Airing, Pane, Program, STEP, VISIBLE};
use crate::theme::{FOCUS_ALPHA, RADIUS, RESTING_ALPHA, TEXT_SECONDARY_ALPHA, TEXT_SIZE};
use crate::{Ui, channel_name, clock, tint};

pub const ROW_H: f32 = 56.0;
pub const NAME_W: f32 = 170.0;
/// The channel number before the name.
const NUMBER_W: f32 = 36.0;
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
        ui.focus.with(|f| {
            (f.pane == Pane::Guide).then(|| (f.channel, f.program(&*crate::schedule()).start))
        })
    });
    scroll_to_top_on_new_data(ui, rows);
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
                        move || {
                            ui.data_version.get();
                            crate::schedule().channel_count()
                        },
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
                ),
        )
}

/// New guide data starts at its first row (the focus is reset to it, but
/// the list keeps its scroll offset across a count change).
pub(crate) fn scroll_to_top_on_new_data(ui: Ui, list: ElementRef) {
    let seen = std::cell::Cell::new(ui.data_version.get_untracked());
    effect(move || {
        let version = ui.data_version.get();
        if seen.replace(version) != version {
            list.scroll_to_index(0, percentage(0.0), px(0.0), false);
        }
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
    let programs = computed(move || {
        ui.data_version.get();
        crate::schedule().programs_between(at.index(), window.get(), VISIBLE)
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
                ui.focus
                    .update(|f| f.tap(at.index(), p.start, &*crate::schedule()));
                ui.watch();
            })
            .child(
                text(move || {
                    // Equal programme minutes on new data still re-read the title.
                    ui.data_version.get();
                    program().map_or(String::new(), |p| crate::schedule().title(at.index(), p))
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
    let playing = move || ui.playing.get() == Some(at.index());
    view()
        .h(px(ROW_H))
        .flex_row()
        .child(
            view()
                .w(px(NAME_W))
                .px(px(10.0))
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .on_press(move || {
                    ui.touched();
                    ui.focus.update(|f| {
                        let now = f.now;
                        f.tap(at.index(), now, &*crate::schedule())
                    });
                    ui.watch();
                })
                .child(
                    text(move || {
                        ui.data_version.get();
                        crate::schedule().channel_number(at.index()).to_string()
                    })
                    .w(px(NUMBER_W))
                    .number_of_lines(1)
                    .text_size(px(TEXT_SIZE))
                    .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA)),
                )
                .child(
                    text(move || {
                        // Cells are made once per recycled slot: the index
                        // alone does not change when the data does.
                        ui.data_version.get();
                        let star = ui.favorites.with(|f| f.contains(&at.index()));
                        format!(
                            "{}{}",
                            if star { "★ " } else { "" },
                            channel_name(at.index())
                        )
                    })
                    .flex_1()
                    .number_of_lines(1)
                    .text_size(px(TEXT_SIZE))
                    .text_color(move || {
                        if playing() {
                            rgb(ui.theme.with(|t| t.accent.light))
                        } else {
                            rgb(WHITE)
                        }
                    }),
                ),
        )
        .child(view().flex_1().children((0..MAX_CELLS).map(cell)))
}
