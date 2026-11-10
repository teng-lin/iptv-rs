//! Remote input: NSH keystrokes → `iptv_nav` keys → short/long presses →
//! context keymap → actions applied to the app state.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use iptv_core::channel_list::ListMode;
use iptv_core::panels::{Control, QuickAction, Shown};
use iptv_core::tracks::Kind as Track;
use iptv_nav::{Action, BackGuard, Context, Keymap, Phase, Press, PressDetector, RemoteKey};
use nsh_core::keymap::KeyPhase;
use nsh_core::prelude::*;

use crate::guide::{Move, Pane};
use crate::{Input, Ui, ViewMode};

/// Long press needs key releases, which NSH (branch `tv-keys`) reports on
/// Android only. Elsewhere every key is a plain press and long bindings are
/// not consulted (a deferred short press would never see its release).
pub(crate) const LONG_PRESS: bool = cfg!(target_os = "android");

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
    // A remote's Menu, and a PC keyboard's context-menu key ("contextmenu"
    // names the same key).
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
    // Back as a key (Android): heard for Long Back only; its short press
    // is the platform's (`on_back_press`, see [`route`]).
    ("back", RemoteKey::Back),
    // Keyboard stand-ins.
    ("m", RemoteKey::Menu),
    ("g", RemoteKey::Guide),
    ("i", RemoteKey::Info),
    ("=", RemoteKey::ChannelUp),
    ("-", RemoteKey::ChannelDown),
    ("space", RemoteKey::PlayPause),
    ("p", RemoteKey::Previous),
    ("n", RemoteKey::Next),
    ("[", RemoteKey::Rewind),
    ("]", RemoteKey::FastForward),
];

struct Nav {
    keymap: Keymap,
    detector: PressDetector,
    start: Instant,
}

/// The surface that owns input now.
pub fn context(ui: &Ui) -> Context {
    if crate::epg_sources_view::is_open(*ui) {
        return Context::Editor; // Up/Down/Left/Right, OK, Back
    }
    if crate::wizard_view::is_open(*ui) || crate::playlists_view::is_open(*ui) {
        return Context::Menu;
    }
    if crate::settings_view::is_open(*ui) {
        return Context::Editor; // Up/Down/Left/Right, OK, Back
    }
    if crate::guide_assign_view::is_open(*ui) {
        return Context::Menu;
    }
    if ui.editor.with(Option::is_some) {
        return Context::Editor;
    }
    let overlay = ui.menu.with(Option::is_some)
        || ui.description.with(Option::is_some)
        || ui.chooser.with(Option::is_some);
    if overlay {
        return Context::Menu;
    }
    if crate::channels_view::is_open(*ui) {
        return Context::ChannelList;
    }
    if crate::search_view::is_open(*ui) {
        return Context::Editor; // Up/Down/Left/Right, OK and digits
    }
    match (ui.mode.get(), ui.focus.with(|f| f.pane)) {
        (ViewMode::Fullscreen, _) if ui.panels.with(|p| p.captures_input()) => Context::Controls,
        (ViewMode::Fullscreen, _) => Context::Player,
        (_, Pane::Groups | Pane::Rail) => Context::Groups,
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
    // Back on a PC keyboard: Escape is the platform's (NSH), Backspace ours
    // (it does not fire while a text field edits).
    on_key("backspace", move |_| {
        crate::go_back(ui);
    });
    // Long Left's stand-in where keys have no long press (the desktop).
    on_key("shift+left", move |_| {
        if context(&ui) == Context::Player {
            crate::channels_view::open(ui, ListMode::Preview);
        }
    });
    // Desktop sound keys (remotes and tablets use the system volume). F11
    // (window full screen; Ctrl+Cmd+F on macOS) is the platform's.
    on_key("ctrl+m", move |_| crate::media::toggle_mute(ui));
    on_key("ctrl+up", move |_| crate::media::step_volume(ui, true));
    on_key("ctrl+down", move |_| crate::media::step_volume(ui, false));
    // macOS keeps Ctrl+Up/Down for Mission Control: Cmd+Up/Down there.
    if cfg!(target_os = "macos") {
        on_key("cmd+up", move |_| crate::media::step_volume(ui, true));
        on_key("cmd+down", move |_| crate::media::step_volume(ui, false));
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
        // `remote.long_press`, taken up when a key goes down (a new press
        // ends whatever was held).
        let threshold = crate::settings::long_press_ms();
        if LONG_PRESS && phase == Phase::Down && nav.detector.threshold_ms() != threshold {
            nav.detector = PressDetector::new(threshold);
        }
        let has_long = LONG_PRESS && nav.keymap.has_long(ctx, key);
        let action = nav
            .detector
            .feed(key, phase, now, has_long)
            .and_then(|(k, press)| {
                catch_up_keys(&ui, ctx, k, nav.keymap.resolve(ctx, k, press)).map(|a| (k, press, a))
            });
        let deadline = (phase == Phase::Down)
            .then(|| nav.detector.long_deadline_ms())
            .flatten()
            .map(|at| at.saturating_sub(now));
        (action, deadline)
    };
    if key == RemoteKey::Back && phase == Phase::Down {
        ui.back_guard.update(BackGuard::pressed);
    }
    if let Some((key, press, action)) = action {
        act(ui, key, press, action);
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
                    .and_then(|(k, press)| nav.keymap.resolve(ctx, k, press).map(|a| (k, press, a)))
            };
            if let Some((key, press, action)) = action {
                log::info!("iptv long press by timer: {action:?}");
                act(ui, key, press, action);
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
    let action = action.map(|(_, _, a)| a);
    log::info!(
        "iptv key={} phase={phase:?} ctx={ctx:?} action={action:?} n={} handler_us={us} mode={:?} panels={:?}",
        key.name(),
        ui.stats.with(|s| s.keys),
        ui.mode.get(),
        ui.panels.with(|p| p.shown)
    );
}

/// Where a resolved key goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    /// The app applies it.
    Apply,
    /// A short Back: the platform's Back handler (`on_back_press` →
    /// `go_back`) takes that step; applying it too would go back twice.
    Platform,
    /// A Long Back: the app applies it, and the platform's Back that
    /// follows on release is swallowed.
    ApplyAndSwallow,
}

fn route(key: RemoteKey, press: Press) -> Route {
    match (key, press) {
        (RemoteKey::Back, Press::Short) => Route::Platform,
        (RemoteKey::Back, Press::Long) => Route::ApplyAndSwallow,
        _ => Route::Apply,
    }
}

fn act(ui: Ui, key: RemoteKey, press: Press, action: Action) {
    match route(key, press) {
        Route::Platform => {}
        Route::Apply => apply(ui, key, action),
        Route::ApplyAndSwallow => {
            arm_back_guard(ui);
            apply(ui, key, action);
        }
    }
}

/// A Long Back acted: its release must not also go back (`lib.rs`
/// `bind_back`), unless no release comes in time.
fn arm_back_guard(ui: Ui) {
    let now = ui.now_ms();
    ui.back_guard.update(|g| g.long_back(now));
    let wait = Duration::from_millis(BackGuard::TIMEOUT_MS);
    let _ = set_timeout(wait, move || {
        if ui.back_guard.with_untracked(BackGuard::armed) {
            let now = ui.now_ms();
            let mut expired = false;
            ui.back_guard.update(|g| expired = g.expire(now));
            if expired {
                log::info!("iptv long back: no release came");
            }
        }
    });
}

/// The platform's Back (`on_back_press`): `true` when it is the release of
/// a Long Back that acted, which takes no step back.
pub(crate) fn swallow_back(ui: Ui) -> bool {
    if !ui.back_guard.with_untracked(BackGuard::armed) {
        return false;
    }
    let now = ui.now_ms();
    let mut swallow = false;
    ui.back_guard.update(|g| swallow = g.take(now));
    if swallow {
        log::info!("iptv back: released after a long back; no step back");
    }
    swallow
}

/// While a programme plays from the archive, the keys `catchup.seek_keys`
/// names step along its time bar in the player: Left/Right (the default,
/// instead of the channel list and the most recent channel) or Down/Up (instead of zapping). With
/// `transport` only the media keys seek (Rewind/Fast-forward always do).
fn catch_up_keys(ui: &Ui, ctx: Context, key: RemoteKey, action: Option<Action>) -> Option<Action> {
    let catch_up = matches!(ui.playback.get_untracked(), crate::Playback::CatchUp { .. });
    if !catch_up || ctx != Context::Player {
        return action;
    }
    seek_key(
        crate::settings::choice(iptv_settings::keys::SEEK_KEYS),
        key,
        action,
    )
}

/// The player's action for `key` during catch-up under the `seek_keys`
/// choice `scheme`.
fn seek_key(scheme: &str, key: RemoteKey, action: Option<Action>) -> Option<Action> {
    match (scheme, key, action) {
        ("left_right", RemoteKey::Left, Some(Action::ShowChannels))
        | ("down_up", RemoteKey::Down, Some(Action::ChannelDown)) => Some(Action::TimeBarBack),
        ("left_right", RemoteKey::Right, Some(Action::PreviousChannel))
        | ("down_up", RemoteKey::Up, Some(Action::ChannelUp)) => Some(Action::TimeBarForward),
        _ => action,
    }
}

fn apply(ui: Ui, key: RemoteKey, action: Action) {
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
    let card = controls && ui.panels.with(|p| p.shown) == Shown::Card;
    let mv = |m: Move| crate::lists::navigate(ui, m);
    if crate::wizard_view::is_open(ui) {
        crate::wizard_view::key(ui, action);
        return;
    }
    // Top-most first: a guide sources screen opens over the playlists
    // screen, which opens over settings.
    if crate::epg_sources_view::is_open(ui) {
        crate::epg_sources_view::key(ui, action);
        return;
    }
    if crate::playlists_view::is_open(ui) {
        crate::playlists_view::key(ui, action);
        return;
    }
    if crate::settings_view::is_open(ui) {
        crate::settings_view::key(ui, action);
        return;
    }
    if crate::guide_assign_view::is_open(ui) {
        crate::guide_assign_view::key(ui, action);
        return;
    }
    if ui.editor.with(Option::is_some) {
        match action {
            Action::MoveUp => crate::fav_editor::move_focus(ui, -1),
            Action::MoveDown => crate::fav_editor::move_focus(ui, 1),
            Action::Select => crate::fav_editor::toggle(ui),
            Action::MoveLeft => crate::fav_editor::clear_group(ui),
            Action::MoveRight => crate::fav_editor::set_group(ui),
            Action::Back => crate::fav_editor::close(ui),
            _ => {}
        }
        return;
    }
    if ui.chooser.with(Option::is_some) {
        match action {
            Action::MoveUp => crate::chooser::move_focus(ui, -1),
            Action::MoveDown => crate::chooser::move_focus(ui, 1),
            Action::Select => {
                if let Some(i) = ui.chooser.with(|c| c.as_ref().map(|c| c.list.focus)) {
                    crate::chooser::pick(ui, i);
                }
            }
            Action::Back | Action::ShowMenu => crate::chooser::close(ui),
            _ => {}
        }
        return;
    }
    if ui.description.with(Option::is_some) {
        // The popover has no actions; Back (or Menu) closes it.
        if matches!(action, Action::Back | Action::ShowMenu) {
            crate::menu_view::close(ui);
        }
        return;
    }
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
    if crate::channels_view::is_open(ui) {
        crate::channels_view::key(ui, action);
        return;
    }
    if crate::search_view::is_open(ui) {
        crate::search_view::key(ui, action);
        return;
    }
    // Any key but Back dismisses the hint card (Back does, as its own step).
    if action != Action::Back && ui.focus.with(|f| f.hint) {
        ui.focus.update(|f| f.hint = false);
    }
    match action {
        // Menu in the groups pane adds a playlist; elsewhere it opens the menu.
        Action::ShowMenu if ui.focus.with(|f| f.pane) == Pane::Groups => {
            crate::wizard_view::open_add(ui)
        }
        Action::ShowMenu if ui.focus.with(|f| f.pane) == Pane::Rail => {}
        Action::ShowMenu => crate::menu_view::open(ui),
        Action::MoveLeft if controls => ui.update_panels(|p, now| p.move_focus(-1, now)),
        Action::MoveRight if controls => ui.update_panels(|p, now| p.move_focus(1, now)),
        // Over the buttons Up and Down move between the quick actions and
        // the control row.
        Action::MoveUp if controls => ui.update_panels(|p, now| {
            p.up(now);
        }),
        Action::MoveDown if controls => ui.update_panels(|p, now| {
            p.down(now);
        }),
        Action::Select if card => press_quick(ui, ui.panels.with(|p| p.quick_focused())),
        Action::Select if controls => press_control(ui, ui.panels.with(|p| p.focused())),
        Action::MoveUp => mv(Move::Up),
        Action::MoveDown => mv(Move::Down),
        Action::MoveLeft => mv(Move::Left),
        Action::MoveRight => mv(Move::Right),
        Action::Select => crate::lists::ok(ui),
        // Over a failed stream, OK retries.
        Action::ShowPanels if crate::player::failed(&ui) => crate::player::retry(ui),
        // Menu opens the control row at once (observed); OK the card.
        Action::ShowPanels if key == RemoteKey::Menu => {
            ui.update_panels(|p, now| p.show_controls(now))
        }
        Action::ShowPanels => ui.update_panels(|p, now| p.show_card(now)),
        Action::ShowInfo if ui.mode.get() == ViewMode::Fullscreen => {
            ui.update_panels(|p, now| p.show_info(now))
        }
        Action::ShowGuide => ui.mode.set(match ui.mode.get() {
            ViewMode::Fullscreen => ViewMode::Overlay,
            _ => ViewMode::Guide,
        }),
        Action::ShowGroups => crate::lists::open_groups(ui),
        Action::PastPrograms => {
            let view = crate::lists::view(ui);
            ui.focus.update(|f| f.enter_past(&view));
        }
        Action::ReturnToNow => {
            let view = crate::lists::view(ui);
            ui.focus.update(|f| {
                f.return_to_now(&view);
            });
        }
        Action::ReturnToPlayer if ui.playing.get().is_some() => ui.mode.set(ViewMode::Fullscreen),
        Action::PlayPause => ui.paused.update(|p| *p = !*p),
        // Catch-up seeking (`seek.rs`); nothing to seek in a live stream.
        Action::SeekBack => {
            crate::seek::step(ui, -crate::seek::steps().rewind);
        }
        Action::SeekForward => {
            crate::seek::step(ui, crate::seek::steps().forward);
        }
        Action::TimeBarBack => {
            crate::seek::step(ui, -crate::seek::steps().timebar);
        }
        Action::TimeBarForward => {
            crate::seek::step(ui, crate::seek::steps().timebar);
        }
        Action::ChannelUp => zap(ui, -1),
        Action::ChannelDown => zap(ui, 1),
        Action::PreviousChannel => crate::player::previous_channel(ui),
        Action::ShowChannels => crate::channels_view::open(ui, ListMode::Overlay),
        Action::ShowChannelsPreview => crate::channels_view::open(ui, ListMode::Preview),
        Action::Stop => stop_playback(ui),
        Action::GoLive => crate::player::go_live(ui),
        Action::PageUp => page(ui, -1),
        Action::PageDown => page(ui, 1),
        other => log::info!("iptv action {other:?} not implemented yet"),
    }
}

/// Long Back (or Stop): nothing plays; back to the guide.
fn stop_playback(ui: Ui) {
    if ui.playing.get_untracked().is_none() {
        return;
    }
    log::info!("iptv player: stopped by the viewer");
    ui.playing.set(None);
    crate::player::stop(ui);
    ui.mode.set(ViewMode::Guide);
}

/// Channel up/down: switches playback and keeps the focus on it. Wraps
/// within the list on screen (from the playing channel's row, else the
/// focused one); a channel the list does not have zaps within its group.
pub(crate) fn zap(ui: Ui, step: isize) {
    let list = ui.list.get();
    let playing = ui.playing.get();
    let from_row = playing
        .and_then(|ch| list.row_of(ch))
        .or_else(|| playing.is_none().then(|| ui.focus.with(|f| f.channel)));
    let to = match from_row.and_then(|row| list.zap(row, step)) {
        Some(row) => list.channel(row),
        None => playing.map(|ch| iptv_core::playback::zap_target(&*crate::schedule(), ch, step)),
    };
    let Some(to) = to else {
        return;
    };
    crate::lists::show_channel(ui, to);
    ui.play(to, crate::Playback::Live);
    // Channel-change feedback over full-screen video (`player.info_on_switch`).
    if ui.mode.get() == ViewMode::Fullscreen
        && crate::settings::flag(iptv_settings::keys::INFO_ON_SWITCH)
    {
        ui.update_panels(|p, now| p.show_info(now));
    }
}

/// OK (or a tap) on a quick action under the info card.
pub fn press_quick(ui: Ui, action: QuickAction) {
    ui.update_panels(|p, now| p.touch(now));
    match action {
        QuickAction::Guide => {
            if let Some(ch) = ui.playing.get() {
                crate::lists::show_channel(ui, ch);
            }
            ui.mode.set(ViewMode::Guide);
        }
        QuickAction::History => crate::chooser::open(ui, crate::chooser::Kind::History),
    }
}

/// OK (or a tap) on a control-row button.
pub fn press_control(ui: Ui, control: Control) {
    use crate::chooser::Kind;
    ui.update_panels(|p, now| p.touch(now));
    if !crate::player_panels::enabled(ui, control) {
        log::info!("iptv control {control:?} not available");
        return;
    }
    match control {
        Control::Search => crate::search_view::open(ui),
        Control::ChannelList => crate::channels_view::open(ui, ListMode::Overlay),
        Control::Scaling => crate::chooser::open(ui, Kind::Picture),
        Control::SleepTimer => crate::chooser::open(ui, Kind::Sleep),
        Control::VideoTracks => crate::chooser::open(ui, Kind::Track(Track::Video)),
        Control::AudioTracks => crate::chooser::open(ui, Kind::Track(Track::Audio)),
        Control::Captions => crate::chooser::open(ui, Kind::Track(Track::Text)),
        Control::AudioOffset => crate::chooser::open(ui, Kind::AudioDelay),
        Control::Favorite => {
            if let Some(ch) = ui.playing.get() {
                crate::favorites::toggle(ui, ch);
            }
        }
        other => log::info!("iptv control {other:?} not implemented yet"),
    }
}

/// Page up/down: moves the focus by the rows on screen.
fn page(ui: Ui, dir: isize) {
    let rows = ui.visible_rows.get().map_or(8, |(a, b)| (b - a).max(1));
    let len = ui.list.with(|l| l.len());
    ui.focus.update(|f| {
        let to = (f.channel as isize + dir * rows as isize).clamp(0, (len as isize - 1).max(0));
        f.channel = to as usize;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_as_a_key_acts_only_when_long() {
        assert_eq!(route(RemoteKey::Back, Press::Short), Route::Platform);
        assert_eq!(route(RemoteKey::Back, Press::Long), Route::ApplyAndSwallow);
        assert_eq!(route(RemoteKey::Left, Press::Long), Route::Apply);
        assert_eq!(route(RemoteKey::Ok, Press::Short), Route::Apply);
    }

    #[test]
    fn seek_keys_choose_the_time_bar_keys() {
        // The player's Left/Right: the channel list and the most recent channel.
        let (list, recent) = (Some(Action::ShowChannels), Some(Action::PreviousChannel));
        let (up, down) = (Some(Action::ChannelUp), Some(Action::ChannelDown));
        let back = Some(Action::TimeBarBack);
        let forward = Some(Action::TimeBarForward);
        // Left/Right (default): instead of those; Up/Down still zap.
        assert_eq!(seek_key("left_right", RemoteKey::Left, list), back);
        assert_eq!(seek_key("left_right", RemoteKey::Right, recent), forward);
        assert_eq!(seek_key("left_right", RemoteKey::Up, up), up);
        // Down/Up: instead of zapping; Left/Right keep their meaning.
        assert_eq!(seek_key("down_up", RemoteKey::Down, down), back);
        assert_eq!(seek_key("down_up", RemoteKey::Up, up), forward);
        assert_eq!(seek_key("down_up", RemoteKey::Left, list), list);
        // Page keys zap whatever the choice.
        assert_eq!(seek_key("down_up", RemoteKey::PageUp, up), up);
        // Transport keys only: the arrows keep their meaning.
        for (key, action) in [
            (RemoteKey::Left, list),
            (RemoteKey::Right, recent),
            (RemoteKey::Up, up),
            (RemoteKey::Down, down),
        ] {
            assert_eq!(seek_key("transport", key, action), action);
        }
    }
}
