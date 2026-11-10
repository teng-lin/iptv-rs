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

const CONTEXTS: [(Context, &str); 6] = [
    (Context::Guide, "In the guide"),
    (Context::Groups, "In the groups"),
    (Context::Player, "Watching full screen"),
    (Context::Controls, "With the player controls"),
    (Context::Menu, "In menus"),
    (Context::Editor, "In lists being edited"),
];

const KEYS: [RemoteKey; 26] = [
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
        RemoteKey::Digit(_) => "0–9",
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
        Action::ShowGuide => "Guide",
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
        Action::ReturnToPlayer => "Back to the player",
        Action::PlayPause => "Play or pause",
        Action::Stop => "Stop",
        Action::SeekBack => "Rewind (replays)",
        Action::SeekForward => "Fast-forward (replays)",
        Action::TimeBarBack => "Time bar back",
        Action::TimeBarForward => "Time bar forward",
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
    lines(&Keymap::defaults(), crate::input::LONG_PRESS)
}

/// Every key the map gives an action, everywhere first, then per screen.
pub fn lines(map: &Keymap, long: bool) -> Vec<Line> {
    let keys = KEYS.iter().copied().chain([RemoteKey::Digit(0)]);
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
        // Menu's Left/Right are bound to nothing: not listed as actions.
        assert!(!lines.iter().any(|l| l.action == "Nothing"));
    }
}
