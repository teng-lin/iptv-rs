//! Player panels over full-screen video, as observed in the reference UX: a
//! top bar (the playing channel's group; live state, sleep timer, date and
//! time), the info card (logo, programme, times, progress, minutes left,
//! number and name, stream badges, next programme), a full-width time bar
//! and the quick actions (TV guide, History); Down swaps the card for the
//! control row. A pointer (desktop mouse, tablet touch) has the play bar
//! instead (`play_bar.rs`). State lives in `iptv_core::panels`; this only
//! draws it.
//! Panels are views over the video SurfaceView (never alpha on the video).
//! [`status`] draws the waiting and error states of `player.rs`.

use iptv_core::panels::{CONTROLS, Control, QUICK_ACTIONS, QuickAction, Shown};
use iptv_core::playback::{ErrorKind, Phase, Waiting};
use iptv_core::player_info::{StreamFormat, badges, date_time, minutes_left};
use iptv_core::sound::delay_label;
use iptv_core::tracks::{Kind as Track, audio_layout};
use nsh_core::prelude::*;

use crate::guide::Airing;
use crate::theme::{FOCUS_ALPHA, RADIUS, RADIUS_LARGE, RESTING_ALPHA, TEXT_SECONDARY_ALPHA};
use crate::{Playback, Ui, ViewMode, channel_name, clock, input, tint};

const WHITE: u32 = 0xFFFFFF;
/// Panel fill over video: the theme's panel colour, mostly opaque.
const PANEL_ALPHA: u8 = 0xE0;
const LOGO_W: f32 = 112.0;
const LOGO_H: f32 = 63.0;

fn quick_label(action: QuickAction) -> &'static str {
    match action {
        QuickAction::Guide => "TV guide",
        QuickAction::History => "History",
    }
}

/// "Name · value", or the name alone.
fn with(name: &str, value: Option<String>) -> String {
    match value {
        Some(v) if !v.is_empty() => format!("{name} · {v}"),
        _ => name.to_owned(),
    }
}

/// The control row's labels (ours), with the current value where the
/// reference shows one.
fn label(ui: Ui, control: Control) -> String {
    use Control::*;
    let main = ui.player.main();
    let tracks = main.tracks;
    match control {
        Search => "Search".to_owned(),
        ChannelList => "Channels".to_owned(),
        Recordings => "Recordings".to_owned(),
        Multiview if crate::multiview::detour(ui) => "Back to multiview".to_owned(),
        Multiview => "Multiview".to_owned(),
        PictureInPicture => "Picture in picture".to_owned(),
        VideoTracks => {
            let playing = main.size.get().map(|(_, h)| format!("{h}p"));
            with(
                "Video",
                tracks.with(|t| t.summary(Track::Video)).or(playing),
            )
        }
        AudioTracks => {
            let ch = main.format.with(|f| f.channels);
            let layout = (ch > 0).then(|| audio_layout(ch));
            with("Audio", tracks.with(|t| t.summary(Track::Audio)).or(layout))
        }
        AudioOffset => with("Audio delay", Some(delay_label(ui.media.delay_ms.get()))),
        Captions => with(
            "Subtitles",
            Some(
                tracks
                    .with(|t| t.summary(Track::Text))
                    .unwrap_or_else(|| "Off".to_owned()),
            ),
        ),
        Scaling => with(
            "Picture size",
            Some(crate::chooser::picture_name(ui).into()),
        ),
        SleepTimer => with(
            "Sleep timer",
            Some(
                ui.sleep
                    .with(|t| t.remaining_minutes(ui.now_ms()))
                    .map_or("Off".to_owned(), |m| format!("{m} min")),
            ),
        ),
        Favorite if is_favorite(ui) => "★ Favourite".to_owned(),
        Favorite => "☆ Favourite".to_owned(),
        ChannelOptions => "Channel options".to_owned(),
        Settings => "Settings".to_owned(),
    }
}

fn is_favorite(ui: Ui) -> bool {
    ui.playing
        .get()
        .is_some_and(|ch| ui.favorites.with(|f| f.contains(&ch)))
}

/// Whether a control can act now; the rest show disabled.
pub fn enabled(ui: Ui, control: Control) -> bool {
    let choice = |k| ui.player.main().tracks.with(|t| t.has_choice(k));
    match control {
        Control::Multiview => ui.playing.get().is_some(),
        Control::Search
        | Control::ChannelList
        | Control::Favorite
        | Control::Scaling
        | Control::SleepTimer => true,
        Control::VideoTracks => choice(Track::Video),
        Control::AudioTracks => choice(Track::Audio),
        Control::Captions => choice(Track::Text),
        Control::AudioOffset => crate::media::AUDIO_DELAY && ui.playing.get().is_some(),
        Control::Recordings
        | Control::PictureInPicture
        | Control::ChannelOptions
        | Control::Settings => false,
    }
}

/// The panels as shown: hidden outside full screen and in multiview.
fn shown(ui: Ui) -> Shown {
    if ui.mode.get() == ViewMode::Fullscreen && !crate::multiview::is_open(ui) {
        ui.panels.with(|p| p.shown)
    } else {
        Shown::Hidden
    }
}

/// The group the playing channel is in (groups start in ascending order).
fn group_of(channel: usize) -> String {
    let s = crate::schedule();
    let n = s.group_count();
    if n == 0 {
        return String::new();
    }
    let (mut lo, mut hi) = (0, n);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if s.group_start(mid) <= channel {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    s.group_name(lo)
}

/// The channel's logo address (imported channels with an http(s) logo).
fn logo(channel: usize) -> Option<String> {
    let s = crate::schedule();
    let epg = s.as_any()?.downcast_ref::<iptv_guide::EpgSchedule>()?;
    let logo = epg.source_channel(channel)?.logo.as_deref()?.trim();
    (logo.starts_with("http://") || logo.starts_with("https://")).then(|| logo.to_owned())
}

pub fn layer(ui: Ui) -> impl IntoElement {
    let shown = computed(move || shown(ui));
    let panel = move || tint(ui.theme.with(|t| t.background.panel), PANEL_ALPHA);
    let card = move || matches!(shown.get(), Shown::Info | Shown::Card);
    let buttons = move || matches!(shown.get(), Shown::Card | Shown::Controls);
    let controls = move || shown.get() == Shown::Controls;
    view()
        .absolute()
        .inset_0()
        .pt_safe_area()
        .px(px(32.0))
        .pb(px(24.0))
        .opacity(move || {
            if shown.get() == Shown::Hidden {
                0.0_f32
            } else {
                1.0
            }
        })
        .pointer_events(move || {
            if shown.get() == Shown::Hidden {
                PointerEvents::None
            } else {
                PointerEvents::BoxNone
            }
        })
        .child(top_bar(ui, panel))
        .child(view().flex_1().pointer_events(PointerEvents::None))
        .child(
            view()
                .gap(px(10.0))
                // Mounted while hidden: nothing in the layer says `Auto` or
                // `BoxNone` then (NSH's pointer events inherit as CSS's,
                // so either would take presses through the hidden layer).
                .pointer_events(move || {
                    if shown.get() == Shown::Hidden {
                        PointerEvents::None
                    } else {
                        PointerEvents::BoxNone
                    }
                })
                .child(when(card, move || info_card(ui, panel)))
                .child(time_bar(ui, panel))
                .child(when(buttons, move || quick_row(ui, panel)))
                .child(
                    view()
                        .px(px(8.0))
                        .py(move || px(if controls() { 6.0 } else { 0.0 }))
                        .rounded(px(RADIUS_LARGE))
                        .bg(panel)
                        .opacity(move || if controls() { 1.0_f32 } else { 0.0 })
                        .pointer_events(move || {
                            if controls() {
                                PointerEvents::Auto
                            } else {
                                PointerEvents::None
                            }
                        })
                        .child(
                            strip(ui, controls).h(move || px(if controls() { 44.0 } else { 0.0 })),
                        ),
                ),
        )
}

/// The playing channel's group at the left; live state, sleep timer, date
/// and time at the right.
fn top_bar(ui: Ui, panel: impl Fn() -> Rgba + Copy + 'static) -> impl IntoElement {
    let live = move || !ui.paused.get();
    view()
        .mt(px(16.0))
        .flex_row()
        .items_center()
        .gap(px(14.0))
        .px(px(20.0))
        .py(px(10.0))
        .rounded(px(RADIUS_LARGE))
        .bg(panel)
        .child(
            text(move || {
                ui.guide_data();
                ui.playing.get().map_or(String::new(), group_of)
            })
            .flex_1()
            .number_of_lines(1)
            .text_size(px(16.0))
            .text_color(rgb(WHITE)),
        )
        .child(
            text(move || match (ui.playback.get(), live()) {
                (Playback::CatchUp { .. }, _) => "CATCH-UP",
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
                ui.guide_data();
                let s = crate::schedule();
                let unix = crate::guide::unix_of(&*s, ui.focus.with(|f| f.now));
                date_time(unix, crate::time::offset_minutes(unix))
            })
            .text_size(px(16.0))
            .text_color(rgb(WHITE)),
        )
}

/// The programme on screen: live now, or the replayed one.
fn programme(ui: Ui) -> (usize, crate::guide::Program) {
    ui.guide_data();
    let channel = ui.playing.get().unwrap_or(0);
    let minute = match ui.playback.get() {
        Playback::Live => ui.focus.with(|f| f.now),
        Playback::CatchUp { start, .. } => start,
    };
    (channel, crate::schedule().program_at(channel, minute))
}

/// How far the programme on screen is, 0–1: where the archive plays (or
/// the seek being chosen), else now in the live programme.
fn progress(ui: Ui) -> f32 {
    if let Some((at, length)) = crate::seek::elapsed(ui) {
        return at as f32 / length.max(1) as f32;
    }
    let (_, p) = programme(ui);
    let now = ui.focus.with(|f| f.now);
    if p.airing(now) == Airing::Live {
        p.progress(now)
    } else {
        0.0
    }
}

fn info_card(ui: Ui, panel: impl Fn() -> Rgba + Copy + 'static) -> impl IntoElement {
    let channel = move || programme(ui).0;
    let now = move || programme(ui).1;
    let next = move || crate::schedule().program_at(channel(), now().end);
    let secondary = move || tint(WHITE, TEXT_SECONDARY_ALPHA);
    let format = move || {
        let main = ui.player.main();
        let (width, height) = main.size.get().unwrap_or((0, 0));
        let f = main.format.get();
        StreamFormat { width, height, ..f }
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(20.0))
        .px(px(22.0))
        .py(px(16.0))
        .rounded(px(RADIUS_LARGE))
        .bg(panel)
        .child(
            // The logo slot stays when a channel has none.
            view()
                .w(px(LOGO_W))
                .h(px(LOGO_H))
                .rounded(px(RADIUS))
                .bg(tint(WHITE, RESTING_ALPHA))
                .child(
                    image(move || logo(channel()).unwrap_or_default())
                        .w_full()
                        .h_full()
                        .object_fit(ResizeMode::Contain),
                ),
        )
        .child(
            view()
                .flex_1()
                .gap(px(5.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            text(move || {
                                let ch = channel();
                                // As in the guide: its place in the list on screen.
                                format!("{}  {}", crate::lists::number(ui, ch), channel_name(ch))
                            })
                            .flex_1()
                            .number_of_lines(1)
                            .text_size(px(14.0))
                            .text_color(secondary()),
                        )
                        .child(badge(ui, format, 0))
                        .child(badge(ui, format, 1))
                        .child(badge(ui, format, 2)),
                )
                .child(
                    text(move || crate::schedule().title(channel(), now()))
                        .number_of_lines(1)
                        .text_size(px(22.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(WHITE)),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(
                            text(move || {
                                let p = now();
                                format!("{}–{}", clock(p.start), clock(p.end))
                            })
                            .text_size(px(14.0))
                            .text_color(rgb(WHITE)),
                        )
                        .child(
                            view()
                                .flex_1()
                                .h(px(4.0))
                                .rounded(px(2.0))
                                .bg(tint(WHITE, RESTING_ALPHA * 2))
                                .child(
                                    view()
                                        .h_full()
                                        .rounded(px(2.0))
                                        .w(move || percentage(progress(ui)))
                                        .bg(move || rgb(ui.theme.with(|t| t.accent.main))),
                                ),
                        )
                        .child(
                            text(move || {
                                let p = now();
                                let live = ui.playback.get() == Playback::Live;
                                let at = ui.focus.with(|f| f.now);
                                live.then(|| minutes_left(p.start, p.end, at))
                                    .flatten()
                                    .unwrap_or_default()
                            })
                            .text_size(px(14.0))
                            .text_color(secondary()),
                        ),
                )
                .child(
                    text(move || {
                        let p = next();
                        format!(
                            "Next  {}  {}",
                            clock(p.start),
                            crate::schedule().title(channel(), p)
                        )
                    })
                    .number_of_lines(1)
                    .text_size(px(14.0))
                    .text_color(secondary()),
                ),
        )
}

/// Stream badge `index` (resolution class, frame rate, audio layout), once
/// the player reported it.
fn badge(
    ui: Ui,
    format: impl Fn() -> StreamFormat + Copy + 'static,
    index: usize,
) -> impl IntoElement {
    let value = move || badges(&format()).get(index).cloned();
    text(move || value().unwrap_or_default())
        .px(move || px(if value().is_some() { 6.0 } else { 0.0 }))
        .py(px(1.0))
        .rounded(px(4.0))
        .text_size(px(12.0))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(WHITE))
        .bg(move || {
            tint(
                WHITE,
                if value().is_some() {
                    RESTING_ALPHA * 3
                } else {
                    0
                },
            )
        })
        .opacity(move || {
            if ui.playing.get().is_some() {
                1.0_f32
            } else {
                0.0
            }
        })
}

/// The full-width time bar: the programme's times (live) or position and
/// length (catch-up), with a dot where playback is.
fn time_bar(ui: Ui, panel: impl Fn() -> Rgba + Copy + 'static) -> impl IntoElement {
    let ends = move || match crate::seek::elapsed(ui) {
        Some((at, length)) => {
            let seeking = if ui.seek_target.get().is_some() {
                "⇢ "
            } else {
                ""
            };
            (
                format!("{seeking}{}", crate::seek::hms(at)),
                crate::seek::hms(length),
            )
        }
        None => {
            let (_, p) = programme(ui);
            (clock(p.start), clock(p.end))
        }
    };
    let accent = move || rgb(ui.theme.with(|t| t.accent.main));
    view()
        .flex_row()
        .items_center()
        .gap(px(14.0))
        .px(px(18.0))
        .py(px(8.0))
        .rounded(px(RADIUS_LARGE))
        .bg(panel)
        .child(
            text(move || ends().0)
                .text_size(px(13.0))
                .text_color(rgb(WHITE)),
        )
        .child(
            view()
                .flex_1()
                .h(px(12.0))
                .justify_center()
                .child(
                    view()
                        .h(px(4.0))
                        .rounded(px(2.0))
                        .bg(tint(WHITE, RESTING_ALPHA * 2))
                        .child(
                            view()
                                .h_full()
                                .rounded(px(2.0))
                                .w(move || percentage(progress(ui)))
                                .bg(accent),
                        ),
                )
                .child(
                    view()
                        .absolute()
                        .top(px(0.0))
                        .left(move || percentage(progress(ui)))
                        .ml(px(-6.0))
                        .w(px(12.0))
                        .h(px(12.0))
                        .rounded(px(6.0))
                        .bg(rgb(WHITE)),
                ),
        )
        .child(
            text(move || ends().1)
                .text_size(px(13.0))
                .text_color(rgb(WHITE)),
        )
}

/// A pill button; `focused` draws the key focus, `dim` a disabled one.
fn pill(
    ui: Ui,
    label: impl Fn() -> String + 'static,
    focused: impl Fn() -> bool + 'static,
    dim: impl Fn() -> bool + 'static,
    press: impl Fn() + 'static,
) -> Text {
    text(label)
        .px(px(16.0))
        .py(px(10.0))
        .rounded(px(RADIUS))
        .text_size(px(14.0))
        .text_color(move || tint(WHITE, if dim() { 0x60 } else { 0xFF }))
        .bg(move || {
            if focused() {
                tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
            } else {
                tint(WHITE, RESTING_ALPHA)
            }
        })
        .on_press(move || {
            ui.touched();
            press();
        })
}

/// TV guide and History (a pointer has the play bar, `play_bar.rs`).
fn quick_row(ui: Ui, panel: impl Fn() -> Rgba + Copy + 'static) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .px(px(8.0))
        .py(px(6.0))
        .rounded(px(RADIUS_LARGE))
        .bg(panel)
        .children(QUICK_ACTIONS.iter().enumerate().map(move |(i, action)| {
            let action = *action;
            let focused = move || {
                ui.keys_shown() && ui.panels.with(|p| p.shown == Shown::Card && p.quick == i)
            };
            pill(
                ui,
                move || quick_label(action).to_owned(),
                focused,
                || false,
                move || {
                    ui.panels.update(|p| p.quick = i);
                    input::press_quick(ui, action);
                },
            )
        }))
        .self_start()
}

/// The control row: a horizontal list that keeps the focused control
/// on screen.
fn strip(ui: Ui, open: impl Fn() -> bool + Copy + 'static) -> VirtualList {
    let list = element_ref();
    let focus = computed(move || ui.panels.with(|p| p.focus));
    effect(move || {
        let at = focus.get();
        if open() {
            list.scroll_to_index(at as u32, percentage(0.5), px(0.0), true);
        }
    });
    virtual_list(CONTROLS.len(), move |at| {
        let index = at.index();
        let control = CONTROLS[index];
        let focused = move || {
            ui.keys_shown()
                && ui
                    .panels
                    .with(|p| p.shown == Shown::Controls && p.focus == index)
        };
        pill(
            ui,
            move || label(ui, control),
            focused,
            move || !enabled(ui, control),
            move || {
                ui.panels.update(|p| p.focus = index);
                input::press_control(ui, control);
            },
        )
        .mr(px(10.0))
    })
    .horizontal()
    .estimated_item_size(px(170.0))
    .element_ref(&list)
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
pub(crate) fn failure_message(kind: &ErrorKind, catch_up: bool) -> &'static str {
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
    if crate::multiview::is_open(ui) {
        return Status::Hidden; // each tile shows its own
    }
    let catch_up = matches!(ui.playback.get(), Playback::CatchUp { .. });
    let now = ui.now_ms();
    ui.player.main().health.with(|h| match h.phase() {
        Phase::Idle if catch_up => Status::Over,
        Phase::Failed(f) => Status::Failed {
            title: format!("Can't play {}", channel_name(channel)),
            message: failure_message(&f.kind, catch_up),
            detail: match f.attempts {
                0 => f.detail.clone(),
                n => format!("{} · tried {n} times", f.detail),
            },
        },
        // A quick reconnect keeps the last frame without a pill.
        _ => h
            .indicator(now)
            .map_or(Status::Hidden, |w| Status::Waiting(waiting_text(&w))),
    })
}

/// The waiting pill's words.
pub(crate) fn waiting_text(waiting: &Waiting) -> String {
    match waiting {
        Waiting::Loading => "Loading…".into(),
        Waiting::Buffering => "Buffering…".into(),
        Waiting::Reconnecting { attempt, of } => format!("Reconnecting… ({attempt} of {of})"),
    }
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
            // Taps go through to the video (the play bar).
            .pointer_events(PointerEvents::None)
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
