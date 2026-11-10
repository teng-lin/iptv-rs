//! The guide's header: the preview at the top left (the video layer is laid
//! out under it in guide mode, `panels::preview_rect`) and, to its right,
//! the focused programme: its title (Roboto Medium) with the favourite star
//! at the right, its times with a progress bar and the minutes left (the
//! group's name at the right), and its description in grey. At the right
//! the list on screen (a tap opens the groups) and Add playlist.

use iptv_core::lists::group_of;
use nsh_core::prelude::*;

use crate::fonts::{HEADER_BODY, HEADER_TIMES, HEADER_TITLE, MEDIUM, SANS};
use crate::guide::{Airing, Program};
use crate::panels::{PREVIEW_MARGIN, PREVIEW_RADIUS, header_now};
use crate::theme::{BRIGHT_TEXT, RADIUS, RESTING_ALPHA, TEXT_SECONDARY_ALPHA};
use crate::{Ui, ViewMode, clock, tint};

const WHITE: u32 = 0xFFFFFF;
/// The progress bar on the times line.
const PROGRESS_W: f32 = 64.0;
const PROGRESS_H: f32 = 4.0;
/// Lines of description under the times.
const BODY_LINES: u32 = 4;
/// The opaque ring that rounds the video's corners in the preview.
const RING: f32 = 12.0;

fn secondary() -> Rgba {
    tint(WHITE, TEXT_SECONDARY_ALPHA)
}

/// The focused programme: data version (a schedule swap re-renders even on
/// equal minutes), now, row, catalog channel, programme. `None` on an
/// empty list.
type Focused = Option<(u64, i32, usize, usize, Program)>;

/// Its times, in our words.
fn span(p: Program) -> String {
    format!("{} – {}", clock(p.start), clock(p.end))
}

/// How far along it is, in our words.
fn left(now: i32, p: Program) -> String {
    match p.airing(now) {
        Airing::Live => format!("{} min left", p.end - now),
        Airing::Upcoming => format!("in {} min", p.start - now),
        Airing::Ended => "ended".to_owned(),
    }
}

fn preview(ui: Ui) -> impl IntoElement {
    // Transparent while something plays: the video layer sits under this
    // rectangle in guide mode. A tap opens full screen.
    let idle = move || ui.playing.get().is_none();
    view()
        .w(move || px(header_now().1))
        .h(move || px(header_now().2))
        .rounded(px(PREVIEW_RADIUS))
        .bg(move || {
            let cell = ui.theme.with(|t| t.guide().cell);
            if idle() {
                crate::grid::fill(ui, cell)
            } else {
                tint(cell, 0)
            }
        })
        .items_center()
        .justify_center()
        .on_press(move || {
            ui.touched();
            if ui.playing.get().is_some() {
                ui.mode.set(crate::ViewMode::Fullscreen);
            }
        })
        .child(
            text(move || if idle() { "Nothing playing" } else { "" })
                .font_family(SANS)
                .text_size(px(HEADER_TIMES))
                .text_color(secondary()),
        )
}

/// Rounds the video's corners: a ring of the window colour whose inner edge
/// is the preview's rounded rectangle (guide mode only).
fn preview_ring(ui: Ui) -> impl IntoElement {
    view()
        .absolute()
        .left(px(PREVIEW_MARGIN - RING))
        .top(px(PREVIEW_MARGIN - RING))
        .w(move || px(header_now().1 + 2.0 * RING))
        .h(move || px(header_now().2 + 2.0 * RING))
        .border(px(RING))
        .rounded(px(PREVIEW_RADIUS + RING))
        .border_color(move || rgb(ui.theme.with(|t| t.background.window)))
        .opacity(move || {
            let shown = ui.mode.get() == ViewMode::Guide && ui.playing.get().is_some();
            if shown { 1.0_f32 } else { 0.0 }
        })
        .pointer_events(PointerEvents::None)
}

fn details(ui: Ui, program: Memo<Focused>) -> impl IntoElement {
    // Read once per focused programme (a stored guide reads it from disk).
    let description = computed(move || {
        program.with(|p| {
            let (_, _, _, ch, p) = (*p)?;
            crate::schedule()
                .as_any()
                .and_then(|s| s.downcast_ref::<iptv_guide::EpgSchedule>())
                .and_then(|s| s.description(ch, p))
        })
    });
    let favourite = move || {
        program
            .with(|p| p.map(|p| p.3))
            .is_some_and(|ch| ui.favorites.with(|f| f.contains(&ch)))
    };
    let live =
        move || program.with(|p| p.is_some_and(|(_, now, _, _, p)| p.airing(now) == Airing::Live));
    let group = move || {
        program.with(|p| {
            let (_, _, _, ch, _) = (*p)?;
            let catalog = crate::schedule();
            group_of(&*catalog, ch).map(|g| catalog.group_name(g))
        })
    };
    view()
        .flex_1()
        .gap(px(6.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .child(
                    text(move || {
                        program.with(|p| {
                            p.map_or_else(String::new, |(_, _, _, ch, p)| {
                                crate::schedule().title(ch, p)
                            })
                        })
                    })
                    .flex_1()
                    .number_of_lines(1)
                    .font_family(SANS)
                    .font_weight(MEDIUM)
                    .text_size(px(HEADER_TITLE))
                    .text_color(rgb(WHITE)),
                )
                .child(
                    text(move || if favourite() { "★" } else { "" })
                        .text_size(px(HEADER_TITLE * 0.8))
                        .text_color(move || rgb(ui.theme.with(|t| t.accent.light))),
                ),
        )
        .child(
            // Times, progress and minutes left; the group at the right.
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .child(
                    text(move || program.with(|p| p.map(|p| span(p.4)).unwrap_or_default()))
                        .number_of_lines(1)
                        .font_family(SANS)
                        .text_size(px(HEADER_TIMES))
                        .text_color(rgb(BRIGHT_TEXT)),
                )
                .child(
                    view()
                        .w(move || px(if live() { PROGRESS_W } else { 0.0 }))
                        .h(px(PROGRESS_H))
                        .rounded(px(PROGRESS_H / 2.0))
                        .bg(tint(WHITE, RESTING_ALPHA * 3))
                        .child(
                            view()
                                .h_full()
                                .rounded(px(PROGRESS_H / 2.0))
                                .w(move || {
                                    let done = program.with(|p| {
                                        p.map_or(0.0, |(_, now, _, _, p)| p.progress(now))
                                    });
                                    px(if live() { PROGRESS_W * done } else { 0.0 })
                                })
                                .bg(move || rgb(ui.theme.with(|t| t.accent.main))),
                        ),
                )
                .child(
                    text(move || {
                        program
                            .with(|p| p.map(|(_, now, _, _, p)| left(now, p)).unwrap_or_default())
                    })
                    .number_of_lines(1)
                    .font_family(SANS)
                    .text_size(px(HEADER_TIMES))
                    .text_color(secondary()),
                )
                .child(
                    text(move || group().unwrap_or_default())
                        .flex_1()
                        .number_of_lines(1)
                        .text_right()
                        .font_family(SANS)
                        .text_size(px(HEADER_TIMES))
                        .text_color(secondary()),
                ),
        )
        .child(
            text(move || description.get().unwrap_or_default())
                .number_of_lines(BODY_LINES)
                .font_family(SANS)
                .text_size(px(HEADER_BODY))
                .text_color(secondary()),
        )
}

/// The list on screen (a tap opens the groups), Add playlist, key stats.
fn side(ui: Ui) -> impl IntoElement {
    view()
        .items_end()
        .gap(px(8.0))
        .child(
            text(move || format!("☰  {}", crate::lists::list_name(ui)))
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(RADIUS))
                .bg(tint(WHITE, RESTING_ALPHA))
                .number_of_lines(1)
                .text_size(px(15.0))
                .text_color(rgb(WHITE))
                .on_press(move || {
                    ui.touched();
                    crate::lists::open_groups(ui);
                }),
        )
        .child(crate::wizard_view::add_button(ui))
        .child(view().flex_1())
        .child(
            text(move || {
                ui.stats
                    .with(|s| format!("keys {} · {} µs (max {})", s.keys, s.last_us, s.max_us))
            })
            .text_size(px(11.0))
            .text_color(tint(WHITE, 0x60)),
        )
}

pub fn header(ui: Ui) -> impl IntoElement {
    let program = computed(move || {
        let version = ui.guide_data();
        let view = crate::lists::view(ui);
        ui.focus.with(|f| {
            let p = f.program(&view);
            view.channel(f.channel)
                .map(|ch| (version, f.now, f.channel, ch, p))
        })
    });
    view()
        .pt_safe_area()
        .bg(move || ui.surface(ui.theme.with(|t| t.background.window)))
        .child(
            view()
                .h(move || px(header_now().0))
                .p(px(PREVIEW_MARGIN))
                .flex_row()
                .gap(px(24.0))
                .child(preview(ui))
                .child(details(ui, program))
                .child(side(ui))
                .child(preview_ring(ui)),
        )
}
