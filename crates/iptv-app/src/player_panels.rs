//! Player panels over full-screen video: a status chip (live, resolution,
//! player state, clock), an info card (channel, now/next with progress) and
//! the control row. State lives in `iptv_core::panels`; this only draws it.
//! Panels are views over the video SurfaceView (never alpha on the video).
//! [`status`] draws the waiting and error states of `player.rs`.

use iptv_core::panels::{CONTROLS, Control, Shown};
use iptv_core::playback::{ErrorKind, Phase};
use nsh_core::prelude::*;

use crate::guide::Airing;
use crate::theme::{FOCUS_ALPHA, RADIUS, RADIUS_LARGE, RESTING_ALPHA, TEXT_SECONDARY_ALPHA};
use crate::{Ui, ViewMode, channel_name, clock, input, tint};

const WHITE: u32 = 0xFFFFFF;
/// Panel fill over video: the theme's panel colour, mostly opaque.
const PANEL_ALPHA: u8 = 0xE0;

/// Our own wording for the strip's controls.
fn label(control: Control, favorite: bool) -> &'static str {
    use Control::*;
    match control {
        Search => "Search",
        ChannelList => "Channels",
        Recordings => "Recordings",
        Multiview => "Multiview",
        VideoTracks => "Video",
        AudioTracks => "Audio",
        AudioOffset => "Audio delay",
        Captions => "Subtitles",
        Scaling => "Picture size",
        SleepTimer => "Sleep timer",
        Favorite if favorite => "★ Favourite",
        Favorite => "☆ Favourite",
        ChannelOptions => "Channel options",
        Settings => "Settings",
    }
}

/// The controls this build can perform; the rest show disabled.
pub fn implemented(control: Control) -> bool {
    matches!(
        control,
        Control::Search
            | Control::ChannelList
            | Control::Favorite
            | Control::Scaling
            | Control::SleepTimer
    )
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
            text(move || match (ui.playback.get(), live()) {
                (crate::Playback::CatchUp { .. }, _) => "CATCH-UP",
                (_, true) => "LIVE",
                (_, false) => "PAUSED",
            })
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
        .child(
            // Sleep timer: minutes left as of the last redraw.
            text(move || {
                ui.sleep
                    .with(|t| t.remaining_minutes(ui.now_ms()))
                    .map_or(String::new(), |m| format!("Sleep {m} min"))
            })
            .text_size(px(13.0))
            .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA)),
        )
        .child(
            text(move || {
                ui.data_version.get();
                clock(ui.focus.with(|f| f.now))
            })
            .text_size(px(16.0))
            .text_color(rgb(WHITE)),
        )
}

fn info_card(
    ui: Ui,
    panel: impl Fn() -> Rgba + Copy + 'static,
    with_controls: impl Fn() -> bool + Copy + 'static,
) -> impl IntoElement {
    // Every label below reads the channel, so new data re-renders them all.
    let channel = move || {
        ui.data_version.get();
        ui.playing.get().unwrap_or(0)
    };
    let live_now = move || ui.focus.with(|f| f.now);
    let minute = move || match ui.playback.get() {
        crate::Playback::Live => live_now(),
        crate::Playback::CatchUp { start, .. } => start,
    };
    let now = move || crate::schedule().program_at(channel(), minute());
    let next = move || crate::schedule().program_at(channel(), now().end);
    let secondary = move || tint(WHITE, TEXT_SECONDARY_ALPHA);
    let line = move |prefix: &'static str, p: crate::guide::Program| {
        format!(
            "{prefix}{} · {}–{}",
            crate::schedule().title(channel(), p),
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
                    text(move || crate::schedule().channel_number(channel()).to_string())
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
                                            // Catch-up: where the archive plays
                                            // (or the seek being chosen).
                                            if let Some((at, length)) = crate::seek::elapsed(ui) {
                                                return percentage(
                                                    at as f32 / length.max(1) as f32,
                                                );
                                            }
                                            let p = now();
                                            let at = live_now();
                                            let done = if p.airing(at) == Airing::Live {
                                                p.progress(at)
                                            } else {
                                                0.0
                                            };
                                            percentage(done)
                                        })
                                        .bg(move || rgb(ui.theme.with(|t| t.accent.main))),
                                ),
                        )
                        .child(
                            // Catch-up: position / length, and a seek target.
                            text(move || match crate::seek::elapsed(ui) {
                                Some((at, length)) => format!(
                                    "{}{} / {}",
                                    if ui.seek_target.get().is_some() {
                                        "⇢ "
                                    } else {
                                        ""
                                    },
                                    crate::seek::hms(at),
                                    crate::seek::hms(length)
                                ),
                                None => String::new(),
                            })
                            .text_size(px(14.0))
                            .text_color(rgb(WHITE)),
                        )
                        .child(
                            text(move || line("Next: ", next()))
                                .text_size(px(14.0))
                                .text_color(secondary()),
                        ),
                ),
        )
        .child(
            strip(ui, with_controls)
                .opacity(move || if with_controls() { 1.0_f32 } else { 0.0 })
                .pointer_events(move || {
                    if with_controls() {
                        PointerEvents::Auto
                    } else {
                        PointerEvents::None
                    }
                })
                .h(move || px(if with_controls() { 44.0 } else { 0.0 })),
        )
}

/// The control strip: a horizontal list that keeps the focused control
/// on screen.
fn strip(ui: Ui, with_controls: impl Fn() -> bool + Copy + 'static) -> VirtualList {
    let list = element_ref();
    let focus = computed(move || ui.panels.with(|p| p.focus));
    effect(move || {
        let at = focus.get();
        if with_controls() {
            list.scroll_to_index(at as u32, percentage(0.5), px(0.0), true);
        }
    });
    virtual_list(CONTROLS.len(), move |at| {
        button(ui, at.index(), CONTROLS[at.index()])
    })
    .horizontal()
    .estimated_item_size(px(150.0))
    .element_ref(&list)
}

fn button(ui: Ui, index: usize, control: Control) -> impl IntoElement {
    let focused =
        move || ui.keys_shown() && ui.panels.with(|p| p.captures_input() && p.focus == index);
    let favorite = move || {
        ui.playing
            .get()
            .is_some_and(|ch| ui.favorites.with(|f| f.contains(&ch)))
    };
    text(move || label(control, favorite()))
        .mr(px(10.0))
        .px(px(16.0))
        .py(px(10.0))
        .rounded(px(RADIUS))
        .text_size(px(14.0))
        .text_color(tint(WHITE, if implemented(control) { 0xFF } else { 0x60 }))
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

/// What the status layer shows over the video.
#[derive(Clone, Debug, PartialEq)]
enum Status {
    Hidden,
    Waiting(String),
    Failed {
        title: String,
        message: &'static str,
        detail: String,
    },
    /// A replayed programme reached its end.
    Over,
}

/// Our own wording for a failure.
fn failure_message(kind: &ErrorKind, catch_up: bool) -> &'static str {
    match kind {
        ErrorKind::Unavailable if catch_up => "This programme can't be replayed.",
        ErrorKind::Unavailable => "This channel has no stream address.",
        ErrorKind::Http(401 | 403) => "The provider refused this stream.",
        ErrorKind::Http(404 | 410) => "The provider doesn't have this stream.",
        ErrorKind::Http(_) => "The provider answered with an error.",
        ErrorKind::Network => "The stream can't be reached.",
        ErrorKind::Format => "This stream's format isn't supported.",
        ErrorKind::Ended => "The stream ended.",
        ErrorKind::Stalled => "The stream stopped sending video.",
        ErrorKind::Resolve => "The portal didn't give a stream address.",
        ErrorKind::Other => "Playback stopped with an error.",
    }
}

fn status_of(ui: Ui) -> Status {
    let Some(channel) = ui.playing.get() else {
        return Status::Hidden;
    };
    if ui.mode.get() == ViewMode::Overlay {
        return Status::Hidden; // the guide covers the video
    }
    let catch_up = matches!(ui.playback.get(), crate::Playback::CatchUp { .. });
    ui.player.health.with(|h| match h.phase() {
        Phase::Idle if catch_up => Status::Over,
        Phase::Idle | Phase::Playing => Status::Hidden,
        Phase::Resolving | Phase::Loading => Status::Waiting("Loading…".into()),
        Phase::Buffering => Status::Waiting("Buffering…".into()),
        Phase::Retrying { attempt, .. } => Status::Waiting(format!(
            "Reconnecting… ({attempt} of {})",
            h.policy.max_attempts
        )),
        Phase::Failed(f) => Status::Failed {
            title: format!("Can't play {}", channel_name(channel)),
            message: failure_message(&f.kind, catch_up),
            detail: match f.attempts {
                0 => f.detail.clone(),
                n => format!("{} · tried {n} times", f.detail),
            },
        },
    })
}

/// Waiting and error states over the video, in the player's rectangle:
/// a small pill while loading, buffering or reconnecting; a card with Retry
/// once retries stopped (OK retries too). Compact in the guide's preview.
pub fn status(ui: Ui) -> impl IntoElement {
    let status = computed(move || status_of(ui));
    let compact = move || ui.mode.get() == ViewMode::Guide;
    let panel = move || tint(ui.theme.with(|t| t.background.panel), PANEL_ALPHA);
    let size = move |full: f32| px(if compact() { full * 0.6 } else { full });
    let waiting = move || matches!(status.get(), Status::Waiting(_));
    let card = move || matches!(status.get(), Status::Failed { .. } | Status::Over);
    view()
        .absolute()
        .left(move || px(crate::video::rect(ui).0))
        .top(move || px(crate::video::rect(ui).1))
        .w(move || px(crate::video::rect(ui).2))
        .h(move || px(crate::video::rect(ui).3))
        .items_center()
        .justify_center()
        .pointer_events(PointerEvents::BoxNone)
        .child(when(waiting, move || {
            text(move || match status.get() {
                Status::Waiting(t) => t,
                _ => String::new(),
            })
            .px(size(18.0))
            .py(size(8.0))
            .rounded(px(RADIUS_LARGE))
            .bg(panel)
            .text_size(size(16.0))
            .text_color(rgb(WHITE))
        }))
        .child(when(card, move || {
            let failed = move || matches!(status.get(), Status::Failed { .. });
            view()
                .items_center()
                .gap(size(10.0))
                .px(size(28.0))
                .py(size(20.0))
                .rounded(px(RADIUS_LARGE))
                .bg(panel)
                .pointer_events(PointerEvents::Auto)
                .child(
                    text(move || match status.get() {
                        Status::Failed { title, .. } => title,
                        _ => "The programme has ended".to_owned(),
                    })
                    .text_size(size(22.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(WHITE)),
                )
                .child(
                    text(move || match status.get() {
                        Status::Failed { message, .. } => message.to_owned(),
                        _ => "Next (⏭) returns to live.".to_owned(),
                    })
                    .text_size(size(16.0))
                    .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA)),
                )
                .child(when(
                    move || failed() && !compact(),
                    move || {
                        text(move || match status.get() {
                            Status::Failed { detail, .. } => detail,
                            _ => String::new(),
                        })
                        .text_size(px(12.0))
                        .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA))
                    },
                ))
                .child(when(
                    move || failed() && !compact(),
                    move || {
                        text(move || {
                            if ui.keys_shown() {
                                "OK  Retry".to_owned()
                            } else {
                                "Retry".to_owned()
                            }
                        })
                        .mt(px(6.0))
                        .px(px(22.0))
                        .py(px(10.0))
                        .rounded(px(RADIUS))
                        .text_size(px(16.0))
                        .text_color(rgb(WHITE))
                        .bg(move || tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA))
                        .on_press(move || {
                            ui.touched();
                            crate::player::retry(ui);
                        })
                    },
                ))
        }))
}
