//! Picture in picture (Android): the playing channel in a small system
//! window over other apps, video only. The state is `iptv_core::pip`.
//!
//! It opens from the control strip's button, or by itself when the viewer
//! leaves the app (Home) while a channel plays full screen and
//! `player.pip_on_leave` allows it (on phones and tablets by default, not
//! on a TV). The window's buttons play/pause and change channel; a tap on
//! it brings the app back full screen; closing it stops playback (the host
//! stops its players when the activity stops, as for Home without it).
//!
//! The host's side is surface 0's `tv.player` (`video.rs`):
//! - prop `pip` `{auto, enter}`: `auto`, whether leaving the app opens the
//!   window now; a new `enter` (a count) opens it now;
//! - event `pip` `{supported, tv, active}`: once mounted, and whenever the
//!   window opens or closes;
//! - event `pipAction` `{action: play|pause|next|previous}`: its buttons.
//!
//! On the desktops (Linux, macOS) the app's own window plays the part
//! (`desktop/pip_window.rs`): small, borderless and above the others,
//! video only; a double click on it, or Escape, brings the window back as
//! it was. Leaving the app opens nothing there.
//!
//! In the window only the video shows (`lib.rs` hides every other layer,
//! [`ui_shown`]): the panels, the play bar, the channel list and any
//! chooser or menu close as it opens, and multiview leaves to its active
//! tile, which plays on in the window (its other tiles stop).

use iptv_core::pip::{Change, OnLeave};
use iptv_settings::keys::PIP_ON_LEAVE;
use nsh_core::prelude::*;
use serde_json::{Value, json};

use crate::{Ui, ViewMode};

/// A channel plays full screen: what the window would show.
fn playing(ui: Ui) -> bool {
    ui.mode.get() == ViewMode::Fullscreen && ui.playing.get().is_some()
}

/// Whether the window is open (reading subscribes).
pub fn active(ui: Ui) -> bool {
    ui.pip.with(|p| p.active)
}

/// Whether the app's layers over the video show: not in the window.
pub fn ui_shown(ui: Ui) -> bool {
    ui.pip.with(|p| p.ui_shown())
}

/// Whether this device has the window at all (reading subscribes): the
/// play bar shows its button only then.
pub fn supported(ui: Ui) -> bool {
    ui.pip.with(|p| p.supported)
}

/// Whether the control strip's button can open the window now.
pub fn can_enter(ui: Ui) -> bool {
    let playing = playing(ui);
    ui.pip.with(|p| p.can_enter(playing))
}

/// The desktops: their window can (Android's host reports its own).
pub fn bind(ui: Ui) {
    if crate::desktop::DESKTOP {
        ui.pip.update(|p| {
            p.report(true, false, false);
        });
    }
}

/// The control strip's button: asks the host to open the window (on the
/// desktops, makes the app's window it).
pub fn enter(ui: Ui) {
    let playing = untracked(|| playing(ui));
    let mut asked = false;
    ui.pip.update(|p| asked = p.request(playing));
    if !asked {
        return;
    }
    log::info!("iptv pip: asked to enter");
    if crate::desktop::DESKTOP && crate::desktop::pip_window::enter() {
        report(ui, true);
    }
}

/// The desktops: a double click on the video, or Escape, in the window.
/// False when no window is open.
pub fn leave(ui: Ui) -> bool {
    if !crate::desktop::DESKTOP || !ui.pip.with_untracked(|p| p.active) {
        return false;
    }
    crate::desktop::pip_window::leave();
    report(ui, false);
    true
}

/// The desktop window opened or closed.
fn report(ui: Ui, active: bool) {
    on_state(
        ui,
        &json!({ "supported": true, "tv": false, "active": active }),
    );
}

/// Surface `surface`'s `pip` prop: `{auto, enter}` for surface 0, null
/// for the others (the window shows surface 0).
pub fn prop(ui: Ui, surface: usize) -> Value {
    if surface != 0 {
        return Value::Null;
    }
    ui.settings.get();
    let on_leave = OnLeave::from_key(crate::settings::choice(PIP_ON_LEAVE));
    let playing = playing(ui);
    let (auto, enter) = ui
        .pip
        .with(|p| (p.auto_enter(on_leave, playing), p.requests));
    json!({ "auto": auto, "enter": enter })
}

/// The host's `pip` event.
pub fn on_state(ui: Ui, data: &Value) {
    let flag = |name: &str| data[name].as_bool().unwrap_or(false);
    let (supported, tv, active) = (flag("supported"), flag("tv"), flag("active"));
    let mut next = ui.pip.get_untracked();
    let change = next.report(supported, tv, active);
    if next == ui.pip.get_untracked() {
        return;
    }
    ui.pip.set(next);
    match change {
        Change::Entered => entered(ui),
        Change::Left => log::info!("iptv pip: left"),
        Change::None => log::info!("iptv pip: supported {supported}, tv {tv}"),
    }
}

/// The window opened: only the video, one stream.
fn entered(ui: Ui) {
    log::info!("iptv pip: entered");
    ui.panels.update(|p| p.hide());
    ui.play_bar.update(iptv_core::play_bar::PlayBar::hide);
    crate::chooser::close(ui);
    crate::menu_view::close(ui);
    if ui.channels.with_untracked(Option::is_some) {
        crate::channels_view::close(ui);
    }
    // The grid leaves to its active tile; a tile watched full screen ends
    // the session (its hidden tiles stop).
    if untracked(|| crate::multiview::is_open(ui)) {
        crate::multiview::exit(ui);
    } else {
        crate::multiview::close(ui);
    }
}

/// The host's `pipAction` event: one of the window's buttons.
pub fn on_action(ui: Ui, data: &Value) {
    let action = data["action"].as_str().unwrap_or("?");
    log::info!("iptv pip: {action}");
    match action {
        "play" => ui.paused.set(false),
        "pause" => ui.paused.set(true),
        "next" => crate::player_keys::zap(ui, 1),
        "previous" => crate::player_keys::zap(ui, -1),
        _ => {}
    }
}
