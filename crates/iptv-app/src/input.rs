//! Remote input: NSH keystrokes → `iptv_nav` keys → short/long presses →
//! context keymap → actions applied to the app state.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use iptv_core::panels::Control;
use iptv_nav::{Action, Context, Keymap, Phase, PressDetector, RemoteKey};
use nsh_core::keymap::KeyPhase;
use nsh_core::prelude::*;

use crate::guide::{CHANNELS, Focus, Move, Pane};
use crate::{Input, Ui, ViewMode};

/// Long press needs key releases, which NSH (branch `tv-keys`) reports on
/// Android only. Elsewhere every key is a plain press and long bindings are
/// not consulted (a deferred short press would never see its release).
const LONG_PRESS: bool = cfg!(target_os = "android");

/// NSH keystroke names and the remote keys they stand for: the remote's own
/// keys, then keyboard stand-ins for the desktop and emulators.
const KEYS: &[(&str, RemoteKey)] = &[
    ("up", RemoteKey::Up),
    ("down", RemoteKey::Down),
    ("left", RemoteKey::Left),
    ("right", RemoteKey::Right),
    ("enter", RemoteKey::Ok),
    ("pageup", RemoteKey::PageUp),
    ("pagedown", RemoteKey::PageDown),
    ("menu", RemoteKey::Menu),
    ("guide", RemoteKey::Guide),
    ("info", RemoteKey::Info),
    ("channelup", RemoteKey::ChannelUp),
    ("channeldown", RemoteKey::ChannelDown),
    ("playpause", RemoteKey::PlayPause),
    ("play", RemoteKey::Play),
    ("pause", RemoteKey::Pause),
    ("stop", RemoteKey::Stop),
    ("rewind", RemoteKey::Rewind),
    ("fastforward", RemoteKey::FastForward),
    ("record", RemoteKey::Record),
    ("next", RemoteKey::Next),
    ("previous", RemoteKey::Previous),
    ("red", RemoteKey::Red),
    ("green", RemoteKey::Green),
    ("yellow", RemoteKey::Yellow),
    ("blue", RemoteKey::Blue),
    // Keyboard stand-ins.
    ("m", RemoteKey::Menu),
    ("g", RemoteKey::Guide),
    ("i", RemoteKey::Info),
    ("=", RemoteKey::ChannelUp),
    ("-", RemoteKey::ChannelDown),
    ("space", RemoteKey::PlayPause),
];

struct Nav {
    keymap: Keymap,
    detector: PressDetector,
    start: Instant,
}

/// The surface that owns input now.
pub fn context(ui: &Ui) -> Context {
    if ui.menu.with(Option::is_some) {
        return Context::Menu;
    }
    match (ui.mode.get(), ui.focus.with(|f| f.pane)) {
        (ViewMode::Fullscreen, _) if ui.panels.with(|p| p.captures_input()) => Context::Controls,
        (ViewMode::Fullscreen, _) => Context::Player,
        (_, Pane::Groups) => Context::Groups,
        _ => Context::Guide,
    }
}

pub fn bind(ui: Ui) {
    let nav = Rc::new(RefCell::new(Nav {
        keymap: Keymap::defaults(),
        detector: PressDetector::default(),
        start: Instant::now(),
    }));
    let repeat = KeyOptions {
        repeat: true,
        ..Default::default()
    };
    let digits = (0..=9u8).map(|d| (d, RemoteKey::Digit(d)));
    let mut bindings: Vec<(String, RemoteKey)> =
        KEYS.iter().map(|(n, k)| ((*n).to_owned(), *k)).collect();
    bindings.extend(digits.map(|(d, k)| (d.to_string(), k)));
    for (name, key) in bindings {
        let nav = nav.clone();
        on_key_event(&name, repeat, move |e: &KeyCommand| {
            let phase = match e.phase {
                KeyPhase::Press => Phase::Down,
                KeyPhase::Repeat => Phase::Repeat,
                KeyPhase::Release => Phase::Up,
            };
            key_event(ui, &nav, key, phase)
        });
    }
    // Spike shortcuts for the view modes.
    on_key("o", move |_| ui.toggle(ViewMode::Overlay));
    on_key("f", move |_| ui.toggle(ViewMode::Fullscreen));
}

fn key_event(ui: Ui, nav: &Rc<RefCell<Nav>>, key: RemoteKey, phase: Phase) {
    let t = Instant::now();
    let ctx = context(&ui);
    let (action, deadline) = {
        let mut nav = nav.borrow_mut();
        let now = nav.start.elapsed().as_millis() as u64;
        let has_long = LONG_PRESS && nav.keymap.has_long(ctx, key);
        let action = nav
            .detector
            .feed(key, phase, now, has_long)
            .and_then(|(k, press)| nav.keymap.resolve(ctx, k, press));
        let deadline = (phase == Phase::Down)
            .then(|| nav.detector.long_deadline_ms())
            .flatten()
            .map(|at| at.saturating_sub(now));
        (action, deadline)
    };
    if let Some(action) = action {
        apply(ui, action);
    }
    // IR remotes never auto-repeat: decide long press on a timer too. If the
    // key was released (or a repeat already fired it), the poll finds nothing.
    if let Some(wait_ms) = deadline {
        let nav = nav.clone();
        let _ = set_timeout(Duration::from_millis(wait_ms), move || {
            let action = {
                let mut nav = nav.borrow_mut();
                let now = nav.start.elapsed().as_millis() as u64;
                nav.detector
                    .poll(now)
                    .and_then(|(k, press)| nav.keymap.resolve(ctx, k, press))
            };
            if let Some(action) = action {
                log::info!("iptv long press by timer: {action:?}");
                apply(ui, action);
            }
        });
    }
    let us = t.elapsed().as_micros();
    if phase == Phase::Up && action.is_none() {
        return; // releases are logged only when they act (a deferred short press)
    }
    ui.stats.update(|s| {
        s.keys += 1;
        s.last_us = us;
        s.max_us = s.max_us.max(us);
    });
    log::info!(
        "iptv key={} phase={phase:?} ctx={ctx:?} action={action:?} n={} handler_us={us} mode={:?} panels={:?}",
        key.name(),
        ui.stats.with(|s| s.keys),
        ui.mode.get(),
        ui.panels.with(|p| p.shown)
    );
}

fn apply(ui: Ui, action: Action) {
    // The first key after touch starts from a row on screen.
    if ui.input.get() == Input::Touch {
        if let Some((first, last)) = ui.visible_rows.get() {
            ui.focus.update(|f| f.adopt_rows(first, last));
        }
        ui.input.set(Input::Keys);
    }
    // Any key while panels show keeps them up.
    ui.update_panels(|p, now| p.touch(now));
    let controls = ui.panels.with(|p| p.captures_input()) && ui.mode.get() == ViewMode::Fullscreen;
    let mv = |m: Move| ui.focus.update(|f| f.apply(m));
    if ui.menu.with(Option::is_some) {
        match action {
            Action::MoveUp => ui
                .menu
                .update(|m| m.as_mut().into_iter().for_each(|m| m.move_focus(-1))),
            Action::MoveDown => ui
                .menu
                .update(|m| m.as_mut().into_iter().for_each(|m| m.move_focus(1))),
            Action::Select => {
                if let Some(item) = ui.menu.with(|m| m.as_ref().and_then(|m| m.focused())) {
                    crate::menu_view::perform(ui, item);
                }
            }
            Action::Back | Action::ShowMenu => crate::menu_view::close(ui),
            _ => {}
        }
        return;
    }
    match action {
        Action::ShowMenu => crate::menu_view::open(ui),
        Action::MoveLeft if controls => ui.update_panels(|p, now| p.move_focus(-1, now)),
        Action::MoveRight if controls => ui.update_panels(|p, now| p.move_focus(1, now)),
        Action::Select if controls => press_control(ui, ui.panels.with(|p| p.focused())),
        Action::MoveUp => mv(Move::Up),
        Action::MoveDown => mv(Move::Down),
        Action::MoveLeft => mv(Move::Left),
        Action::MoveRight => mv(Move::Right),
        Action::Select => match ui.focus.with(|f| f.pane) {
            Pane::Groups => ui.focus.update(Focus::enter_group),
            Pane::Guide => ui.watch(),
        },
        Action::ShowPanels => ui.update_panels(|p, now| p.show_controls(now)),
        Action::ShowInfo if ui.mode.get() == ViewMode::Fullscreen => {
            ui.update_panels(|p, now| p.show_info(now))
        }
        Action::ShowGuide => ui.mode.set(match ui.mode.get() {
            ViewMode::Fullscreen => ViewMode::Overlay,
            _ => ViewMode::Guide,
        }),
        Action::ShowGroups => ui.focus.update(|f| f.pane = Pane::Groups),
        Action::ReturnToPlayer if ui.playing.get().is_some() => ui.mode.set(ViewMode::Fullscreen),
        Action::PlayPause => ui.paused.update(|p| *p = !*p),
        Action::ChannelUp => zap(ui, -1),
        Action::ChannelDown => zap(ui, 1),
        Action::PageUp => page(ui, -1),
        Action::PageDown => page(ui, 1),
        other => log::info!("iptv action {other:?} not implemented yet"),
    }
}

/// Channel up/down: switches playback and keeps the focus on it.
fn zap(ui: Ui, step: isize) {
    let from = ui
        .playing
        .get()
        .unwrap_or_else(|| ui.focus.with(|f| f.channel));
    let to = (from as isize + step).rem_euclid(CHANNELS as isize) as usize;
    ui.focus.update(|f| {
        f.pane = Pane::Guide;
        f.channel = to;
        f.cursor = 0;
    });
    ui.playing.set(Some(to));
    ui.paused.set(false);
    // Channel-change feedback over full-screen video.
    if ui.mode.get() == ViewMode::Fullscreen {
        ui.update_panels(|p, now| p.show_info(now));
    }
}

/// OK (or a tap) on a control-row button.
pub fn press_control(ui: Ui, control: Control) {
    ui.update_panels(|p, now| p.touch(now));
    match control {
        Control::PlayPause => ui.paused.update(|p| *p = !*p),
        // Live streams resume at the live edge when unpaused; catch-up later.
        Control::GoLive => ui.paused.set(false),
        Control::Guide => ui.mode.set(ViewMode::Overlay),
        other => log::info!("iptv control {other:?} not implemented yet"),
    }
}

/// Page up/down: moves the focus by the rows on screen.
fn page(ui: Ui, dir: isize) {
    let rows = ui.visible_rows.get().map_or(8, |(a, b)| (b - a).max(1));
    ui.focus.update(|f| {
        let to = (f.channel as isize + dir * rows as isize).clamp(0, CHANNELS as isize - 1);
        f.channel = to as usize;
    });
}
