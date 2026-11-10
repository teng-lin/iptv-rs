//! The programme context menu (Long OK / Menu): a side panel over a scrim.
//! State lives in `iptv_core::menu`; this opens, draws and performs it.

use iptv_core::guide::{catchup_days, program_at};
use iptv_core::menu::{Menu, MenuItem, Origin, program_menu};
use nsh_core::prelude::*;

use crate::guide::Pane;
use crate::theme::{FOCUS_ALPHA, RADIUS, RESTING_ALPHA, TEXT_SECONDARY_ALPHA};
use crate::{Ui, ViewMode, channel_name, clock, tint};

const WHITE: u32 = 0xFFFFFF;
const PANEL_W: f32 = 340.0;

fn label(item: MenuItem, favorite: bool) -> &'static str {
    match item {
        MenuItem::Watch => "Watch",
        MenuItem::WatchFromStart => "Watch from the start",
        MenuItem::PlayRecorded => "Play recording",
        MenuItem::Record => "Record",
        MenuItem::Reminder => "Remind me",
        MenuItem::Description => "Details",
        MenuItem::ToggleFavorite if favorite => "Remove from favourites",
        MenuItem::ToggleFavorite => "Add to favourites",
    }
}

/// Long OK / Menu: the focused programme's menu (in full screen, the playing
/// channel's current programme).
pub fn open(ui: Ui) {
    let (channel, minute) = match (ui.mode.get(), ui.playing.get()) {
        (ViewMode::Fullscreen, Some(ch)) => (ch, 0),
        _ => ui.focus.with(|f| (f.channel, f.cursor)),
    };
    let program = program_at(channel, minute);
    let entries = program_menu(program, catchup_days(channel), 0, true);
    let origin = Origin {
        channel,
        program_start: program.start,
    };
    ui.menu.set(Some(Menu::new(entries, origin)));
}

/// Closes the menu; guide focus returns to the programme it was opened on.
pub fn close(ui: Ui) {
    let Some(origin) = ui.menu.with(|m| m.as_ref().map(|m| m.origin)) else {
        return;
    };
    ui.menu.set(None);
    if ui.mode.get() != ViewMode::Fullscreen {
        ui.focus
            .update(|f| f.tap(origin.channel, origin.program_start.max(0)));
        ui.focus.update(|f| f.pane = Pane::Guide);
    }
}

pub fn perform(ui: Ui, item: MenuItem) {
    let channel = ui.menu.with(|m| m.as_ref().map(|m| m.origin.channel));
    close(ui);
    let Some(channel) = channel else { return };
    match item {
        MenuItem::Watch => {
            ui.playing.set(Some(channel));
            ui.paused.set(false);
        }
        MenuItem::ToggleFavorite => ui.favorites.update(|f| {
            if !f.remove(&channel) {
                f.insert(channel);
            }
        }),
        other => {
            log::info!("iptv menu {other:?} not implemented yet (catch-up, details, recordings)")
        }
    }
}

pub fn layer(ui: Ui) -> impl IntoElement {
    let open = move || ui.menu.with(Option::is_some);
    let title = move || {
        ui.menu.with(|m| {
            m.as_ref().map_or(String::new(), |m| {
                let p = program_at(m.origin.channel, m.origin.program_start.max(-30));
                format!(
                    "{} · {}–{}",
                    channel_name(m.origin.channel),
                    clock(p.start),
                    clock(p.end)
                )
            })
        })
    };
    let entry = move |index: usize| {
        let entry = move || {
            ui.menu
                .with(|m| m.as_ref().and_then(|m| m.entries.get(index).copied()))
        };
        let favorite = move || {
            let ch = ui.menu.with(|m| m.as_ref().map(|m| m.origin.channel));
            ch.is_some_and(|ch| ui.favorites.with(|f| f.contains(&ch)))
        };
        let focused = move || {
            ui.keys_shown()
                && ui
                    .menu
                    .with(|m| m.as_ref().is_some_and(|m| m.focus == index))
        };
        text(move || entry().map_or(String::new(), |e| label(e.item, favorite()).to_owned()))
            .px(px(16.0))
            .py(px(12.0))
            .rounded(px(RADIUS))
            .text_size(px(16.0))
            .text_color(move || {
                let enabled = entry().is_some_and(|e| e.enabled);
                tint(
                    WHITE,
                    if enabled {
                        0xFF
                    } else {
                        TEXT_SECONDARY_ALPHA / 2
                    },
                )
            })
            .bg(move || {
                if focused() {
                    tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
                } else {
                    tint(WHITE, 0)
                }
            })
            .opacity(move || if entry().is_some() { 1.0_f32 } else { 0.0 })
            .on_press(move || {
                ui.touched();
                let picked = ui.menu.with(|m| m.clone()).and_then(|mut m| m.tap(index));
                if let Some(item) = picked {
                    perform(ui, item);
                }
            })
    };
    view()
        .absolute()
        .inset_0()
        .flex_row()
        .opacity(move || if open() { 1.0_f32 } else { 0.0 })
        .pointer_events(move || {
            if open() {
                PointerEvents::Auto
            } else {
                PointerEvents::None
            }
        })
        .child(
            // Scrim: a tap outside closes the menu.
            view().flex_1().bg(tint(0x000000, 0x80)).on_press(move || {
                ui.touched();
                close(ui);
            }),
        )
        .child(
            view()
                .w(px(PANEL_W))
                .pt_safe_area()
                .px(px(16.0))
                .py(px(24.0))
                .gap(px(6.0))
                .bg(move || rgb(ui.theme.with(|t| t.background.panel)))
                .child(
                    text(title)
                        .px(px(16.0))
                        .pb(px(12.0))
                        .text_size(px(14.0))
                        .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA)),
                )
                .children((0..6).map(entry))
                .child(view().h(px(1.0)).bg(tint(WHITE, RESTING_ALPHA))),
        )
}
