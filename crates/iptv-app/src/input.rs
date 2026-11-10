//! Remote input: NSH keystrokes → `iptv_nav` keys → short/long presses →
//! context keymap → actions applied to the app state.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use iptv_nav::{Action, Context, Keymap, Phase, PressDetector, RemoteKey};
use nsh_core::prelude::*;

use crate::guide::{CHANNELS, Focus, Move, Pane};
use crate::{Input, Ui, ViewMode};

/// Long press needs key releases from NSH (branch `tv-keys`). Until then
/// every key is a plain press and long bindings are not consulted.
const LONG_PRESS: bool = false;

/// NSH keystroke names and the remote keys they stand for. Keyboard letters
/// stand in for remote buttons on the desktop and in emulators.
const KEYS: &[(&str, RemoteKey)] = &[
    ("up", RemoteKey::Up),
    ("down", RemoteKey::Down),
    ("left", RemoteKey::Left),
    ("right", RemoteKey::Right),
    ("enter", RemoteKey::Ok),
    ("pageup", RemoteKey::PageUp),
    ("pagedown", RemoteKey::PageDown),
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
    match (ui.mode.get(), ui.focus.with(|f| f.pane)) {
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
        on_key_with(&name, repeat, move |_| {
            key_event(ui, &nav, key, Phase::Down)
        });
    }
    // Spike shortcuts for the view modes.
    on_key("o", move |_| ui.toggle(ViewMode::Overlay));
    on_key("f", move |_| ui.toggle(ViewMode::Fullscreen));
}

fn key_event(ui: Ui, nav: &Rc<RefCell<Nav>>, key: RemoteKey, phase: Phase) {
    let t = Instant::now();
    let ctx = context(&ui);
    let action = {
        let mut nav = nav.borrow_mut();
        let now = nav.start.elapsed().as_millis() as u64;
        let has_long = LONG_PRESS && nav.keymap.has_long(ctx, key);
        nav.detector
            .feed(key, phase, now, has_long)
            .and_then(|(k, press)| nav.keymap.resolve(ctx, k, press))
    };
    if let Some(action) = action {
        apply(ui, action);
    }
    let us = t.elapsed().as_micros();
    ui.stats.update(|s| {
        s.keys += 1;
        s.last_us = us;
        s.max_us = s.max_us.max(us);
    });
    log::info!(
        "iptv key={} ctx={ctx:?} action={action:?} n={} handler_us={us}",
        key.name(),
        ui.stats.with(|s| s.keys)
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
    let mv = |m: Move| ui.focus.update(|f| f.apply(m));
    match action {
        Action::MoveUp => mv(Move::Up),
        Action::MoveDown => mv(Move::Down),
        Action::MoveLeft => mv(Move::Left),
        Action::MoveRight => mv(Move::Right),
        Action::Select => match ui.focus.with(|f| f.pane) {
            Pane::Groups => ui.focus.update(Focus::enter_group),
            Pane::Guide => ui.watch(),
        },
        // No panels or menus yet: the guide over the video stands in.
        Action::ShowPanels | Action::ShowGuide => ui.mode.set(match ui.mode.get() {
            ViewMode::Fullscreen => ViewMode::Overlay,
            _ => ViewMode::Guide,
        }),
        Action::ShowGroups => ui.focus.update(|f| f.pane = Pane::Groups),
        Action::ReturnToPlayer if ui.playing.get().is_some() => ui.mode.set(ViewMode::Fullscreen),
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
}

/// Page up/down: moves the focus by the rows on screen.
fn page(ui: Ui, dir: isize) {
    let rows = ui.visible_rows.get().map_or(8, |(a, b)| (b - a).max(1));
    ui.focus.update(|f| {
        let to = (f.channel as isize + dir * rows as isize).clamp(0, CHANNELS as isize - 1);
        f.channel = to as usize;
    });
}
