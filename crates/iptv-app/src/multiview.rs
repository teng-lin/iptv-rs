//! Multiview in the app: the control row's Multiview turns the playing
//! channel into the first tile, more channels come from the channel list,
//! and each tile plays on its own player surface (`player.rs`, `video.rs`;
//! surface 0 is the full-screen player). The state machine is
//! `iptv_core::multiview`; this runs it, starts and stops the surfaces its
//! tiles name, mutes all but the active one, and leaves to full screen.
//! `multiview_view.rs` draws it.
//!
//! Limits: the device's decoders ([`DECODERS`]) and the streams the
//! account allows at once (an Xtream account's `max_connections`, from its
//! last import). Nothing else of the app opens streams (no recordings), so
//! the whole allowance is multiview's: adding the stream that goes past it
//! asks first.

use std::cell::Cell;

use iptv_core::channel_list::ListMode;
use iptv_core::multiview::{Key, Limits, MAX_SURFACES, Multiview, Outcome};
use iptv_import::SourceMeta;
use iptv_nav::Action;
use nsh_core::prelude::*;

use crate::{Playback, Ui, ViewMode};

pub type State = Multiview<usize>;

/// Streams this device decodes at once (setting-ready): cheap TV boxes
/// manage two, desktops four.
pub const DECODERS: usize = if cfg!(target_os = "android") { 2 } else { 4 };

/// Space between tiles, px.
const GAP: f32 = 4.0;

thread_local! {
    static ACCOUNT_STREAMS: Cell<Option<u32>> = const { Cell::new(None) };
}

/// The streams the shown source's account allows at once, if it says.
pub fn account_streams(meta: &SourceMeta) -> Option<u32> {
    match meta {
        SourceMeta::Xtream {
            account: Some(account),
            ..
        } => account.max_connections.filter(|n| *n > 0),
        _ => None,
    }
}

/// The source on screen changed its account's allowance (`data.rs`).
pub fn set_account_streams(allowed: Option<u32>) {
    ACCOUNT_STREAMS.with(|a| a.set(allowed));
}

pub fn is_open(ui: Ui) -> bool {
    ui.multiview.with(Option::is_some)
}

/// The channel list picks for multiview.
pub fn picking(ui: Ui) -> bool {
    ui.multiview
        .with_untracked(|m| m.as_ref().is_some_and(|m| m.pick().is_some()))
}

/// Where the channel list starts while picking: the active tile's channel.
pub fn pick_from(ui: Ui) -> Option<usize> {
    ui.multiview.with_untracked(|m| {
        m.as_ref()
            .filter(|m| m.pick().is_some())
            .and_then(|m| m.tiles().get(m.active_index()))
            .map(|t| t.channel)
    })
}

/// Whether `surface` is heard: the active tile's, or the full-screen one
/// outside multiview.
pub fn audible(ui: Ui, surface: usize) -> bool {
    ui.multiview.with(|m| match m {
        None => surface == 0,
        Some(m) => m.tiles().get(m.active_index()).map(|t| t.surface) == Some(surface),
    })
}

/// Surfaces 1–3 exist while a tile plays on them.
pub fn mounted(ui: Ui, surface: usize) -> bool {
    surface > 0
        && ui
            .multiview
            .with(|m| m.as_ref().is_some_and(|m| m.tile_on(surface).is_some()))
}

/// Cell `index`'s rectangle (px) in the window.
pub fn cell_rect(ui: Ui, index: usize) -> Option<(f32, f32, f32, f32)> {
    let layout = ui.multiview.with(|m| m.as_ref().map(State::layout))?;
    let w = window();
    let (width, height) = (w.width.0, w.height.0);
    let r = layout.cell(index);
    Some((
        r.x * width + GAP,
        r.y * height + GAP,
        (r.w * width - 2.0 * GAP).max(0.0),
        (r.h * height - 2.0 * GAP).max(0.0),
    ))
}

/// Where `surface` shows, while multiview is open; `None` without a tile.
pub fn surface_rect(ui: Ui, surface: usize) -> Option<(f32, f32, f32, f32)> {
    let tile = ui
        .multiview
        .with(|m| m.as_ref().and_then(|m| m.tile_on(surface)))?;
    cell_rect(ui, tile)
}

/// The control row's Multiview: the playing channel becomes the first tile
/// (live), and adding the next one starts at once.
pub fn open(ui: Ui) {
    let Some(channel) = ui.playing.get_untracked() else {
        return;
    };
    if is_open(ui) {
        return;
    }
    if ui.playback.get_untracked() != Playback::Live {
        ui.play(channel, Playback::Live);
    }
    ui.paused.set(false);
    ui.panels.update(|p| p.hide());
    crate::channels_view::close(ui);
    let limits = Limits {
        decoders: DECODERS,
        connections: ACCOUNT_STREAMS.with(Cell::get),
    };
    log::info!(
        "iptv multiview: open (device {} streams, account {:?})",
        limits.decoders,
        limits.connections
    );
    ui.multiview.set(Some(Multiview::open(channel, limits)));
    if ui
        .multiview
        .with_untracked(|m| m.as_ref().is_some_and(State::can_add))
    {
        apply(ui, |m, failed| m.key(Key::Ok, failed));
    }
}

/// Another playlist is shown: the tiles' channels belong to the old one.
/// Surface 0 is the caller's.
pub fn close(ui: Ui) {
    if !untracked(|| is_open(ui)) {
        return;
    }
    log::info!("iptv multiview: closed");
    for surface in 1..MAX_SURFACES {
        crate::player::stop_surface(ui, surface);
    }
    ui.multiview.set(None);
}

/// The remote while multiview owns it (`input.rs`).
pub fn key(ui: Ui, action: Action) {
    let key = match action {
        Action::MoveUp => Key::Up,
        Action::MoveDown => Key::Down,
        Action::MoveLeft => Key::Left,
        Action::MoveRight => Key::Right,
        Action::Select => Key::Ok,
        Action::ShowMenu => Key::Menu,
        Action::Back => Key::Back,
        Action::Stop => return crate::input::stop_playback(ui),
        _ => return,
    };
    apply(ui, |m, failed| m.key(key, failed));
}

/// Back (`lib.rs`): closes a dialog, else leaves to the active tile.
pub fn back(ui: Ui) -> bool {
    apply(ui, |m, failed| m.key(Key::Back, failed));
    true
}

/// A tap on cell `index`.
pub fn tap(ui: Ui, index: usize) {
    ui.touched();
    apply(ui, |m, failed| m.tap(index, failed));
}

/// A tap on item `index` of the open dialog.
pub fn choose(ui: Ui, index: usize) {
    ui.touched();
    apply(ui, |m, _| m.choose(index));
}

/// The channel list chose `channel` for multiview; `false` when it was
/// not picking for it.
pub fn picked(ui: Ui, channel: usize) -> bool {
    if !picking(ui) {
        return false;
    }
    log::info!("iptv multiview: picked {channel}");
    apply(ui, |m, _| m.picked(channel));
    true
}

/// The channel list closed (with or without a choice).
pub fn list_closed(ui: Ui) {
    if picking(ui) {
        ui.multiview
            .update(|m| m.iter_mut().for_each(State::cancel_pick));
    }
}

/// Runs a step, starts and stops the surfaces whose channel changed, and
/// does what the step asks.
fn apply(ui: Ui, f: impl FnOnce(&mut State, &dyn Fn(usize) -> bool) -> Outcome<usize>) {
    let failed = move |surface| crate::player::surface_failed(ui, surface);
    let mut step = None;
    ui.multiview.update(|m| {
        if let Some(m) = m.as_mut() {
            let before = m.assignments();
            let outcome = f(m, &failed);
            step = Some((before, m.assignments(), outcome));
        }
    });
    let Some((before, after, outcome)) = step else {
        return;
    };
    for surface in 0..MAX_SURFACES {
        if before[surface] == after[surface] {
            continue;
        }
        match after[surface] {
            Some(channel) => {
                log::info!("iptv multiview: surface {surface} plays {channel}");
                crate::player::start_surface(ui, surface, channel);
            }
            None => {
                log::info!("iptv multiview: surface {surface} stops");
                crate::player::stop_surface(ui, surface);
            }
        }
    }
    match outcome {
        Outcome::Stay => {}
        Outcome::PickChannel => crate::channels_view::open(ui, ListMode::Overlay),
        Outcome::Retry(surface) => crate::player::retry_surface(ui, surface),
        Outcome::Leave { channel, surface } => leave(ui, channel, surface),
    }
}

/// Leaves multiview: `channel` (on `surface`) plays full screen on
/// surface 0, the other surfaces stop.
fn leave(ui: Ui, channel: usize, surface: usize) {
    log::info!("iptv multiview: leaving to surface {surface}");
    for s in 1..MAX_SURFACES {
        crate::player::stop_surface(ui, s);
    }
    ui.multiview.set(None);
    if surface != 0 || ui.playing.get_untracked() != Some(channel) {
        crate::lists::show_channel(ui, channel);
        ui.play(channel, Playback::Live);
    }
    if ui.mode.get_untracked() == ViewMode::Fullscreen {
        ui.update_panels(|p, now| p.show_info(now));
    }
}

/// The data changed: each tile finds its channel again by key; a tile
/// whose channel went is removed, and with none left nothing plays.
/// `false` when multiview is not open.
pub fn follow(ui: Ui) -> bool {
    if !is_open(ui) {
        return false;
    }
    let schedule = crate::schedule();
    let mut moved = Vec::new();
    let mut gone = Vec::new();
    let mut any = true;
    ui.multiview.update(|m| {
        if let Some(m) = m.as_mut() {
            any = m.retain(|surface, channel| {
                let key = ui.player.stream(surface).key.get_untracked();
                match key.and_then(|k| schedule.find_channel(&k)) {
                    Some(row) => {
                        if row != *channel {
                            *channel = row;
                            moved.push((surface, row));
                        }
                        true
                    }
                    None => {
                        gone.push(surface);
                        false
                    }
                }
            });
        }
    });
    for (surface, row) in moved {
        if surface == 0 {
            ui.playing.set(Some(row));
        } else {
            ui.player.stream(surface).channel.set(Some(row));
        }
    }
    for surface in gone {
        log::info!("iptv multiview: surface {surface}'s channel left the guide");
        crate::player::stop_surface(ui, surface);
    }
    if !any {
        ui.multiview.set(None);
        ui.mode.set(ViewMode::Guide);
    }
    true
}

/// Multiview belongs to full-screen playback: leaving it (the guide's
/// keys) keeps the active tile playing.
pub fn bind(ui: Ui) {
    effect(move || {
        if ui.mode.get() != ViewMode::Fullscreen && untracked(|| is_open(ui)) {
            untracked(|| {
                let active = ui.multiview.with(|m| {
                    m.as_ref()
                        .and_then(|m| m.tiles().get(m.active_index()).cloned())
                });
                if let Some(t) = active {
                    leave(ui, t.channel, t.surface);
                }
            });
        }
    });
}
