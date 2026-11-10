//! Player panels over full-screen video: which panels show, which button
//! has focus, and when they hide on their own.
//!
//! As observed in the reference UX: OK (or a tap) shows a top bar, the info
//! card, the time bar and two quick actions (TV guide, History) whose row
//! takes Left/Right/OK. Down from the quick actions replaces the card with
//! the full control row; Up comes back. A channel change shows the card
//! alone, which takes no input. A moving mouse shows the card too, and the
//! panels then hide sooner once it rests.
//!
//! Every input restarts the auto-hide timer. Times are milliseconds on the
//! caller's monotonic clock, so this stays portable (no clock access here).

/// What is on screen over the video.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Hidden,
    /// Channel change feedback: the top bar and the info card; no input.
    Info,
    /// OK or a tap: the top bar, the info card, the time bar and the quick
    /// actions, which own Left/Right/OK.
    Card,
    /// Down from the quick actions: the control row in place of the card.
    Controls,
}

/// The buttons under the info card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickAction {
    /// Back to the guide, the video in its preview.
    Guide,
    /// The recently played channels.
    History,
}

pub const QUICK_ACTIONS: [QuickAction; 2] = [QuickAction::Guide, QuickAction::History];

/// Buttons in the full-screen control row, in the reference UX's observed
/// order (tivimate docs/08, section E). Play/pause and live are on the
/// remote's media keys, not in the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Search,
    ChannelList,
    Recordings,
    Multiview,
    PictureInPicture,
    VideoTracks,
    AudioTracks,
    AudioOffset,
    Captions,
    Scaling,
    SleepTimer,
    Favorite,
    ChannelOptions,
    Settings,
}

pub const CONTROLS: [Control; 14] = [
    Control::Search,
    Control::ChannelList,
    Control::Recordings,
    Control::Multiview,
    Control::PictureInPicture,
    Control::VideoTracks,
    Control::AudioTracks,
    Control::AudioOffset,
    Control::Captions,
    Control::Scaling,
    Control::SleepTimer,
    Control::Favorite,
    Control::ChannelOptions,
    Control::Settings,
];

/// Auto-hide delays. iptv-rs defaults; they become settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeouts {
    /// The card alone, after a channel change.
    pub info_ms: u64,
    /// The card with its buttons, or the control row.
    pub controls_ms: u64,
    /// Shown by the mouse: once it stops moving.
    pub pointer_ms: u64,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            info_ms: 4_000,
            controls_ms: 7_000,
            pointer_ms: 3_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panels {
    pub shown: Shown,
    /// Index into [`QUICK_ACTIONS`] of the focused quick action.
    pub quick: usize,
    /// Index into [`CONTROLS`] of the focused control.
    pub focus: usize,
    pub timeouts: Timeouts,
    /// The mouse showed (or last kept) them.
    by_pointer: bool,
    hide_at_ms: Option<u64>,
}

impl Default for Panels {
    fn default() -> Self {
        Panels {
            shown: Shown::Hidden,
            quick: 0,
            focus: 0,
            timeouts: Timeouts::default(),
            by_pointer: false,
            hide_at_ms: None,
        }
    }
}

impl Panels {
    /// OK in full screen (or a tap): the card with its quick actions, the
    /// first one focused. Open buttons stay as they are.
    pub fn show_card(&mut self, now_ms: u64) {
        if !self.captures_input() {
            self.shown = Shown::Card;
            self.quick = 0;
        }
        self.by_pointer = false;
        self.touch(now_ms);
    }

    /// Menu in full screen: the control row at once, focus on its first
    /// control unless it was already open.
    pub fn show_controls(&mut self, now_ms: u64) {
        if self.shown != Shown::Controls {
            self.focus = 0;
        }
        self.shown = Shown::Controls;
        self.by_pointer = false;
        self.touch(now_ms);
    }

    /// The card alone, e.g. after a channel change. Keeps open buttons.
    pub fn show_info(&mut self, now_ms: u64) {
        if self.shown == Shown::Hidden {
            self.shown = Shown::Info;
        }
        self.touch(now_ms);
    }

    /// The mouse moved over the video: the card with its buttons, hidden
    /// again once the mouse rests ([`Timeouts::pointer_ms`]).
    pub fn pointer_moved(&mut self, now_ms: u64) {
        if !self.captures_input() {
            self.shown = Shown::Card;
        }
        self.by_pointer = true;
        self.touch(now_ms);
    }

    pub fn hide(&mut self) {
        self.shown = Shown::Hidden;
        self.quick = 0;
        self.focus = 0;
        self.by_pointer = false;
        self.hide_at_ms = None;
    }

    /// Whether a row of buttons owns Left/Right/OK (and Up/Down).
    pub fn captures_input(&self) -> bool {
        matches!(self.shown, Shown::Card | Shown::Controls)
    }

    /// Whether the info card shows (the control row replaces it).
    pub fn card_shown(&self) -> bool {
        matches!(self.shown, Shown::Info | Shown::Card)
    }

    pub fn focused(&self) -> Control {
        CONTROLS[self.focus]
    }

    pub fn quick_focused(&self) -> QuickAction {
        QUICK_ACTIONS[self.quick]
    }

    /// Down: from the quick actions to the control row. Returns whether
    /// it moved.
    pub fn down(&mut self, now_ms: u64) -> bool {
        self.by_pointer = false;
        let moved = self.shown == Shown::Card;
        if moved {
            self.shown = Shown::Controls;
        }
        self.touch(now_ms);
        moved
    }

    /// Up: from the control row back to the card. Returns whether it moved.
    pub fn up(&mut self, now_ms: u64) -> bool {
        self.by_pointer = false;
        let moved = self.shown == Shown::Controls;
        if moved {
            self.shown = Shown::Card;
        }
        self.touch(now_ms);
        moved
    }

    /// Moves focus within the row that has it (clamped, no wrap).
    pub fn move_focus(&mut self, step: isize, now_ms: u64) {
        let clamp =
            |at: usize, len: usize| (at as isize + step).clamp(0, len as isize - 1) as usize;
        match self.shown {
            Shown::Card => self.quick = clamp(self.quick, QUICK_ACTIONS.len()),
            Shown::Controls => self.focus = clamp(self.focus, CONTROLS.len()),
            Shown::Hidden | Shown::Info => {}
        }
        self.by_pointer = false;
        self.touch(now_ms);
    }

    /// Any input while shown: restart the auto-hide timer.
    pub fn touch(&mut self, now_ms: u64) {
        let t = self.timeouts;
        self.hide_at_ms = match self.shown {
            Shown::Hidden => None,
            _ if self.by_pointer => Some(now_ms + t.pointer_ms),
            Shown::Info => Some(now_ms + t.info_ms),
            Shown::Card | Shown::Controls => Some(now_ms + t.controls_ms),
        };
    }

    /// When the panels hide unless something happens (for scheduling).
    pub fn hide_at_ms(&self) -> Option<u64> {
        self.hide_at_ms
    }

    /// Hides once the deadline has passed. Returns whether it hid.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        match self.hide_at_ms {
            Some(at) if now_ms >= at => {
                self.hide();
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_shows_the_card_with_the_guide_button_focused() {
        let mut p = Panels {
            quick: 1,
            ..Panels::default()
        };
        p.show_card(0);
        assert_eq!(p.shown, Shown::Card);
        assert_eq!(p.quick_focused(), QuickAction::Guide);
        assert!(p.captures_input() && p.card_shown());
        assert_eq!(p.hide_at_ms(), Some(7_000));
    }

    #[test]
    fn down_replaces_the_card_with_the_control_row_and_up_returns() {
        let mut p = Panels::default();
        p.show_card(0);
        p.move_focus(1, 1);
        assert_eq!(p.quick_focused(), QuickAction::History);
        assert!(p.down(2));
        assert_eq!((p.shown, p.focused()), (Shown::Controls, Control::Search));
        assert!(!p.card_shown());
        assert!(!p.down(3), "nothing below the control row");
        p.move_focus(5, 4);
        assert_eq!(p.focused(), Control::VideoTracks);
        assert!(p.up(5));
        assert_eq!(
            (p.shown, p.quick_focused()),
            (Shown::Card, QuickAction::History),
            "the quick action keeps its focus"
        );
        assert!(!p.up(6), "nothing above the quick actions");
        assert!(p.down(7));
        assert_eq!(p.focused(), Control::VideoTracks, "the row keeps its focus");
    }

    #[test]
    fn info_does_not_capture_input_and_does_not_drop_open_buttons() {
        let mut p = Panels::default();
        p.show_info(100);
        assert_eq!(p.shown, Shown::Info);
        assert!(!p.captures_input() && p.card_shown());
        assert_eq!(p.hide_at_ms(), Some(4_100));
        assert!(!p.down(150), "the card alone has no buttons to leave");
        p.show_controls(200);
        p.move_focus(2, 300);
        p.show_info(400);
        assert_eq!(
            (p.shown, p.focus),
            (Shown::Controls, 2),
            "a channel change keeps the open controls"
        );
        // OK on the card alone brings its buttons.
        let mut p = Panels::default();
        p.show_info(0);
        p.show_card(10);
        assert_eq!(p.shown, Shown::Card);
    }

    #[test]
    fn menu_opens_the_control_row_directly() {
        let mut p = Panels::default();
        p.show_controls(0);
        assert_eq!((p.shown, p.focused()), (Shown::Controls, Control::Search));
        p.move_focus(3, 1);
        p.show_card(2);
        assert_eq!(p.shown, Shown::Controls, "open buttons stay as they are");
    }

    #[test]
    fn panels_hide_after_the_timeout_and_input_postpones_it() {
        let mut p = Panels::default();
        p.show_controls(0);
        assert!(!p.tick(6_999));
        p.move_focus(1, 5_000);
        assert!(!p.tick(7_000), "input restarted the timer");
        assert!(p.tick(12_000));
        assert_eq!((p.shown, p.hide_at_ms()), (Shown::Hidden, None));
        assert!(!p.tick(20_000));
    }

    #[test]
    fn the_mouse_shows_the_card_and_it_hides_soon_after_the_mouse_rests() {
        let mut p = Panels::default();
        p.pointer_moved(0);
        assert_eq!((p.shown, p.hide_at_ms()), (Shown::Card, Some(3_000)));
        p.pointer_moved(2_000);
        assert!(!p.tick(4_999));
        assert!(p.tick(5_000));
        // Keys after the mouse: the usual timeout again.
        p.pointer_moved(10_000);
        p.move_focus(1, 11_000);
        assert_eq!(p.hide_at_ms(), Some(18_000));
        // The mouse over open controls keeps them.
        p.down(12_000);
        p.pointer_moved(13_000);
        assert_eq!(p.shown, Shown::Controls);
    }

    #[test]
    fn focus_clamps_at_the_ends_and_reopening_starts_at_the_first_button() {
        let mut p = Panels::default();
        p.show_controls(0);
        p.move_focus(-1, 1);
        assert_eq!(p.focus, 0);
        p.move_focus(99, 2);
        assert_eq!(p.focused(), Control::Settings);
        p.show_controls(3);
        assert_eq!(p.focused(), Control::Settings, "still open: focus stays");
        p.hide();
        p.show_controls(4);
        assert_eq!(p.focused(), Control::Search);
        p.up(5);
        p.move_focus(9, 6);
        assert_eq!(p.quick_focused(), QuickAction::History);
        p.down(7);
        p.move_focus(3, 8);
        p.hide();
        p.show_card(9);
        assert_eq!(p.quick_focused(), QuickAction::Guide);
        p.down(10);
        assert_eq!(
            p.focused(),
            Control::Search,
            "a new session starts at the first"
        );
    }
}
