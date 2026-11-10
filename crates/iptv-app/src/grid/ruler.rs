//! The time ruler over the grid: the date and time (light accent) and the
//! touch steps of the time window over the channel column, the half-hour
//! slot times (grey) over the programmes, the now line's dot, and a thin
//! line under it all.

use nsh_core::prelude::*;

use super::{NOW_W, fill};
use crate::fonts::{RULER, SANS};
use crate::guide::{STEP, VISIBLE};
use crate::scale::Scale;
use crate::theme::{RESTING_ALPHA, TEXT_SECONDARY_ALPHA};
use crate::{Ui, clock, tint};

/// The ruler's height, the line under it and the now dot's diameter (the
/// height, dot and inset at full size, scaled with the grid).
pub const RULER_H: f32 = 36.0;
const SEPARATOR_H: f32 = 1.0;
const DOT: f32 = 8.0;
/// A slot time's inset from its half hour.
const TICK_INSET: f32 = 8.0;
const WHITE: u32 = 0xFFFFFF;

pub(super) fn ruler(
    ui: Ui,
    size: Scale,
    column: fn() -> f32,
    window: Memo<i32>,
    now: Memo<i32>,
    now_x: Memo<Option<f32>>,
    width: Signal<f32>,
) -> impl IntoElement {
    let scale = move || width.get() / VISIBLE as f32;
    let shift = move |label: &'static str, minutes: i32| {
        text(label)
            .px(move || size.px(10.0))
            .rounded(move || size.px(4.0))
            .bg(tint(WHITE, RESTING_ALPHA))
            .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA))
            .font_family(SANS)
            .text_size(move || size.px(RULER))
            .on_press(move || {
                ui.touched();
                ui.focus.update(|f| f.shift_window(minutes));
            })
    };
    let tick = move |i: i32| {
        // The epoch comes with the data; now decides which day is "today".
        text(move || {
            ui.guide_data();
            now.get();
            clock(window.get() + i * STEP)
        })
        .absolute()
        .left(move || px(i as f32 * STEP as f32 * scale() + size.of(TICK_INSET)))
        .number_of_lines(1)
        .font_family(SANS)
        .text_size(move || size.px(RULER))
        .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA))
    };
    let separator = move || fill(ui, ui.theme.with(|t| t.guide().separator));
    view()
        .child(
            view()
                .h(move || px(size.whole(RULER_H)))
                .flex_row()
                .items_center()
                .child(
                    // The date and time, then the touch steps of the window.
                    view()
                        .w(move || px(column()))
                        .flex_row()
                        .items_center()
                        .gap(move || size.px(6.0))
                        .px(move || size.px(16.0))
                        .child(date_time(ui, size).flex_1())
                        .child(shift("‹", -STEP))
                        .child(shift("›", STEP)),
                )
                .child(
                    view()
                        .flex_1()
                        .h_full()
                        .justify_center()
                        .on_layout(move |b: &Bounds<Points>| width.set(b.size.width.0))
                        .children((0..VISIBLE / STEP).map(tick))
                        .child(
                            // Where the now line meets the ruler.
                            view()
                                .absolute()
                                .bottom(move || size.px(-DOT / 2.0))
                                .w(move || size.px(DOT))
                                .h(move || size.px(DOT))
                                .rounded(move || size.px(DOT / 2.0))
                                .left(move || px(now_x.get().unwrap_or(0.0) - size.of(DOT) / 2.0))
                                .opacity(move || if now_x.get().is_some() { 1.0_f32 } else { 0.0 })
                                .bg(move || fill(ui, ui.theme.with(|t| t.guide().now_line)))
                                .pointer_events(PointerEvents::None),
                        ),
                ),
        )
        .child(view().h(px(SEPARATOR_H)).bg(separator))
        .child(
            // The now line through the separator, so it meets the dot.
            view()
                .absolute()
                .bottom(px(0.0))
                .h(move || px(SEPARATOR_H + size.of(DOT) / 2.0))
                .w(px(NOW_W))
                .left(move || px(column() + now_x.get().unwrap_or(0.0) - NOW_W / 2.0))
                .opacity(move || if now_x.get().is_some() { 1.0_f32 } else { 0.0 })
                .bg(move || fill(ui, ui.theme.with(|t| t.guide().now_line)))
                .pointer_events(PointerEvents::None),
        )
}

/// The date and time at the left of the ruler, in the light accent.
pub fn date_time(ui: Ui, size: Scale) -> Text {
    text(move || {
        ui.guide_data();
        let now = ui.focus.with(|f| f.now);
        let unix = crate::guide::unix_of(&*crate::schedule(), now);
        format!(
            "{}  {}",
            iptv_core::clock::date(unix, crate::time::offset_minutes(unix)),
            clock(now)
        )
    })
    .number_of_lines(1)
    .font_family(SANS)
    .text_size(move || size.px(RULER))
    .text_color(move || rgb(ui.theme.with(|t| t.guide().ruler_date)))
}
