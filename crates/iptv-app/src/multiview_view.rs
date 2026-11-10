//! Multiview on screen (`multiview.rs` runs it): over each tile's video a
//! frame (the active tile's in the accent: its sound plays), its channel
//! and its own waiting or error state with Retry; the add cell; a hint
//! line; and the dialogs (a tile's options, the connection warning). Taps
//! go to the cells and dialog items; the video surfaces are below
//! (`video.rs`).

use iptv_core::multiview::{Cell, Dialog, TileAction, WARNING_CHOICES, WarningChoice};
use iptv_core::playback::Phase;
use nsh_core::prelude::*;

use crate::multiview::{self, State};
use crate::theme::{FOCUS_ALPHA, RADIUS, RADIUS_LARGE, RESTING_ALPHA, TEXT_SECONDARY_ALPHA};
use crate::{Ui, channel_name, tint};

const WHITE: u32 = 0xFFFFFF;
const PANEL_ALPHA: u8 = 0xE0;
/// The active tile's frame, px.
const FRAME: f32 = 3.0;

fn action_label(action: TileAction) -> &'static str {
    match action {
        TileAction::Retry => "Try again",
        TileAction::Replace => "Change channel",
        TileAction::FullScreen => "Watch full screen",
        TileAction::Remove => "Remove from multiview",
    }
}

fn warning_label(choice: WarningChoice) -> &'static str {
    match choice {
        WarningChoice::AddAnyway => "Add it anyway",
        WarningChoice::Cancel => "Cancel",
    }
}

fn with_state<T>(ui: Ui, f: impl FnOnce(&State) -> T) -> Option<T> {
    ui.multiview.with(|m| m.as_ref().map(f))
}

/// What a tile shows over its video.
#[derive(Clone, Debug, PartialEq)]
enum TileStatus {
    Playing,
    Waiting(String),
    Failed(&'static str),
}

fn tile_status(ui: Ui, surface: usize) -> TileStatus {
    let now = ui.now_ms();
    ui.player.stream(surface).health.with(|h| match h.phase() {
        Phase::Failed(f) => {
            TileStatus::Failed(crate::player_panels::failure_message(&f.kind, false))
        }
        // A quick reconnect keeps the last frame without a label.
        _ => h.indicator(now).map_or(TileStatus::Playing, |w| {
            TileStatus::Waiting(crate::player_panels::waiting_text(&w))
        }),
    })
}

pub fn layer(ui: Ui) -> impl IntoElement {
    let open = move || multiview::is_open(ui);
    view()
        .absolute()
        .inset_0()
        .pointer_events(move || {
            if open() {
                PointerEvents::BoxNone
            } else {
                PointerEvents::None
            }
        })
        .child(when(open, move || {
            view()
                .absolute()
                .inset_0()
                .pointer_events(PointerEvents::BoxNone)
                .children((0..iptv_core::multiview::MAX_SURFACES).map(move |i| cell(ui, i)))
                .child(hint(ui))
                .child(when(
                    move || with_state(ui, |m| m.dialog().is_some()).unwrap_or(false),
                    move || dialog(ui),
                ))
        }))
}

/// Cell `index`: a tile's frame and status, or the add cell.
fn cell(ui: Ui, index: usize) -> impl IntoElement {
    let what = move || with_state(ui, |m| m.cells().get(index).copied()).flatten();
    let rect = move || multiview::cell_rect(ui, index).unwrap_or((0.0, 0.0, 0.0, 0.0));
    let surface = move || {
        with_state(ui, |m| match m.cells().get(index) {
            Some(Cell::Tile(t)) => m.tiles().get(*t).map(|t| t.surface),
            _ => None,
        })
        .flatten()
    };
    let channel = move || {
        with_state(ui, |m| match m.cells().get(index) {
            Some(Cell::Tile(t)) => m.tiles().get(*t).map(|t| t.channel),
            _ => None,
        })
        .flatten()
    };
    let active = move || with_state(ui, |m| m.active_index() == index).unwrap_or(false);
    let focused =
        move || ui.keys_shown() && with_state(ui, |m| m.focus_index() == index).unwrap_or(false);
    let accent = move || ui.theme.with(|t| t.accent.main);
    let panel = move || tint(ui.theme.with(|t| t.background.panel), PANEL_ALPHA);
    let is_tile = move || matches!(what(), Some(Cell::Tile(_)));
    let is_add = move || what() == Some(Cell::Add);
    let status = move || surface().map_or(TileStatus::Playing, |s| tile_status(ui, s));
    view()
        .absolute()
        .left(move || px(rect().0))
        .top(move || px(rect().1))
        .w(move || px(rect().2))
        .h(move || px(rect().3))
        .rounded(px(RADIUS))
        .border(move || px(if is_tile() && active() { FRAME } else { 1.0 }))
        .border_color(move || {
            if what().is_none() {
                tint(WHITE, 0)
            } else if is_tile() && active() {
                rgb(accent())
            } else if focused() {
                tint(accent(), FOCUS_ALPHA)
            } else {
                tint(WHITE, RESTING_ALPHA * 2)
            }
        })
        .bg(move || {
            if is_add() {
                if focused() {
                    tint(accent(), FOCUS_ALPHA)
                } else {
                    tint(WHITE, RESTING_ALPHA)
                }
            } else {
                tint(WHITE, 0)
            }
        })
        .items_center()
        .justify_center()
        .pointer_events(move || {
            if what().is_some() {
                PointerEvents::Auto
            } else {
                PointerEvents::None
            }
        })
        .on_press(move || multiview::tap(ui, index))
        .child(when(is_add, move || {
            view()
                .items_center()
                .gap(px(8.0))
                .child(text("+").text_size(px(48.0)).text_color(rgb(WHITE)))
                .child(
                    text("Add a channel")
                        .text_size(px(18.0))
                        .text_color(rgb(WHITE)),
                )
        }))
        .child(when(
            move || is_tile() && matches!(status(), TileStatus::Waiting(_)),
            move || {
                text(move || match status() {
                    TileStatus::Waiting(t) => t,
                    _ => String::new(),
                })
                .px(px(14.0))
                .py(px(6.0))
                .rounded(px(RADIUS_LARGE))
                .bg(panel)
                .text_size(px(14.0))
                .text_color(rgb(WHITE))
            },
        ))
        .child(when(
            move || is_tile() && matches!(status(), TileStatus::Failed(_)),
            move || {
                view()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(18.0))
                    .py(px(14.0))
                    .rounded(px(RADIUS_LARGE))
                    .bg(panel)
                    .child(
                        text(move || match status() {
                            TileStatus::Failed(m) => m.to_owned(),
                            _ => String::new(),
                        })
                        .text_size(px(15.0))
                        .text_color(rgb(WHITE)),
                    )
                    .child(
                        text("Try again")
                            .px(px(18.0))
                            .py(px(8.0))
                            .rounded(px(RADIUS))
                            .text_size(px(14.0))
                            .text_color(rgb(WHITE))
                            .bg(move || tint(accent(), FOCUS_ALPHA))
                            .on_press(move || {
                                ui.touched();
                                if let Some(s) = surface() {
                                    crate::player::retry_surface(ui, s);
                                }
                            }),
                    )
            },
        ))
        .child(when(is_tile, move || {
            // The channel at the bottom left; the sound mark on the active one.
            view()
                .absolute()
                .left(px(8.0))
                .bottom(px(8.0))
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .px(px(10.0))
                .py(px(4.0))
                .rounded(px(RADIUS))
                .bg(panel)
                .pointer_events(PointerEvents::None)
                .child(
                    text(move || {
                        ui.data_version.get();
                        channel().map_or(String::new(), |ch| {
                            format!("{}  {}", crate::lists::number(ui, ch), channel_name(ch))
                        })
                    })
                    .number_of_lines(1)
                    .text_size(px(14.0))
                    .text_color(rgb(WHITE)),
                )
                .child(
                    text(move || if active() { "♪" } else { "" })
                        .text_size(px(14.0))
                        .text_color(move || rgb(accent())),
                )
        }))
}

/// What the keys (or taps) do, and the device's limit once reached.
fn hint(ui: Ui) -> impl IntoElement {
    let words = move || {
        let keys = if ui.keys_shown() {
            "Arrows: choose the sound · OK: options · Back: full screen"
        } else {
            "Tap a tile to hear it, tap it again for options"
        };
        let full = with_state(ui, |m| {
            (!m.can_add())
                .then(|| format!("This device plays up to {} at once", m.limits().decoders))
        })
        .flatten();
        match full {
            Some(f) => format!("{keys} · {f}"),
            None => keys.to_owned(),
        }
    };
    view()
        .absolute()
        .top(px(10.0))
        .left(px(0.0))
        .right(px(0.0))
        .items_center()
        .pointer_events(PointerEvents::None)
        .child(
            text(words)
                .px(px(14.0))
                .py(px(5.0))
                .rounded(px(RADIUS_LARGE))
                .bg(move || tint(ui.theme.with(|t| t.background.panel), PANEL_ALPHA))
                .text_size(px(13.0))
                .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA)),
        )
}

/// A dialog's content: title, message, items and which is focused, and
/// whether they sit in a row.
fn dialog_content(ui: Ui) -> (String, String, Vec<&'static str>, usize, bool) {
    let content = with_state(ui, |m| match m.dialog() {
        Some(Dialog::Options { tile, items, focus }) => {
            let ch = m.tiles().get(*tile).map(|t| t.channel);
            (
                ch.map(channel_name).unwrap_or_default(),
                String::new(),
                items.iter().map(|a| action_label(*a)).collect(),
                *focus,
                false,
            )
        }
        Some(Dialog::Warning { allowed, focus }) => (
            match allowed {
                1 => "Your account allows 1 stream at once".to_owned(),
                n => format!("Your account allows {n} streams at once"),
            },
            "Another one may not play, or may stop one that is playing.".to_owned(),
            WARNING_CHOICES.iter().map(|c| warning_label(*c)).collect(),
            *focus,
            true,
        ),
        None => (String::new(), String::new(), Vec::new(), 0, false),
    });
    content.unwrap_or((String::new(), String::new(), Vec::new(), 0, false))
}

fn dialog(ui: Ui) -> impl IntoElement {
    let content = computed(move || dialog_content(ui));
    let panel = move || tint(ui.theme.with(|t| t.background.panel), 0xF0);
    let accent = move || ui.theme.with(|t| t.accent.main);
    let item = move |i: usize| {
        let label = move || content.with(|c| c.2.get(i).map(|s| (*s).to_owned()));
        let focused = move || ui.keys_shown() && content.with(|c| c.3 == i);
        when(
            move || label().is_some(),
            move || {
                text(move || label().unwrap_or_default())
                    .px(px(20.0))
                    .py(px(10.0))
                    .rounded(px(RADIUS))
                    .text_size(px(16.0))
                    .text_color(rgb(WHITE))
                    .bg(move || {
                        if focused() {
                            tint(accent(), FOCUS_ALPHA)
                        } else {
                            tint(WHITE, RESTING_ALPHA)
                        }
                    })
                    .on_press(move || multiview::choose(ui, i))
            },
        )
    };
    let row = move || content.with(|c| c.4);
    view()
        .absolute()
        .inset_0()
        .items_center()
        .justify_center()
        .bg(tint(0x000000, 0x80))
        .pointer_events(PointerEvents::Auto)
        // A tap beside the dialog closes it.
        .on_press(move || multiview::tap(ui, usize::MAX))
        .child(
            view()
                .w(px(380.0))
                .gap(px(10.0))
                .px(px(24.0))
                .py(px(20.0))
                .rounded(px(RADIUS_LARGE))
                .bg(panel)
                .pointer_events(PointerEvents::Auto)
                .on_press(|| {})
                .child(
                    text(move || content.with(|c| c.0.clone()))
                        .text_size(px(20.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(WHITE)),
                )
                .child(when(
                    move || content.with(|c| !c.1.is_empty()),
                    move || {
                        text(move || content.with(|c| c.1.clone()))
                            .text_size(px(15.0))
                            .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA))
                    },
                ))
                .child(when(row, move || {
                    view()
                        .mt(px(6.0))
                        .gap(px(8.0))
                        .flex_row()
                        .children((0..4).map(item))
                }))
                .child(when(
                    move || !row(),
                    move || view().mt(px(6.0)).gap(px(8.0)).children((0..4).map(item)),
                )),
        )
}
