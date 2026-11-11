//! A row's channel: its place in the list (right-aligned), its logo in a
//! box, and its name, cut with two dots where it is too long. The playing
//! channel's number, name and ▶ marker take the accent.

use nsh_core::prelude::*;

use super::{CELL_H, CELL_RADIUS, fill};
use crate::fonts::{CHANNEL, SANS};
use crate::scale::Scale;
use crate::theme::BRIGHT_TEXT;
use crate::{Ui, channel_name};

/// The logo box, as tall as a programme: missing or broken logos leave
/// it empty (sizes at full size, scaled with the grid).
const LOGO_W: f32 = 60.0;
/// Inside the logo box around the logo.
const LOGO_PAD: f32 = 4.0;
/// Before the number, between the parts, after the name.
const PAD: f32 = 8.0;
const GAP: f32 = 14.0;
/// The ▶ marker after the playing channel's name (its text is empty on
/// the other rows, but still takes a gap).
const MARKER_W: f32 = 20.0;

/// Shown where a channel has no logo (an image needs a source).
pub(crate) fn no_logo() -> String {
    "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='1' height='1'/%3E"
        .to_owned()
}

/// The width the name has in a column `column` wide after a number
/// `number` wide (`numbers.rs`), at size scale `scale`.
fn name_width(column: f32, number: f32, playing: bool, scale: f32) -> f32 {
    let marker = if playing { MARKER_W } else { 0.0 };
    let logo = (LOGO_W * scale).round();
    column - number - logo - scale * (2.0 * PAD + 3.0 * GAP + marker)
}

pub(super) fn cell(
    ui: Ui,
    at: Position,
    channel: Memo<Option<usize>>,
    (column, size): (fn() -> f32, Scale),
    number_w: Memo<f32>,
) -> impl IntoElement {
    let playing = move || channel.get().is_some() && ui.playing.get() == channel.get();
    let colour = move || {
        if playing() {
            rgb(ui.theme.with(|t| t.guide().playing))
        } else {
            rgb(BRIGHT_TEXT)
        }
    };
    let logo = move || {
        ui.guide_data();
        channel
            .get()
            .and_then(|ch| crate::schedule().channel_logo(ch))
            .unwrap_or_else(no_logo)
    };
    view()
        .w(move || px(column()))
        .px(move || size.px(PAD))
        .flex_row()
        .items_center()
        .gap(move || size.px(GAP))
        .on_press(move || {
            ui.touched();
            if crate::grid::closed_columns(ui) {
                return;
            }
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
                .w(move || px(number_w.get()))
                .number_of_lines(1)
                .text_right()
                .font_family(SANS)
                .text_size(move || size.px(CHANNEL))
                .text_color(colour),
        )
        .child(
            view()
                .w(move || px(size.whole(LOGO_W)))
                .h(move || px(size.whole(CELL_H)))
                .p(move || size.px(LOGO_PAD))
                .rounded(move || size.px(CELL_RADIUS))
                .bg(move || fill(ui, ui.theme.with(|t| t.guide().cell)))
                .items_center()
                .justify_center()
                .child(
                    image(logo)
                        .w(move || px(size.whole(LOGO_W) - 2.0 * size.of(LOGO_PAD)))
                        .h(move || px(size.whole(CELL_H) - 2.0 * size.of(LOGO_PAD)))
                        .object_fit(ResizeMode::Contain),
                ),
        )
        .child(
            text(move || {
                // Cells are made once per recycled slot: the index
                // alone does not change when the data does.
                ui.guide_data();
                let Some(ch) = channel.get() else {
                    return String::new();
                };
                let star = ui.favorites.with(|f| f.contains(&ch));
                let name = format!("{}{}", if star { "★ " } else { "" }, channel_name(ch));
                let scale = size.get();
                crate::fonts::fit(
                    &name,
                    name_width(column(), number_w.get(), playing(), scale),
                    CHANNEL * scale,
                )
            })
            .flex_shrink(1.0)
            .number_of_lines(1)
            .font_family(SANS)
            .text_size(move || size.px(CHANNEL))
            .text_color(colour),
        )
        .child(
            // The playing marker, after the name.
            text(move || if playing() { "▶" } else { "" })
                .text_size(move || size.px(CHANNEL * 0.6))
                .text_color(move || rgb(ui.theme.with(|t| t.guide().playing))),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_have_the_room_the_column_leaves() {
        // A 1280 dp window: 36 % is 461 dp; three digits take about 42.
        assert_eq!(
            name_width(461.0, 42.0, false, 1.0),
            461.0 - 16.0 - 42.0 - 60.0 - 3.0 * 14.0
        );
        assert_eq!(
            name_width(461.0, 42.0, true, 1.0),
            name_width(461.0, 42.0, false, 1.0) - MARKER_W
        );
        // Scaled: the logo in whole px, the gaps with it.
        assert_eq!(
            name_width(461.0, 42.0, false, 0.875),
            461.0 - 42.0 - 53.0 - 0.875 * (16.0 + 3.0 * 14.0)
        );
    }
}
