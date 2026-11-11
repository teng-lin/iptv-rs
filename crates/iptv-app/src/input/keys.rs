//! NSH keystroke names and the remote keys they stand for.

use iptv_nav::RemoteKey;

/// NSH keystroke names and the remote keys they stand for: the remote's own
/// keys, then keyboard stand-ins for the desktop and emulators.
pub(super) const KEYS: &[(&str, RemoteKey)] = &[
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
    // A desktop keyboard's own (Ctrl + arrows: Cmd on macOS, where Ctrl
    // switches spaces).
    ("home", RemoteKey::Home),
    ("tab", RemoteKey::Tab),
    ("shift+tab", RemoteKey::ShiftTab),
    ("shift+left", RemoteKey::ShiftLeft),
    ("shift+right", RemoteKey::ShiftRight),
    ("mod+left", RemoteKey::CtrlLeft),
    ("mod+right", RemoteKey::CtrlRight),
    ("/", RemoteKey::Search),
    ("mod+f", RemoteKey::Search),
];

/// A remote's Search (`KEYCODE_SEARCH`): NSH has no key for it, so the
/// Android host hands it on as F12, which no remote sends
/// (`TvActivity.dispatchKeyEvent`).
pub(super) const ANDROID_SEARCH: &str = "f12";
