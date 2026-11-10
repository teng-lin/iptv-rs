//! The regions around the guide: the context bar, the group column and
//! the preview + programme info panel with remote hints.

use nsh_core::prelude::*;

use crate::guide::{Airing, GROUPS, Pane};
use crate::theme::{FOCUS_ALPHA, RADIUS, RESTING_ALPHA, TEXT_SECONDARY_ALPHA, TEXT_SIZE};
use crate::{Ui, channel_name, clock, tint};

const WHITE: u32 = 0xFFFFFF;
pub const GROUPS_W: f32 = 200.0;
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
    view()
        .absolute()
        .inset_0()
        .pointer_events(PointerEvents::None)
        .child(view().pt_safe_area().bg(panel).child(view().h(px(TOP_H))))
        .child(
            view()
                .flex_1()
                .flex_row()
                .child(view().w(px(GROUPS_W)).bg(panel))
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
                    text(move || ui.focus.with(|f| format!("Group {:02}", f.group + 1)))
                        .text_size(px(16.0))
                        .text_color(rgb(WHITE)),
                )
                .child(
                    text(move || ui.stats.with(|s| format!("keys {} · handler {} µs (max {})", s.keys, s.last_us, s.max_us)))
                        .flex_1()
                        .text_size(px(12.0))
                        .text_color(secondary()),
                )
                .child(text(clock(0)).text_size(px(16.0)).text_color(rgb(WHITE))),
        )
}

pub fn group_column(ui: Ui, groups: ElementRef) -> impl IntoElement {
    virtual_list(GROUPS, move |at| {
        let current = move || ui.focus.with(|f| f.group) == at.index();
        let focused = move || current() && ui.focus.with(|f| f.pane) == Pane::Groups && ui.keys_shown();
        view()
            .h(px(48.0))
            .mx(px(8.0))
            .my(px(2.0))
            .px(px(12.0))
            .rounded(px(RADIUS))
            .justify_center()
            .bg(move || {
                if focused() { tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA) } else { tint(WHITE, 0) }
            })
            .on_press(move || {
                ui.touched();
                ui.focus.update(|f| f.tap_group(at.index()));
            })
            .child(
                text(move || format!("Group {:02}", at.index() + 1))
                    .text_size(px(TEXT_SIZE))
                    .text_color(move || {
                        if current() { rgb(ui.theme.with(|t| t.accent.light)) } else { secondary() }
                    }),
            )
    })
    .estimated_item_size(px(52.0))
    .element_ref(&groups)
    .w(px(GROUPS_W))
    .bg(move || ui.surface(ui.theme.with(|t| t.background.panel)))
}

pub fn info_panel(ui: Ui) -> impl IntoElement {
    let program = computed(move || ui.focus.with(|f| (f.channel, f.program())));
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
            text(move || if ui.playing.get().is_none() { "Nothing playing".to_owned() } else { String::new() })
                .text_size(px(TEXT_SIZE))
                .text_color(secondary()),
        );
    let details = view()
        .flex_1()
        .gap(px(6.0))
        .child(
            text(move || program.with(|(_, p)| format!("Show {}", p.start.rem_euclid(97))))
                .text_size(px(20.0))
                .text_color(rgb(WHITE)),
        )
        .child(
            text(move || {
                program.with(|(ch, p)| {
                    let state = match p.airing() {
                        Airing::Live => "Live",
                        Airing::Upcoming => "Upcoming",
                        Airing::Ended => "Ended",
                    };
                    format!("{} · {}–{} · {}", channel_name(*ch), clock(p.start), clock(p.end), state)
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
                .opacity(move || if program.with(|(_, p)| p.airing() == Airing::Live) { 1.0_f32 } else { 0.0 })
                .child(
                    view()
                        .h_full()
                        .rounded(px(2.0))
                        .w(move || px(260.0 * program.with(|(_, p)| p.progress())))
                        .bg(move || rgb(ui.theme.with(|t| t.accent.main))),
                ),
        )
        .child(
            text(move || match ui.playing.get() {
                Some(ch) => format!("Playing {} · player {} · mode {:?}", channel_name(ch), ui.player_status.get(), ui.mode.get()),
                None => format!("player {} · mode {:?}", ui.player_status.get(), ui.mode.get()),
            })
            .text_size(px(12.0))
            .text_color(secondary()),
        )
        .child(
            text(move || match (ui.focus.with(|f| f.pane), ui.keys_shown()) {
                (Pane::Guide, true) => "OK watch   ◀ groups   Back groups",
                (Pane::Groups, true) => "OK open group   ▲▼ choose   Back exit",
                (_, false) => "Tap a programme to watch · tap a group to open it",
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
