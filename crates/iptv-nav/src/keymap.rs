//! The layered key → action map: user overrides, then the context's
//! defaults, then global defaults. Defaults are iptv-rs's own choices; the
//! context-specific ones follow the reference UX's documented instructions
//! (Long OK opens the menu in the guide, Left shows groups, Long Left browses
//! past programmes, Long Back returns to the player).

use crate::{Press, RemoteKey};

/// The surface that owns the remote's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Context {
    /// The guide grid (also when shown over video).
    Guide,
    /// The group list.
    Groups,
    /// Full-screen playback.
    Player,
    /// The buttons over full-screen video: the quick actions under the
    /// info card and the control row (Left/Right/OK move and press them;
    /// Up/Down move between the two rows).
    Controls,
    /// The channel list over the video (Up/Down move, OK plays, Left shows
    /// the groups, Back closes).
    ChannelList,
    /// An editing mode (favourites): Up/Down move, OK toggles, Left and
    /// Right act on the whole group, Back leaves.
    Editor,
    /// A context menu or popover (Up/Down move, OK selects, Back closes;
    /// Left/Right do nothing, as observed in the reference UX).
    Menu,
}

/// What the app should do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    /// OK on the focused item.
    Select,
    Back,
    ShowMenu,
    ShowGroups,
    ShowGuide,
    ShowInfo,
    /// Info and control panels over full-screen video.
    ShowPanels,
    ChannelUp,
    ChannelDown,
    /// Back to the channel played before the current one (again: return).
    PreviousChannel,
    /// From catch-up back to the live stream of the playing channel.
    GoLive,
    /// The playing group's channels as a list over full-screen video.
    ShowChannels,
    /// The same list beside the video, shrunk to a preview.
    ShowChannelsPreview,
    PageUp,
    PageDown,
    PastPrograms,
    ReturnToNow,
    ReturnToPlayer,
    PlayPause,
    Stop,
    SeekBack,
    SeekForward,
    /// One step back/forward along a replayed programme's time bar.
    TimeBarBack,
    TimeBarForward,
    Record,
    ToggleFavorite,
    Digit(u8),
    /// Bound explicitly to do nothing (a user override).
    Ignore,
}

/// One mapping. `context: None` applies everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub context: Option<Context>,
    pub key: RemoteKey,
    pub press: Press,
    pub action: Action,
}

const fn b(context: Option<Context>, key: RemoteKey, press: Press, action: Action) -> Binding {
    Binding {
        context,
        key,
        press,
        action,
    }
}

#[derive(Clone, Debug, Default)]
pub struct Keymap {
    defaults: Vec<Binding>,
    overrides: Vec<Binding>,
}

impl Keymap {
    /// iptv-rs's default remote map.
    pub fn defaults() -> Keymap {
        use Action as A;
        use Context::{Controls, Guide, Menu, Player};
        use Press::{Long, Short};
        use RemoteKey as K;
        let g = None;
        let mut defaults = vec![
            b(g, K::Up, Short, A::MoveUp),
            b(g, K::Down, Short, A::MoveDown),
            b(g, K::Left, Short, A::MoveLeft),
            b(g, K::Right, Short, A::MoveRight),
            b(g, K::Ok, Short, A::Select),
            b(g, K::Back, Short, A::Back),
            b(g, K::Menu, Short, A::ShowMenu),
            b(g, K::Guide, Short, A::ShowGuide),
            b(g, K::Info, Short, A::ShowInfo),
            b(g, K::ChannelUp, Short, A::ChannelUp),
            b(g, K::ChannelDown, Short, A::ChannelDown),
            b(g, K::PageUp, Short, A::PageUp),
            b(g, K::PageDown, Short, A::PageDown),
            b(g, K::PlayPause, Short, A::PlayPause),
            b(g, K::Play, Short, A::PlayPause),
            b(g, K::Pause, Short, A::PlayPause),
            b(g, K::Stop, Short, A::Stop),
            b(g, K::Rewind, Short, A::SeekBack),
            b(g, K::FastForward, Short, A::SeekForward),
            b(g, K::Record, Short, A::Record),
            b(g, K::Red, Short, A::ToggleFavorite),
            // Previous/Next (⏮/⏭): last channel and back to live. Free on
            // every remote with media keys; no seeking uses them yet.
            b(g, K::Previous, Short, A::PreviousChannel),
            b(g, K::Next, Short, A::GoLive),
            // Guide: the reference UX's documented long presses.
            b(Some(Guide), K::Ok, Long, A::ShowMenu),
            b(Some(Guide), K::Left, Long, A::PastPrograms),
            b(Some(Guide), K::Back, Long, A::ReturnToPlayer),
            // Ch+/Ch- page the guide's channel list (observed); over video they zap.
            b(Some(Guide), K::ChannelUp, Short, A::PageUp),
            b(Some(Guide), K::ChannelDown, Short, A::PageDown),
            // Full screen, the reference's player defaults: OK shows the
            // panels, Long OK the menu, Up/Down zap, Left the channel list
            // over the video (Long Left beside the video), Right the most
            // recent channel, Long Back stops playback.
            b(Some(Player), K::Ok, Short, A::ShowPanels),
            b(Some(Player), K::Ok, Long, A::ShowMenu),
            b(Some(Player), K::Up, Short, A::ChannelUp),
            b(Some(Player), K::Down, Short, A::ChannelDown),
            b(Some(Player), K::Left, Short, A::ShowChannels),
            b(Some(Player), K::Left, Long, A::ShowChannelsPreview),
            b(Some(Player), K::Right, Short, A::PreviousChannel),
            b(Some(Player), K::Back, Long, A::Stop),
            // Menu opens the control row (observed).
            b(Some(Player), K::Menu, Short, A::ShowPanels),
            // Remotes without media keys: hold 0 for the last channel.
            b(Some(Player), K::Digit(0), Long, A::PreviousChannel),
            // Buttons: directions/OK use the global moves (Up/Down between
            // the quick actions and the control row).
            // Over video there is no list to page: Page Up/Down (CH+/CH- on
            // many remotes, and a PC keyboard's) zap, as in the guide's rows.
            b(Some(Player), K::PageUp, Short, A::ChannelUp),
            b(Some(Player), K::PageDown, Short, A::ChannelDown),
            b(Some(Controls), K::PageUp, Short, A::ChannelUp),
            b(Some(Controls), K::PageDown, Short, A::ChannelDown),
            // Menus: Left and Right do nothing (observed); Back closes.
            b(Some(Menu), K::Left, Short, A::Ignore),
            b(Some(Menu), K::Right, Short, A::Ignore),
        ];
        defaults.extend((0..=9).map(|d| b(g, K::Digit(d), Short, A::Digit(d))));
        Keymap {
            defaults,
            overrides: Vec::new(),
        }
    }

    /// Adds or replaces a user override (same context, key and press).
    pub fn set_override(&mut self, binding: Binding) {
        self.overrides.retain(|o| {
            (o.context, o.key, o.press) != (binding.context, binding.key, binding.press)
        });
        self.overrides.push(binding);
    }

    pub fn clear_overrides(&mut self) {
        self.overrides.clear();
    }

    pub fn overrides(&self) -> &[Binding] {
        &self.overrides
    }

    /// The action for `key` pressed `press` in `context`: overrides before
    /// defaults, the context's before global ones.
    pub fn resolve(&self, context: Context, key: RemoteKey, press: Press) -> Option<Action> {
        let find = |list: &[Binding], ctx: Option<Context>| {
            list.iter()
                .find(|b| b.context == ctx && b.key == key && b.press == press)
                .map(|b| b.action)
        };
        find(&self.overrides, Some(context))
            .or_else(|| find(&self.overrides, None))
            .or_else(|| find(&self.defaults, Some(context)))
            .or_else(|| find(&self.defaults, None))
            .filter(|a| *a != Action::Ignore)
    }

    /// Whether `key` has a long action in `context` (the press detector then
    /// defers its short action until release).
    pub fn has_long(&self, context: Context, key: RemoteKey) -> bool {
        self.resolve(context, key, Press::Long).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Context::{ChannelList, Controls, Groups, Guide, Menu, Player};

    #[test]
    fn the_players_remote_defaults() {
        let k = Keymap::defaults();
        let r = |key, press| k.resolve(Player, key, press);
        assert_eq!(r(RemoteKey::Ok, Press::Short), Some(Action::ShowPanels));
        assert_eq!(r(RemoteKey::Ok, Press::Long), Some(Action::ShowMenu));
        assert_eq!(r(RemoteKey::Back, Press::Short), Some(Action::Back));
        assert_eq!(r(RemoteKey::Back, Press::Long), Some(Action::Stop));
        assert_eq!(r(RemoteKey::Left, Press::Short), Some(Action::ShowChannels));
        assert_eq!(
            r(RemoteKey::Left, Press::Long),
            Some(Action::ShowChannelsPreview)
        );
        assert_eq!(
            r(RemoteKey::Right, Press::Short),
            Some(Action::PreviousChannel)
        );
        assert_eq!(r(RemoteKey::Up, Press::Short), Some(Action::ChannelUp));
        assert_eq!(r(RemoteKey::Down, Press::Short), Some(Action::ChannelDown));
        assert!(
            !k.has_long(Player, RemoteKey::Right),
            "the most recent channel comes at once"
        );
    }

    #[test]
    fn the_channel_list_moves_and_selects_with_the_global_moves() {
        let k = Keymap::defaults();
        let r = |key| k.resolve(ChannelList, key, Press::Short);
        assert_eq!(r(RemoteKey::Up), Some(Action::MoveUp));
        assert_eq!(r(RemoteKey::Left), Some(Action::MoveLeft));
        assert_eq!(r(RemoteKey::Ok), Some(Action::Select));
        assert_eq!(r(RemoteKey::Back), Some(Action::Back));
        assert_eq!(r(RemoteKey::PageDown), Some(Action::PageDown));
        assert!(!k.has_long(ChannelList, RemoteKey::Left));
    }

    #[test]
    fn context_defaults_win_over_global_ones() {
        let k = Keymap::defaults();
        assert_eq!(
            k.resolve(Guide, RemoteKey::Up, Press::Short),
            Some(Action::MoveUp)
        );
        assert_eq!(
            k.resolve(Player, RemoteKey::Up, Press::Short),
            Some(Action::ChannelUp)
        );
        assert_eq!(
            k.resolve(Player, RemoteKey::Ok, Press::Short),
            Some(Action::ShowPanels)
        );
        assert_eq!(
            k.resolve(Groups, RemoteKey::Ok, Press::Short),
            Some(Action::Select)
        );
    }

    #[test]
    fn the_buttons_move_with_the_directions() {
        let k = Keymap::defaults();
        assert_eq!(
            k.resolve(Controls, RemoteKey::Left, Press::Short),
            Some(Action::MoveLeft)
        );
        assert_eq!(
            k.resolve(Controls, RemoteKey::Ok, Press::Short),
            Some(Action::Select)
        );
        assert_eq!(
            k.resolve(Controls, RemoteKey::Down, Press::Short),
            Some(Action::MoveDown),
            "Down goes from the quick actions to the control row"
        );
        assert!(
            !k.has_long(Controls, RemoteKey::Ok),
            "OK presses the button at once"
        );
    }

    #[test]
    fn page_keys_page_the_guide_and_zap_over_video() {
        let k = Keymap::defaults();
        assert_eq!(
            k.resolve(Context::Guide, RemoteKey::PageDown, Press::Short),
            Some(Action::PageDown)
        );
        assert_eq!(
            k.resolve(Context::Guide, RemoteKey::ChannelUp, Press::Short),
            Some(Action::PageUp),
            "Ch+ pages the guide"
        );
        assert_eq!(
            k.resolve(Context::Guide, RemoteKey::ChannelDown, Press::Short),
            Some(Action::PageDown)
        );
        assert_eq!(
            k.resolve(Context::Player, RemoteKey::ChannelUp, Press::Short),
            Some(Action::ChannelUp),
            "and zaps over video"
        );
        for ctx in [Context::Player, Controls] {
            assert_eq!(
                k.resolve(ctx, RemoteKey::PageUp, Press::Short),
                Some(Action::ChannelUp)
            );
            assert_eq!(
                k.resolve(ctx, RemoteKey::PageDown, Press::Short),
                Some(Action::ChannelDown)
            );
        }
    }

    #[test]
    fn menus_move_vertically_and_ignore_left_right() {
        let k = Keymap::defaults();
        assert_eq!(
            k.resolve(Menu, RemoteKey::Down, Press::Short),
            Some(Action::MoveDown)
        );
        assert_eq!(k.resolve(Menu, RemoteKey::Left, Press::Short), None);
        assert_eq!(k.resolve(Menu, RemoteKey::Right, Press::Short), None);
        assert!(!k.has_long(Menu, RemoteKey::Ok));
    }

    #[test]
    fn the_editor_gets_plain_moves_and_select() {
        let k = Keymap::defaults();
        assert_eq!(
            k.resolve(Context::Editor, RemoteKey::Left, Press::Short),
            Some(Action::MoveLeft)
        );
        assert_eq!(
            k.resolve(Context::Editor, RemoteKey::Ok, Press::Short),
            Some(Action::Select)
        );
        assert!(!k.has_long(Context::Editor, RemoteKey::Ok));
    }

    #[test]
    fn long_presses_exist_only_where_bound() {
        let k = Keymap::defaults();
        assert!(k.has_long(Guide, RemoteKey::Ok));
        assert!(k.has_long(Guide, RemoteKey::Left));
        assert!(
            !k.has_long(Groups, RemoteKey::Ok),
            "OK is immediate in the group list"
        );
        assert!(
            !k.has_long(Guide, RemoteKey::Down),
            "directions scroll on repeat"
        );
        assert_eq!(
            k.resolve(Guide, RemoteKey::Back, Press::Long),
            Some(Action::ReturnToPlayer)
        );
    }

    #[test]
    fn overrides_win_and_ignore_unbinds() {
        let mut k = Keymap::defaults();
        k.set_override(Binding {
            context: None,
            key: RemoteKey::Red,
            press: Press::Short,
            action: Action::Record,
        });
        assert_eq!(
            k.resolve(Guide, RemoteKey::Red, Press::Short),
            Some(Action::Record)
        );
        k.set_override(Binding {
            context: Some(Guide),
            key: RemoteKey::Ok,
            press: Press::Long,
            action: Action::Ignore,
        });
        assert!(!k.has_long(Guide, RemoteKey::Ok));
        assert!(
            k.has_long(Player, RemoteKey::Ok),
            "the override is scoped to the guide"
        );
        k.set_override(Binding {
            context: None,
            key: RemoteKey::Red,
            press: Press::Short,
            action: Action::ShowInfo,
        });
        assert_eq!(k.overrides().len(), 2, "same slot replaced, not duplicated");
        k.clear_overrides();
        assert_eq!(
            k.resolve(Guide, RemoteKey::Red, Press::Short),
            Some(Action::ToggleFavorite)
        );
    }

    #[test]
    fn previous_channel_and_live_have_their_own_keys() {
        let k = Keymap::defaults();
        for ctx in [Guide, Groups, Player, Controls] {
            assert_eq!(
                k.resolve(ctx, RemoteKey::Previous, Press::Short),
                Some(Action::PreviousChannel)
            );
            assert_eq!(
                k.resolve(ctx, RemoteKey::Next, Press::Short),
                Some(Action::GoLive)
            );
        }
        assert_eq!(
            k.resolve(Player, RemoteKey::Digit(0), Press::Long),
            Some(Action::PreviousChannel)
        );
        assert_eq!(
            k.resolve(Player, RemoteKey::Digit(0), Press::Short),
            Some(Action::Digit(0)),
            "a short 0 is still a digit"
        );
        assert!(!k.has_long(Guide, RemoteKey::Digit(0)));
    }

    #[test]
    fn digits_and_unknown_combinations() {
        let k = Keymap::defaults();
        assert_eq!(
            k.resolve(Guide, RemoteKey::Digit(7), Press::Short),
            Some(Action::Digit(7))
        );
        assert_eq!(k.resolve(Groups, RemoteKey::Digit(7), Press::Long), None);
    }
}
