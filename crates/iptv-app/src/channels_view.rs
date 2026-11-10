//! The player's channel list (Left over full-screen video; Long Left
//! beside the video shrunk to a preview; the control row's Channels): the
//! playing group's channels with what is on now, or the groups. State is
//! `iptv_core::channel_list`; this draws it, takes its keys and plays what
//! it picks. The video stays the one persistent layer: preview mode only
//! changes its rectangle ([`preview_rect`]).

use iptv_core::channel_list::{ChannelList, ListMode, ListPane, Outcome};
use iptv_nav::Action;
use nsh_core::prelude::*;

use crate::theme::{FOCUS_ALPHA, RADIUS, TEXT_SECONDARY_ALPHA};
use crate::{Playback, Ui, ViewMode, clock, tint};

const WHITE: u32 = 0xFFFFFF;
const LIST_W: f32 = 420.0;
const ROW_H: f32 = 58.0;
const GROUP_H: f32 = 46.0;
/// Page Up/Down moves this many rows.
const PAGE: isize = 8;

pub fn is_open(ui: Ui) -> bool {
    ui.channels.with(Option::is_some)
}

/// Opens the list over (or beside) full-screen video.
pub fn open(ui: Ui, mode: ListMode) {
    if ui.mode.get_untracked() != ViewMode::Fullscreen {
        return;
    }
    ui.panels.update(|p| p.hide());
    let s = crate::schedule();
    let list = ChannelList::open(mode, ui.playing.get_untracked(), &*s);
    ui.channels.set(Some(list));
}

pub fn close(ui: Ui) {
    ui.channels.set(None);
}

/// The video's rectangle while the list shows beside it: right of the
/// list, 16:9, at the top.
pub fn preview_rect(ui: Ui, width: f32, _height: f32) -> Option<(f32, f32, f32, f32)> {
    let preview = ui
        .channels
        .with(|c| c.is_some_and(|c| c.mode == ListMode::Preview));
    preview.then(|| {
        let x = LIST_W + 40.0;
        let w = (width - x - 40.0).max(160.0);
        (x, 40.0, w, w * 9.0 / 16.0)
    })
}

fn update(ui: Ui, f: impl FnOnce(&mut ChannelList, &dyn crate::guide::Schedule) -> Outcome) {
    let s = crate::schedule();
    let mut outcome = Outcome::Stay;
    ui.channels.update(|c| {
        if let Some(c) = c.as_mut() {
            outcome = f(c, &*s);
        }
    });
    match outcome {
        Outcome::Stay => {}
        Outcome::Close => close(ui),
        Outcome::Play(channel) => {
            log::info!("iptv channel list: play {channel}");
            ui.focus.update(|f| f.show_channel(channel, &*s));
            ui.play(channel, Playback::Live);
        }
    }
}

/// The remote while the list shows.
pub fn key(ui: Ui, action: Action) {
    let playing = ui.playing.get_untracked();
    match action {
        Action::MoveUp => update(ui, |c, s| step(c, -1, s)),
        Action::MoveDown => update(ui, |c, s| step(c, 1, s)),
        Action::PageUp => update(ui, |c, s| step(c, -PAGE, s)),
        Action::PageDown => update(ui, |c, s| step(c, PAGE, s)),
        Action::MoveLeft => update(ui, |c, _| {
            c.left();
            Outcome::Stay
        }),
        Action::MoveRight => update(ui, |c, s| {
            c.right(s);
            Outcome::Stay
        }),
        Action::Select => update(ui, |c, s| c.select(playing, s)),
        Action::Back => update(ui, |c, _| c.back()),
        Action::ChannelUp | Action::ChannelDown => {
            crate::input::zap(ui, if action == Action::ChannelUp { -1 } else { 1 });
            if let Some(ch) = ui.playing.get_untracked() {
                update(ui, |c, s| {
                    c.follow(ch, s);
                    Outcome::Stay
                });
            }
        }
        Action::PlayPause => ui.paused.update(|p| *p = !*p),
        _ => {}
    }
}

fn step(c: &mut ChannelList, by: isize, s: &dyn crate::guide::Schedule) -> Outcome {
    c.move_focus(by, s);
    Outcome::Stay
}

/// A tap on row `index` of the open pane: focus it and pick it.
fn tap(ui: Ui, index: usize) {
    ui.touched();
    let playing = ui.playing.get_untracked();
    update(ui, |c, s| match c.pane {
        ListPane::Channels => {
            c.channel = c.channels(s).start + index;
            c.select(playing, s)
        }
        ListPane::Groups => {
            c.enter_group(index, s);
            Outcome::Stay
        }
    });
}

pub fn layer(ui: Ui) -> impl IntoElement {
    // The list belongs to full-screen playback and to the data it indexes.
    effect(move || {
        if ui.mode.get() != ViewMode::Fullscreen && untracked(|| is_open(ui)) {
            untracked(|| close(ui));
        }
    });
    let shown = std::cell::Cell::new(None);
    effect(move || {
        let version = ui.data_version.get();
        if shown.replace(Some(version)).is_some_and(|v| v != version) && untracked(|| is_open(ui)) {
            untracked(|| close(ui));
        }
    });
    let open = move || is_open(ui);
    let state = move || ui.channels.get();
    let overlay = move || state().is_some_and(|c| c.mode == ListMode::Overlay);
    let groups = move || state().is_some_and(|c| c.pane == ListPane::Groups);
    let count = move || {
        let s = crate::schedule();
        state().map_or(0, |c| match c.pane {
            ListPane::Channels => c.channels(&*s).len(),
            ListPane::Groups => s.group_count(),
        })
    };
    let focus_row = computed(move || {
        let s = crate::schedule();
        state().map(|c| match c.pane {
            ListPane::Channels => c.channel.saturating_sub(c.channels(&*s).start),
            ListPane::Groups => c.group_focus,
        })
    });
    let list = element_ref();
    effect(move || {
        if let Some(i) = focus_row.get() {
            list.scroll_to_index(i as u32, percentage(0.5), px(0.0), false);
        }
    });
    let panel = move || {
        let alpha = if overlay() { 0xE0 } else { 0xFF };
        tint(ui.theme.with(|t| t.background.panel), alpha)
    };
    view()
        .absolute()
        .inset_0()
        .flex_row()
        .opacity(move || if open() { 1.0_f32 } else { 0.0 })
        .pointer_events(move || {
            if open() {
                PointerEvents::BoxNone
            } else {
                PointerEvents::None
            }
        })
        .child(
            view()
                .w(px(LIST_W))
                .pt_safe_area()
                .px(px(12.0))
                .py(px(16.0))
                .bg(panel)
                .pointer_events(PointerEvents::Auto)
                .child(
                    text(move || {
                        let s = crate::schedule();
                        match state() {
                            Some(c) if c.pane == ListPane::Groups => "Groups".to_owned(),
                            Some(c) if s.group_count() > 0 => {
                                format!("◂  {}", s.group_name(c.group))
                            }
                            _ => String::new(),
                        }
                    })
                    .px(px(14.0))
                    .py(px(10.0))
                    .text_size(px(15.0))
                    .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA))
                    .on_press(move || {
                        ui.touched();
                        update(ui, |c, _| {
                            c.left();
                            Outcome::Stay
                        });
                    }),
                )
                .child(
                    virtual_list(count, move |at| row(ui, at.index(), groups, focus_row))
                        .estimated_item_size(px(ROW_H))
                        .element_ref(&list)
                        .flex_1(),
                ),
        )
        .child(
            // Over the video: a tap closes the list. Beside the preview:
            // the focused channel's programme; a tap on the preview goes
            // back to full screen.
            view()
                .flex_1()
                .pointer_events(PointerEvents::Auto)
                .on_press(move || {
                    ui.touched();
                    close(ui);
                })
                .child(when(move || !overlay(), move || preview_info(ui))),
        )
}

fn row(
    ui: Ui,
    index: usize,
    groups: impl Fn() -> bool + Copy + 'static,
    focus_row: Memo<Option<usize>>,
) -> impl IntoElement {
    let entry = move || {
        ui.data_version.get();
        let s = crate::schedule();
        let c = ui.channels.get()?;
        if groups() {
            let marker = if c.group == index { "•  " } else { "    " };
            return Some((format!("{marker}{}", s.group_name(index)), String::new()));
        }
        let ch = c.channels(&*s).start + index;
        if ch >= s.channel_count() {
            return None;
        }
        let now = ui.focus.with(|f| f.now);
        let playing = if ui.playing.get() == Some(ch) {
            "  ▶"
        } else {
            ""
        };
        let p = s.program_at(ch, now);
        Some((
            format!("{}  {}{playing}", s.channel_number(ch), s.channel_name(ch)),
            format!("{}  {}", clock(p.start), s.title(ch, p)),
        ))
    };
    let focused = move || ui.keys_shown() && focus_row.get() == Some(index);
    view()
        .h(move || px(if groups() { GROUP_H } else { ROW_H }))
        .px(px(14.0))
        .justify_center()
        .gap(px(2.0))
        .rounded(px(RADIUS))
        .bg(move || {
            if focused() {
                tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
            } else {
                tint(WHITE, 0)
            }
        })
        .on_press(move || tap(ui, index))
        .child(
            text(move || entry().map_or(String::new(), |e| e.0))
                .number_of_lines(1)
                .text_size(px(16.0))
                .text_color(rgb(WHITE)),
        )
        .child(when(
            move || !groups(),
            move || {
                text(move || entry().map_or(String::new(), |e| e.1))
                    .number_of_lines(1)
                    .text_size(px(13.0))
                    .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA))
            },
        ))
}

/// Under the preview: the focused channel and its programme now.
fn preview_info(ui: Ui) -> impl IntoElement {
    let info = move || {
        ui.data_version.get();
        let s = crate::schedule();
        let c = ui.channels.get()?;
        let now = ui.focus.with(|f| f.now);
        let p = s.program_at(c.channel, now);
        Some((
            format!(
                "{}  {}",
                s.channel_number(c.channel),
                s.channel_name(c.channel)
            ),
            s.title(c.channel, p),
            format!("{}–{}", clock(p.start), clock(p.end)),
        ))
    };
    let top = move || {
        let w = window();
        preview_rect(ui, w.width.0, w.height.0).map_or(0.0, |r| r.1 + r.3 + 20.0)
    };
    view()
        .pt(move || px(top()))
        .px(px(40.0))
        .gap(px(6.0))
        .pointer_events(PointerEvents::None)
        .child(
            text(move || info().map_or(String::new(), |i| i.0))
                .text_size(px(15.0))
                .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA)),
        )
        .child(
            text(move || info().map_or(String::new(), |i| i.1))
                .text_size(px(24.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(WHITE)),
        )
        .child(
            text(move || info().map_or(String::new(), |i| i.2))
                .text_size(px(15.0))
                .text_color(rgb(WHITE)),
        )
}
