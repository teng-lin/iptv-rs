//! The regions around the guide: the context bar, the groups column (shown
//! with the rail while either has the focus) and the preview + programme
//! info panel with remote hints.

use iptv_core::lists::{entry_count, entry_name};
use nsh_core::prelude::*;

use crate::guide::{Airing, Pane};
use crate::theme::{FOCUS_ALPHA, RADIUS, RESTING_ALPHA, TEXT_SECONDARY_ALPHA, TEXT_SIZE};
use crate::{Ui, channel_name, clock, tint};

const WHITE: u32 = 0xFFFFFF;
pub const GROUPS_W: f32 = 220.0;

/// The groups column's width now (0 while the guide has the focus).
pub fn groups_width(ui: Ui) -> f32 {
    if crate::lists::columns_shown(ui) {
        GROUPS_W
    } else {
        0.0
    }
}
/// The top bar's height below the status bar.
pub const TOP_H: f32 = 44.0;
/// The info panel's height and padding, and the preview rectangle the video
/// layer occupies in guide mode (`video.rs` positions the player from these).
pub const INFO_H: f32 = 141.0;
pub const INFO_PAD_X: f32 = 20.0;
pub const INFO_PAD_Y: f32 = 12.0;
pub const PREVIEW_W: f32 = 208.0;
pub const PREVIEW_H: f32 = 117.0;

fn secondary() -> Rgba {
    tint(WHITE, TEXT_SECONDARY_ALPHA)
}

/// The opaque colours of the regions, drawn under the video layer with the
/// same geometry as the foreground (top bar, group column, guide, info panel).
/// The foreground stays clear in guide mode, so the video shows wherever it is
/// laid out: today the preview rectangle.
pub fn backdrop(ui: Ui) -> impl IntoElement {
    let panel = move || rgb(ui.theme.with(|t| t.background.panel));
    let window = move || rgb(ui.theme.with(|t| t.background.window));
    let alt = move || rgb(ui.theme.with(|t| t.background.alt));
    view()
        .absolute()
        .inset_0()
        .pointer_events(PointerEvents::None)
        .child(view().pt_safe_area().bg(panel).child(view().h(px(TOP_H))))
        .child(
            view()
                .flex_1()
                .flex_row()
                .child(view().w(move || px(crate::rail::width(ui))).bg(alt))
                .child(view().w(move || px(groups_width(ui))).bg(panel))
                .child(view().flex_1().bg(window)),
        )
        .child(view().h(px(INFO_H)).bg(panel))
}

pub fn top_bar(ui: Ui) -> impl IntoElement {
    view()
        .pt_safe_area()
        .bg(move || ui.surface(ui.theme.with(|t| t.background.panel)))
        .child(
            view()
                .h(px(TOP_H))
                .px(px(20.0))
                .flex_row()
                .items_center()
                .gap(px(20.0))
                .child(
                    // The list on screen; a tap opens the groups.
                    text(move || format!("☰  {}", crate::lists::list_name(ui)))
                        .px(px(10.0))
                        .py(px(6.0))
                        .rounded(px(RADIUS))
                        .bg(tint(WHITE, RESTING_ALPHA))
                        .text_size(px(16.0))
                        .text_color(rgb(WHITE))
                        .on_press(move || {
                            ui.touched();
                            crate::lists::open_groups(ui);
                        }),
                )
                .child(
                    text(move || {
                        ui.stats.with(|s| {
                            format!(
                                "keys {} · handler {} µs (max {})",
                                s.keys, s.last_us, s.max_us
                            )
                        })
                    })
                    .flex_1()
                    .text_size(px(12.0))
                    .text_color(secondary()),
                )
                .child(crate::wizard_view::add_button(ui))
                .child(
                    text(move || {
                        ui.data_version.get();
                        clock(ui.focus.with(|f| f.now))
                    })
                    .text_size(px(16.0))
                    .text_color(rgb(WHITE)),
                ),
        )
}

/// The groups column: Favourites, Recently watched, All channels, then the
/// playlist's groups. Moving over (or tapping) one shows its list.
pub fn group_column(ui: Ui, groups: ElementRef) -> impl IntoElement {
    // New data: the selected group stays in view.
    effect(move || {
        ui.data_version.get();
        let group = ui.focus.with_untracked(|f| f.group);
        groups.scroll_to_index(group as u32, percentage(0.5), px(0.0), false);
    });
    virtual_list(
        move || {
            ui.data_version.get();
            entry_count(&*crate::schedule())
        },
        move |at| {
            let current = move || ui.focus.with(|f| f.group) == at.index();
            let focused =
                move || current() && ui.focus.with(|f| f.pane) == Pane::Groups && ui.keys_shown();
            view()
                .h(px(44.0))
                .mx(px(8.0))
                .my(px(2.0))
                .px(px(12.0))
                .rounded(px(RADIUS))
                .justify_center()
                .bg(move || {
                    if focused() {
                        tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
                    } else if current() {
                        tint(WHITE, RESTING_ALPHA)
                    } else {
                        tint(WHITE, 0)
                    }
                })
                .on_press(move || {
                    ui.touched();
                    crate::lists::tap_entry(ui, at.index());
                })
                .child(
                    // Re-read on new data: the recycled cell keeps its index.
                    text(move || {
                        ui.data_version.get();
                        entry_name(at.index(), &*crate::schedule())
                    })
                    .number_of_lines(1)
                    .text_size(px(TEXT_SIZE))
                    .text_color(move || {
                        if focused() {
                            rgb(WHITE)
                        } else if current() {
                            rgb(ui.theme.with(|t| t.accent.light))
                        } else {
                            secondary()
                        }
                    }),
                )
        },
    )
    .estimated_item_size(px(48.0))
    .element_ref(&groups)
    .w(move || px(groups_width(ui)))
    .py(px(6.0))
    .bg(move || ui.surface(ui.theme.with(|t| t.background.panel)))
}

pub fn info_panel(ui: Ui) -> impl IntoElement {
    // The data version is part of the value, so a schedule swap re-renders
    // even when the focused channel and programme minutes are unchanged.
    // The focused programme by catalog channel (`None`: an empty list).
    let program = computed(move || {
        let version = ui.data_version.get();
        let view = crate::lists::view(ui);
        ui.focus.with(|f| {
            let p = f.program(&view);
            view.channel(f.channel).map(|ch| (version, f.now, ch, p))
        })
    });
    // Read once per focused programme (a stored guide reads it from disk).
    let description = computed(move || {
        program.with(|p| {
            let (_, _, ch, p) = (*p)?;
            let schedule = crate::schedule();
            schedule
                .as_any()
                .and_then(|s| s.downcast_ref::<iptv_guide::EpgSchedule>())
                .and_then(|s| s.description(ch, p))
        })
    });
    // Transparent: the video layer sits under this rectangle in guide mode.
    // A tap opens full screen.
    let preview = view()
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
        );
    let details = view()
        .flex_1()
        .gap(px(4.0))
        .child(
            text(move || {
                program.with(|p| {
                    p.map_or_else(String::new, |(_, _, ch, p)| crate::schedule().title(ch, p))
                })
            })
            .text_size(px(20.0))
            .text_color(rgb(WHITE)),
        )
        .child(
            text(move || {
                program.with(|p| {
                    let Some((_, now, ch, p)) = *p else {
                        return String::new();
                    };
                    let state = match p.airing(now) {
                        Airing::Live => "Live",
                        Airing::Upcoming => "Upcoming",
                        Airing::Ended => "Ended",
                    };
                    format!(
                        "{} · {}–{} · {}",
                        channel_name(ch),
                        clock(p.start),
                        clock(p.end),
                        state
                    )
                })
            })
            .text_size(px(TEXT_SIZE))
            .text_color(secondary()),
        )
        .child(
            // Progress of a live programme.
            view()
                .w(px(260.0))
                .h(px(4.0))
                .rounded(px(2.0))
                .bg(tint(WHITE, RESTING_ALPHA * 2))
                .opacity(move || {
                    let live = program
                        .with(|p| p.is_some_and(|(_, now, _, p)| p.airing(now) == Airing::Live));
                    if live { 1.0_f32 } else { 0.0 }
                })
                .child(
                    view()
                        .h_full()
                        .rounded(px(2.0))
                        .w(move || {
                            let done =
                                program.with(|p| p.map_or(0.0, |(_, now, _, p)| p.progress(now)));
                            px(260.0 * done)
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
            text(move || match (ui.focus.with(|f| f.pane), ui.keys_shown()) {
                (Pane::Guide, true) => "OK watch   ◀ groups   Back groups",
                (Pane::Groups, true) => "OK choose   ▲▼ browse   ◀ sidebar   Back sidebar",
                (Pane::Rail, true) => "OK open   ▶ groups   Back exit",
                (_, false) => "Tap a programme to watch · tap the list name for groups",
            })
            .text_size(px(12.0))
            .text_color(secondary()),
        );
    view()
        .h(px(INFO_H))
        .px(px(INFO_PAD_X))
        .py(px(INFO_PAD_Y))
        .flex_row()
        .gap(px(20.0))
        .bg(move || ui.surface(ui.theme.with(|t| t.background.panel)))
        .child(preview)
        .child(details)
}
