//! Player panels over full-screen video: which panels show, which control
//! has focus, and when they hide on their own.
//!
//! The info panel only informs and never takes input; the control panel
//! takes Left/Right/OK while shown. Every input restarts the auto-hide
//! timer. Times are milliseconds on the caller's monotonic clock, so this
//! stays portable (no clock access here).

/// What is on screen over the video.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Hidden,
    /// Channel and programme information only.
    Info,
    /// Info and the control row.
    Both,
}

/// Buttons in the control row, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    PlayPause,
    GoLive,
    Audio,
    Subtitles,
    Guide,
    Favorite,
}

pub const CONTROLS: [Control; 6] = [
    Control::PlayPause,
    Control::GoLive,
    Control::Audio,
    Control::Subtitles,
    Control::Guide,
    Control::Favorite,
];

/// Auto-hide delays. iptv-rs defaults; they become settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeouts {
    pub info_ms: u64,
    pub controls_ms: u64,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            info_ms: 4_000,
            controls_ms: 7_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panels {
    pub shown: Shown,
    /// Index into [`CONTROLS`] of the focused button.
    pub focus: usize,
    pub timeouts: Timeouts,
    hide_at_ms: Option<u64>,
}

impl Default for Panels {
    fn default() -> Self {
        Panels {
            shown: Shown::Hidden,
            focus: 0,
            timeouts: Timeouts::default(),
            hide_at_ms: None,
        }
    }
}

impl Panels {
    /// OK in full screen (or a tap): info and controls, focus on the first
    /// control.
    pub fn show_controls(&mut self, now_ms: u64) {
        if self.shown != Shown::Both {
            self.focus = 0;
        }
        self.shown = Shown::Both;
        self.hide_at_ms = Some(now_ms + self.timeouts.controls_ms);
    }

    /// Info only, e.g. after a channel change. Keeps controls if shown.
    pub fn show_info(&mut self, now_ms: u64) {
        if self.shown == Shown::Both {
            self.touch(now_ms);
            return;
        }
        self.shown = Shown::Info;
        self.hide_at_ms = Some(now_ms + self.timeouts.info_ms);
    }

    pub fn hide(&mut self) {
        self.shown = Shown::Hidden;
        self.hide_at_ms = None;
    }

    /// Whether the control row owns Left/Right/OK.
    pub fn captures_input(&self) -> bool {
        self.shown == Shown::Both
    }

    pub fn focused(&self) -> Control {
        CONTROLS[self.focus]
    }

    /// Moves focus within the control row (clamped, no wrap).
    pub fn move_focus(&mut self, step: isize, now_ms: u64) {
        let last = CONTROLS.len() as isize - 1;
        self.focus = (self.focus as isize + step).clamp(0, last) as usize;
        self.touch(now_ms);
    }

    /// Any input while shown: restart the auto-hide timer.
    pub fn touch(&mut self, now_ms: u64) {
        self.hide_at_ms = match self.shown {
            Shown::Hidden => None,
            Shown::Info => Some(now_ms + self.timeouts.info_ms),
            Shown::Both => Some(now_ms + self.timeouts.controls_ms),
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
    fn ok_shows_controls_focused_on_the_first_button() {
        let mut p = Panels {
            focus: 3,
            ..Panels::default()
        };
        p.show_controls(0);
        assert_eq!((p.shown, p.focused()), (Shown::Both, Control::PlayPause));
        assert!(p.captures_input());
        assert_eq!(p.hide_at_ms(), Some(7_000));
    }

    #[test]
    fn info_does_not_capture_input_and_does_not_drop_controls() {
        let mut p = Panels::default();
        p.show_info(100);
        assert_eq!(p.shown, Shown::Info);
        assert!(!p.captures_input());
        assert_eq!(p.hide_at_ms(), Some(4_100));
        p.show_controls(200);
        p.move_focus(2, 300);
        p.show_info(400);
        assert_eq!(
            (p.shown, p.focus),
            (Shown::Both, 2),
            "a channel change keeps the open controls"
        );
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
    fn focus_clamps_at_the_ends_and_reopening_starts_at_the_first_control() {
        let mut p = Panels::default();
        p.show_controls(0);
        p.move_focus(-1, 1);
        assert_eq!(p.focus, 0);
        p.move_focus(99, 2);
        assert_eq!(p.focused(), Control::Favorite);
        p.show_controls(3);
        assert_eq!(p.focused(), Control::Favorite, "still open: focus stays");
        p.hide();
        p.show_controls(4);
        assert_eq!(p.focused(), Control::PlayPause);
    }
}
