//! The guide's header: the preview at the top left (the video layer is laid
//! out under it in guide mode, `panels::preview_rect`) and, to its right,
//! the focused programme (title, times, minutes left, progress,
//! description, channel logo, number, name and group, favourite star) with
//! remote hints; at the right the list on screen (a tap opens the groups)
//! and Add playlist.

use iptv_core::lists::group_of;
use nsh_core::prelude::*;

use crate::guide::{Airing, Pane, Program};
use crate::panels::{HEADER_H, HEADER_PAD_X, HEADER_PAD_Y, PREVIEW_H, PREVIEW_W};
use crate::theme::{RADIUS, RESTING_ALPHA, TEXT_SECONDARY_ALPHA, TEXT_SIZE};
use crate::{Ui, channel_name, clock, tint};

const WHITE: u32 = 0xFFFFFF;
const PROGRESS_W: f32 = 240.0;

fn secondary() -> Rgba {
    tint(WHITE, TEXT_SECONDARY_ALPHA)
}

/// The focused programme: data version (a schedule swap re-renders even on
/// equal minutes), now, row, catalog channel, programme. `None` on an
/// empty list.
type Focused = Option<(u64, i32, usize, usize, Program)>;

/// Its times and how far along it is, in our words.
fn times(now: i32, p: Program) -> String {
    let span = format!("{}–{}", clock(p.start), clock(p.end));
    match p.airing(now) {
        Airing::Live => format!("{span} · {} min left", p.end - now),
        Airing::Upcoming => format!("{span} · in {} min", p.start - now),
        Airing::Ended => format!("{span} · ended"),
    }
}

fn preview(ui: Ui) -> impl IntoElement {
    // Transparent: the video layer sits under this rectangle in guide
    // mode. A tap opens full screen.
    view()
        .w(px(PREVIEW_W))
        .h(px(PREVIEW_H))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(tint(WHITE, RESTING_ALPHA * 2))
        .items_center()
        .justify_center()
        .on_press(move || {
            ui.touched();
            if ui.playing.get().is_some() {
                ui.mode.set(crate::ViewMode::Fullscreen);
            }
        })
        .child(
            text(move || {
                if ui.playing.get().is_none() {
                    "Nothing playing".to_owned()
                } else {
                    String::new()
                }
            })
            .text_size(px(TEXT_SIZE))
            .text_color(secondary()),
        )
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
    view()
        .flex_1()
        .gap(px(3.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(
                    text(move || {
                        program.with(|p| {
                            p.map_or_else(String::new, |(_, _, _, ch, p)| {
                                crate::schedule().title(ch, p)
                            })
                        })
                    })
                    .flex_shrink(1.0)
                    .number_of_lines(1)
                    .text_size(px(20.0))
                    .text_color(rgb(WHITE)),
                )
                .child(
                    text(move || if favourite() { "★" } else { "" })
                        .text_size(px(16.0))
                        .text_color(move || rgb(ui.theme.with(|t| t.accent.light))),
                ),
        )
        .child(
            text(move || {
                program.with(|p| p.map_or_else(String::new, |(_, now, _, _, p)| times(now, p)))
            })
            .number_of_lines(1)
            .text_size(px(13.0))
            .text_color(secondary()),
        )
        .child(
            // Progress of a live programme.
            view()
                .w(px(PROGRESS_W))
                .h(px(4.0))
                .my(px(2.0))
                .rounded(px(2.0))
                .bg(tint(WHITE, RESTING_ALPHA * 2))
                .opacity(move || if live() { 1.0_f32 } else { 0.0 })
                .child(
                    view()
                        .h_full()
                        .rounded(px(2.0))
                        .w(move || {
                            let done = program
                                .with(|p| p.map_or(0.0, |(_, now, _, _, p)| p.progress(now)));
                            px(PROGRESS_W * done)
                        })
                        .bg(move || rgb(ui.theme.with(|t| t.accent.main))),
                ),
        )
        .child(
            text(move || description.get().unwrap_or_default())
                .text_size(px(12.0))
                .number_of_lines(2)
                .text_color(secondary()),
        )
        .child(
            // The channel's logo, its place in the list, name and group.
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    image(move || {
                        let ch = program.with(|p| p.map(|p| p.3));
                        ch.and_then(|ch| crate::schedule().channel_logo(ch))
                            .unwrap_or_else(crate::grid::no_logo)
                    })
                    .w(px(30.0))
                    .h(px(20.0))
                    .object_fit(ResizeMode::Contain),
                )
                .child(
                    text(move || {
                        program.with(|p| {
                            let Some((_, _, row, ch, _)) = *p else {
                                return String::new();
                            };
                            let catalog = crate::schedule();
                            let group = group_of(&*catalog, ch).map(|g| catalog.group_name(g));
                            match group {
                                Some(g) => format!("{}  {} · {g}", row + 1, channel_name(ch)),
                                None => format!("{}  {}", row + 1, channel_name(ch)),
                            }
                        })
                    })
                    .flex_shrink(1.0)
                    .number_of_lines(1)
                    .text_size(px(13.0))
                    .text_color(rgb(WHITE)),
                ),
        )
        .child(
            text(move || match (ui.focus.with(|f| f.pane), ui.keys_shown()) {
                (Pane::Guide, true) => "OK watch   ◀ groups   Back groups",
                (Pane::Groups, true) => "OK choose   ▲▼ browse   ◀ sidebar   Back sidebar",
                (Pane::Rail, true) => "OK open   ▶ groups   Back exit",
                (_, false) => "Tap a programme to watch · tap the list name for groups",
            })
            .number_of_lines(1)
            .text_size(px(12.0))
            .text_color(secondary()),
        )
}

/// The list on screen (a tap opens the groups), Add playlist, and in
/// development the key stats ([`debug_overlay`]).
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
        .children(debug_overlay().then(|| {
            text(move || {
                ui.stats
                    .with(|s| format!("keys {} · {} µs (max {})", s.keys, s.last_us, s.max_us))
            })
            .text_size(px(11.0))
            .text_color(tint(WHITE, 0x60))
        }))
}

/// The key-timing line in the header is for development: debug builds, or
/// `IPTV_RS_DEBUG_OVERLAY` set (not empty, not `0`). The log line in
/// `input.rs` stays in every build (`tools/perf-android.sh` reads it).
pub fn debug_overlay() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        overlay_wanted(
            cfg!(debug_assertions),
            std::env::var("IPTV_RS_DEBUG_OVERLAY").ok().as_deref(),
        )
    })
}

fn overlay_wanted(debug_build: bool, env: Option<&str>) -> bool {
    debug_build || env.is_some_and(|v| !v.is_empty() && v != "0")
}

#[cfg(test)]
mod tests {
    use super::overlay_wanted;

    #[test]
    fn the_key_timing_line_is_for_development_only() {
        assert!(!overlay_wanted(false, None), "release builds hide it");
        assert!(!overlay_wanted(false, Some("")));
        assert!(!overlay_wanted(false, Some("0")));
        assert!(overlay_wanted(false, Some("1")), "unless asked for");
        assert!(overlay_wanted(true, None), "debug builds show it");
    }
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
        .bg(move || ui.surface(ui.theme.with(|t| t.background.panel)))
        .child(
            view()
                .h(px(HEADER_H))
                .px(px(HEADER_PAD_X))
                .py(px(HEADER_PAD_Y))
                .flex_row()
                .gap(px(18.0))
                .child(preview(ui))
                .child(details(ui, program))
                .child(side(ui)),
        )
}

/// The date and time at the left of the time bar.
pub fn date_time(ui: Ui) -> Text {
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
    .text_size(px(13.0))
    .text_color(rgb(WHITE))
}
