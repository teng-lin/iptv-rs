//! Player panels over full-screen video: a status chip (live, resolution,
//! player state, clock), an info card (channel, now/next with progress) and
//! the control row. State lives in `iptv_core::panels`; this only draws it.
//! Panels are views over the video SurfaceView (never alpha on the video).

use iptv_core::panels::{CONTROLS, Control, Shown};
use nsh_core::prelude::*;

use crate::guide::{Airing, program_at};
use crate::theme::{FOCUS_ALPHA, RADIUS, RADIUS_LARGE, RESTING_ALPHA, TEXT_SECONDARY_ALPHA};
use crate::{Ui, ViewMode, channel_name, clock, input, tint};

const WHITE: u32 = 0xFFFFFF;
/// Panel fill over video: the theme's panel colour, mostly opaque.
const PANEL_ALPHA: u8 = 0xE0;

fn label(control: Control, paused: bool) -> &'static str {
    match control {
        Control::PlayPause if paused => "▶ Play",
        Control::PlayPause => "❚❚ Pause",
        Control::GoLive => "● Live",
        Control::Audio => "Audio",
        Control::Subtitles => "Subtitles",
        Control::Guide => "Guide",
        Control::Favorite => "☆ Favourite",
    }
}

/// "SD" / "HD" / "FHD" / "4K" from the video height.
fn resolution(height: u32) -> &'static str {
    match height {
        h if h >= 2000 => "4K",
        h if h >= 1000 => "FHD",
        h if h >= 700 => "HD",
        _ => "SD",
    }
}

pub fn layer(ui: Ui) -> impl IntoElement {
    let shown = move || {
        if ui.mode.get() == ViewMode::Fullscreen {
            ui.panels.with(|p| p.shown)
        } else {
            Shown::Hidden
        }
    };
    let panel = move || tint(ui.theme.with(|t| t.background.panel), PANEL_ALPHA);
    view()
        .absolute()
        .inset_0()
        .pt_safe_area()
        .px(px(32.0))
        .pb(px(28.0))
        .opacity(move || {
            if shown() == Shown::Hidden {
                0.0_f32
            } else {
                1.0
            }
        })
        .pointer_events(move || {
            if shown() == Shown::Hidden {
                PointerEvents::None
            } else {
                PointerEvents::BoxNone
            }
        })
        .child(status_chip(ui, panel))
        .child(view().flex_1().pointer_events(PointerEvents::None))
        .child(info_card(ui, panel, move || shown() == Shown::Both))
}

fn status_chip(ui: Ui, panel: impl Fn() -> Rgba + Copy + 'static) -> impl IntoElement {
    let live = move || !ui.paused.get();
    view()
        .self_end()
        .mt(px(16.0))
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .px(px(14.0))
        .py(px(8.0))
        .rounded(px(RADIUS_LARGE))
        .bg(panel)
        .child(
            text(move || if live() { "LIVE" } else { "PAUSED" })
                .px(px(8.0))
                .py(px(2.0))
                .rounded(px(4.0))
                .text_size(px(12.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(WHITE))
                .bg(move || {
                    if live() {
                        rgb(ui.theme.with(|t| t.accent.main))
                    } else {
                        tint(WHITE, RESTING_ALPHA * 3)
                    }
                }),
        )
        .child(
            text(move || {
                let res = ui.video_size.get().map_or("", |(_, h)| resolution(h));
                match ui.player_status.get().as_str() {
                    "ready" => res.to_owned(),
                    other => format!("{res} {other}").trim().to_owned(),
                }
            })
            .text_size(px(13.0))
            .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA)),
        )
        .child(text(clock(0)).text_size(px(16.0)).text_color(rgb(WHITE)))
}

fn info_card(
    ui: Ui,
    panel: impl Fn() -> Rgba + Copy + 'static,
    with_controls: impl Fn() -> bool + Copy + 'static,
) -> impl IntoElement {
    let channel = move || ui.playing.get().unwrap_or(0);
    let now = move || program_at(channel(), 0);
    let next = move || program_at(channel(), now().end);
    let secondary = move || tint(WHITE, TEXT_SECONDARY_ALPHA);
    let line = move |prefix: &'static str, p: crate::guide::Program| {
        format!(
            "{prefix}Show {} · {}–{}",
            p.start.rem_euclid(97),
            clock(p.start),
            clock(p.end)
        )
    };
    view()
        .gap(px(14.0))
        .px(px(24.0))
        .py(px(18.0))
        .rounded(px(RADIUS_LARGE))
        .bg(panel)
        .pointer_events(PointerEvents::Auto)
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(20.0))
                .child(
                    text(move || format!("{}", channel() + 1))
                        .text_size(px(34.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(WHITE)),
                )
                .child(
                    view()
                        .flex_1()
                        .gap(px(6.0))
                        .child(
                            text(move || channel_name(channel()))
                                .text_size(px(14.0))
                                .text_color(secondary()),
                        )
                        .child(
                            text(move || line("", now()))
                                .text_size(px(20.0))
                                .text_color(rgb(WHITE)),
                        )
                        .child(
                            view()
                                .h(px(4.0))
                                .rounded(px(2.0))
                                .bg(tint(WHITE, RESTING_ALPHA * 2))
                                .child(
                                    view()
                                        .h_full()
                                        .rounded(px(2.0))
                                        .w(move || {
                                            let p = now();
                                            let done = if p.airing() == Airing::Live {
                                                p.progress()
                                            } else {
                                                0.0
                                            };
                                            percentage(done)
                                        })
                                        .bg(move || rgb(ui.theme.with(|t| t.accent.main))),
                                ),
                        )
                        .child(
                            text(move || line("Next: ", next()))
                                .text_size(px(14.0))
                                .text_color(secondary()),
                        ),
                ),
        )
        .child(
            view()
                .flex_row()
                .gap(px(10.0))
                .opacity(move || if with_controls() { 1.0_f32 } else { 0.0 })
                .pointer_events(move || {
                    if with_controls() {
                        PointerEvents::Auto
                    } else {
                        PointerEvents::None
                    }
                })
                .h(move || px(if with_controls() { 44.0 } else { 0.0 }))
                .children(
                    CONTROLS
                        .iter()
                        .enumerate()
                        .map(move |(i, control)| button(ui, i, *control)),
                ),
        )
}

fn button(ui: Ui, index: usize, control: Control) -> impl IntoElement {
    let focused =
        move || ui.keys_shown() && ui.panels.with(|p| p.captures_input() && p.focus == index);
    text(move || label(control, ui.paused.get()))
        .px(px(16.0))
        .py(px(10.0))
        .rounded(px(RADIUS))
        .text_size(px(14.0))
        .text_color(rgb(WHITE))
        .bg(move || {
            if focused() {
                tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
            } else {
                tint(WHITE, RESTING_ALPHA)
            }
        })
        .on_press(move || {
            ui.touched();
            ui.panels.update(|p| p.focus = index);
            input::press_control(ui, control);
        })
}
