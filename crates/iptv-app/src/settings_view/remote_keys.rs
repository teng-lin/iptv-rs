//! The remote's keys and what they do, read-only (remapping comes later):
//! first what a key does everywhere, then per screen what differs. Read
//! from `iptv_nav`'s default map, so the list cannot drift from it.

use iptv_nav::{Action, Context, Keymap, Press, RemoteKey};

/// One line of the list: a heading (no action) or a key and its action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub key: String,
    pub action: &'static str,
}

impl Line {
    pub fn is_heading(&self) -> bool {
        self.action.is_empty()
    }
}

const CONTEXTS: [(Context, &str); 7] = [
    (Context::Guide, "In the guide"),
    (Context::Groups, "In the groups"),
    (Context::Player, "Watching full screen"),
    (Context::Controls, "With the player controls"),
    (Context::Menu, "In menus"),
    (Context::Editor, "In lists being edited"),
    (Context::Multiview, "In multiview"),
];

const KEYS: [RemoteKey; 27] = [
    RemoteKey::Up,
    RemoteKey::Down,
    RemoteKey::Left,
    RemoteKey::Right,
    RemoteKey::Ok,
    RemoteKey::Back,
    RemoteKey::Menu,
    RemoteKey::Guide,
    RemoteKey::Info,
    RemoteKey::ChannelUp,
    RemoteKey::ChannelDown,
    RemoteKey::PageUp,
    RemoteKey::PageDown,
    RemoteKey::PlayPause,
    RemoteKey::Play,
    RemoteKey::Pause,
    RemoteKey::Stop,
    RemoteKey::Rewind,
    RemoteKey::FastForward,
    RemoteKey::Record,
    RemoteKey::Next,
    RemoteKey::Previous,
    RemoteKey::Red,
    RemoteKey::Green,
    RemoteKey::Yellow,
    RemoteKey::Blue,
    RemoteKey::Search,
];

/// A desktop keyboard's own keys (`iptv_nav::Profile::Keyboard`).
const KEYBOARD_KEYS: [RemoteKey; 7] = [
    RemoteKey::Home,
    RemoteKey::Tab,
    RemoteKey::ShiftTab,
    RemoteKey::ShiftLeft,
    RemoteKey::ShiftRight,
    RemoteKey::CtrlLeft,
    RemoteKey::CtrlRight,
];

fn key_name(key: RemoteKey) -> String {
    let name = match key {
        RemoteKey::Up => "Up",
        RemoteKey::Down => "Down",
        RemoteKey::Left => "Left",
        RemoteKey::Right => "Right",
        RemoteKey::Ok => "OK",
        RemoteKey::Back => "Back",
        RemoteKey::Menu => "Menu",
        RemoteKey::Guide => "Guide",
        RemoteKey::Info => "Info",
        RemoteKey::ChannelUp => "Channel +",
        RemoteKey::ChannelDown => "Channel −",
        RemoteKey::PageUp => "Page up",
        RemoteKey::PageDown => "Page down",
        RemoteKey::PlayPause => "Play/pause",
        RemoteKey::Play => "Play",
        RemoteKey::Pause => "Pause",
        RemoteKey::Stop => "Stop",
        RemoteKey::Rewind => "Rewind",
        RemoteKey::FastForward => "Fast-forward",
        RemoteKey::Record => "Record",
        RemoteKey::Next => "Next",
        RemoteKey::Previous => "Previous",
        RemoteKey::Red => "Red",
        RemoteKey::Green => "Green",
        RemoteKey::Yellow => "Yellow",
        RemoteKey::Blue => "Blue",
        RemoteKey::Search => "Search",
        RemoteKey::Digit(_) => "0–9",
        RemoteKey::Home => "Home",
        RemoteKey::Tab => "Tab",
        RemoteKey::ShiftTab => "Shift+Tab",
        RemoteKey::ShiftLeft => "Shift+Left",
        RemoteKey::ShiftRight => "Shift+Right",
        RemoteKey::CtrlLeft if cfg!(target_os = "macos") => "Cmd+Left",
        RemoteKey::CtrlRight if cfg!(target_os = "macos") => "Cmd+Right",
        RemoteKey::CtrlLeft => "Ctrl+Left",
        RemoteKey::CtrlRight => "Ctrl+Right",
    };
    // The keyboard stands in for a remote on the desktop (`input.rs`).
    let keyboard = match key {
        RemoteKey::Ok => "Enter",
        RemoteKey::Back => "Esc",
        RemoteKey::Menu => "M",
        RemoteKey::Guide => "G",
        RemoteKey::Info => "I",
        RemoteKey::ChannelUp => "=",
        RemoteKey::ChannelDown => "-",
        RemoteKey::PlayPause => "Space",
        RemoteKey::Next => "N",
        RemoteKey::Previous => "P",
        RemoteKey::Search if cfg!(target_os = "macos") => "/ or Cmd+F",
        RemoteKey::Search => "/ or Ctrl+F",
        _ => "",
    };
    if cfg!(target_os = "android") || keyboard.is_empty() {
        name.to_owned()
    } else {
        format!("{name} ({keyboard})")
    }
}

fn action_name(action: Action) -> &'static str {
    match action {
        Action::MoveUp => "Move up",
        Action::MoveDown => "Move down",
        Action::MoveLeft => "Move left",
        Action::MoveRight => "Move right",
        Action::Select => "Choose",
        Action::Back => "Back",
        Action::ShowMenu => "Menu",
        Action::ShowGroups => "Groups",
        Action::OpenSearch => "Search",
        Action::ShowGuide => "Guide",
        Action::ShowChannels => "Channel list over the video",
        Action::ShowChannelsPreview => "Channel list beside the video",
        Action::ShowInfo => "Programme information",
        Action::ShowPanels => "Player controls",
        Action::ChannelUp => "Channel up",
        Action::ChannelDown => "Channel down",
        Action::PreviousChannel => "Last channel",
        Action::GoLive => "Back to live",
        Action::PageUp => "Page up",
        Action::PageDown => "Page down",
        Action::PastPrograms => "Earlier programmes",
        Action::ReturnToNow => "Back to now",
        Action::JumpToNow => "Now on the channel",
        Action::HourBack => "An hour earlier",
        Action::HourForward => "An hour later",
        Action::DayBack => "A day earlier",
        Action::DayForward => "A day later",
        Action::ToggleGroups => "Groups, or back to the guide",
        Action::ReturnToPlayer => "Back to the player",
        Action::PlayPause => "Play or pause",
        Action::Stop => "Stop",
        Action::SeekBack => "Rewind (replays)",
        Action::SeekForward => "Fast-forward (replays)",
        Action::TimeBarBack => "Time bar back",
        Action::TimeBarForward => "Time bar forward",
        Action::ToggleEnlarge => "Enlarge or reduce",
        Action::Record => "Record",
        Action::ToggleFavorite => "Favourite on or off",
        Action::Digit(_) => "Channel number",
        Action::Ignore => "Nothing",
    }
}

/// The presses the app tells apart: long ones only where keys report
/// their release (`input::LONG_PRESS`).
fn presses(long: bool) -> &'static [Press] {
    if long {
        &[Press::Short, Press::Long]
    } else {
        &[Press::Short]
    }
}

fn binding(key: RemoteKey, press: Press) -> String {
    match press {
        Press::Short => key_name(key),
        Press::Long => format!("Hold {}", key_name(key)),
    }
}

/// The app's keys now.
pub fn key_lines() -> Vec<Line> {
    let map = Keymap::for_profile(crate::input::PROFILE);
    lines(&map, crate::input::LONG_PRESS)
}

/// Every key the map gives an action, everywhere first, then per screen;
/// a keyboard's own keys where keys have no long press (the desktops).
pub fn lines(map: &Keymap, long: bool) -> Vec<Line> {
    let keyboard: &[RemoteKey] = if long { &[] } else { &KEYBOARD_KEYS };
    let keys = KEYS
        .iter()
        .chain(keyboard)
        .copied()
        .chain([RemoteKey::Digit(0)]);
    let all: Vec<(RemoteKey, Press)> = keys
        .flat_map(|k| presses(long).iter().map(move |p| (k, *p)))
        .collect();
    let everywhere = |k, p| {
        let first = map.resolve(CONTEXTS[0].0, k, p);
        CONTEXTS
            .iter()
            .all(|(c, _)| map.resolve(*c, k, p) == first)
            .then_some(first)
            .flatten()
    };
    let heading = |title: &str| Line {
        key: title.to_owned(),
        action: "",
    };
    let mut lines = vec![heading("Everywhere")];
    lines.extend(all.iter().filter_map(|&(k, p)| {
        everywhere(k, p).map(|a| Line {
            key: binding(k, p),
            action: action_name(a),
        })
    }));
    for (context, title) in CONTEXTS {
        let own: Vec<Line> = all
            .iter()
            .filter(|&&(k, p)| everywhere(k, p).is_none())
            .filter_map(|&(k, p)| {
                map.resolve(context, k, p).map(|a| Line {
                    key: binding(k, p),
                    action: action_name(a),
                })
            })
            .collect();
        if !own.is_empty() {
            lines.push(heading(title));
            lines.extend(own);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(lines: &'a [Line], key: &str) -> Vec<&'a Line> {
        lines.iter().filter(|l| l.key == key).collect()
    }

    #[test]
    fn the_list_follows_the_default_map() {
        let lines = lines(&Keymap::defaults(), true);
        assert!(lines[0].is_heading());
        // Back is the same everywhere: listed once, under "Everywhere".
        let back = lines
            .iter()
            .position(|l| l.key.starts_with("Back"))
            .unwrap();
        let first_screen = lines.iter().skip(1).position(Line::is_heading).unwrap() + 1;
        assert!(back < first_screen);
        // Up moves in the guide but changes channel full screen.
        let up = find(&lines, "Up");
        assert!(up.iter().any(|l| l.action == "Move up"));
        assert!(up.iter().any(|l| l.action == "Channel up"));
        // Long presses only where the device reports releases.
        assert!(lines.iter().any(|l| l.key.starts_with("Hold OK")));
        let short_only = super::lines(&Keymap::defaults(), false);
        assert!(!short_only.iter().any(|l| l.key.starts_with("Hold")));
        assert!(!lines.iter().any(|l| l.key == "Home"), "a remote has none");
        // A keyboard's own keys where keys have no long press.
        let keyboard = Keymap::for_profile(iptv_nav::Profile::Keyboard);
        let desk = super::lines(&keyboard, false);
        let home = find(&desk, "Home");
        assert!(home.iter().any(|l| l.action == "Now on the channel"));
        assert!(
            find(&desk, "Tab")
                .iter()
                .any(|l| l.action.starts_with("Groups"))
        );
        // Left moves on both (the focus model walks replays, then groups);
        // Hold Left shows the groups on a remote.
        let left = find(&desk, "Left");
        assert!(left.iter().any(|l| l.action == "Move left"));
        assert!(
            find(&lines, "Hold Left")
                .iter()
                .any(|l| l.action == "Groups")
        );
        // Search is a key everywhere: the keyboard's `/` and Ctrl+F.
        let search: Vec<_> = desk
            .iter()
            .filter(|l| l.key.starts_with("Search"))
            .collect();
        assert_eq!(search.len(), 1, "listed once, under Everywhere");
        assert_eq!(search[0].action, "Search");
        assert!(search[0].key.contains('/'));
        // Menu's Left/Right are bound to nothing: not listed as actions.
        assert!(!lines.iter().any(|l| l.action == "Nothing"));
    }
}
