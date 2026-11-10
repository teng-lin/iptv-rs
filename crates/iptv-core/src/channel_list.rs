//! The channel list over full-screen video (the player's Left; Long Left
//! shows it beside the video shrunk to a preview), as the reference's
//! player defaults have it: the playing group's channels with their
//! programmes, Up/Down move, OK plays (OK on what plays closes the list),
//! Left shows the groups, Back steps back and closes. Focus is kept by
//! channel and group index into the caller's [`Schedule`].

use crate::guide::Schedule;
use crate::playback::group_range;

/// How the list sits over the video.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListMode {
    /// Over the full-screen video.
    Overlay,
    /// Beside the video, which shrinks to a preview.
    Preview,
}

/// Which list has the focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListPane {
    Channels,
    Groups,
}

/// What a key did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The list changed (or nothing happened).
    Stay,
    /// Play this channel; the list stays.
    Play(usize),
    /// Close the list.
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelList {
    pub mode: ListMode,
    pub pane: ListPane,
    /// The group whose channels are listed.
    pub group: usize,
    /// The focused channel (an index into the schedule, inside `group`).
    pub channel: usize,
    /// The focused group in the groups pane.
    pub group_focus: usize,
}

/// The group holding `channel` (groups start in ascending order; 0 without
/// groups).
pub fn group_of(s: &dyn Schedule, channel: usize) -> usize {
    let n = s.group_count();
    if n == 0 {
        return 0;
    }
    let (mut lo, mut hi) = (0, n);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if s.group_start(mid) <= channel {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

impl ChannelList {
    /// Opens on `playing` (else the first channel) in its group.
    pub fn open(mode: ListMode, playing: Option<usize>, s: &dyn Schedule) -> ChannelList {
        let last = s.channel_count().saturating_sub(1);
        let channel = playing.unwrap_or(0).min(last);
        let group = group_of(s, channel);
        ChannelList {
            mode,
            pane: ListPane::Channels,
            group,
            channel,
            group_focus: group,
        }
    }

    /// The channels listed (a range of schedule indices).
    pub fn channels(&self, s: &dyn Schedule) -> std::ops::Range<usize> {
        if s.group_count() == 0 {
            return 0..s.channel_count();
        }
        let start = s.group_start(self.group.min(s.group_count() - 1));
        group_range(s, start)
    }

    /// Up/Down (and pages): moves within the pane's list, clamped.
    pub fn move_focus(&mut self, step: isize, s: &dyn Schedule) {
        match self.pane {
            ListPane::Channels => {
                let r = self.channels(s);
                if r.is_empty() {
                    return;
                }
                let to = (self.channel as isize + step).clamp(r.start as isize, r.end as isize - 1);
                self.channel = to as usize;
            }
            ListPane::Groups => {
                let last = s.group_count().saturating_sub(1) as isize;
                self.group_focus = (self.group_focus as isize + step).clamp(0, last) as usize;
            }
        }
    }

    /// Left: from the channels to the groups.
    pub fn left(&mut self) {
        if self.pane == ListPane::Channels {
            self.pane = ListPane::Groups;
            self.group_focus = self.group;
        }
    }

    /// Right: from the groups back to the focused group's channels.
    pub fn right(&mut self, s: &dyn Schedule) {
        if self.pane == ListPane::Groups {
            self.enter_group(self.group_focus, s);
        }
    }

    /// Lists `group`'s channels, focus on its first.
    pub fn enter_group(&mut self, group: usize, s: &dyn Schedule) {
        if s.group_count() == 0 {
            self.pane = ListPane::Channels;
            return;
        }
        let group = group.min(s.group_count() - 1);
        if group != self.group {
            self.group = group;
            self.channel = self.channels(s).start;
        }
        self.group_focus = group;
        self.pane = ListPane::Channels;
    }

    /// OK: a channel plays (on the one playing: the list closes); a group
    /// opens.
    pub fn select(&mut self, playing: Option<usize>, s: &dyn Schedule) -> Outcome {
        match self.pane {
            ListPane::Groups => {
                self.enter_group(self.group_focus, s);
                Outcome::Stay
            }
            ListPane::Channels if playing == Some(self.channel) => Outcome::Close,
            ListPane::Channels => Outcome::Play(self.channel),
        }
    }

    /// Back: from the groups to the channels, else closes.
    pub fn back(&mut self) -> Outcome {
        match self.pane {
            ListPane::Groups => {
                self.pane = ListPane::Channels;
                Outcome::Stay
            }
            ListPane::Channels => Outcome::Close,
        }
    }

    /// The playing channel changed (a zap while the list shows): follow it.
    pub fn follow(&mut self, playing: usize, s: &dyn Schedule) {
        self.group = group_of(s, playing);
        self.channel = playing;
        self.group_focus = self.group;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guide::{SyntheticSchedule, group_start};

    const S: SyntheticSchedule = SyntheticSchedule;

    #[test]
    fn opens_on_the_playing_channel_in_its_group() {
        let playing = group_start(2) + 3;
        let l = ChannelList::open(ListMode::Overlay, Some(playing), &S);
        assert_eq!(
            (l.group, l.channel, l.pane),
            (2, playing, ListPane::Channels)
        );
        assert_eq!(l.channels(&S), group_start(2)..group_start(3));
        let none = ChannelList::open(ListMode::Preview, None, &S);
        assert_eq!((none.group, none.channel), (0, 0));
    }

    #[test]
    fn moves_stay_inside_the_group() {
        let mut l = ChannelList::open(ListMode::Overlay, Some(group_start(1)), &S);
        l.move_focus(-1, &S);
        assert_eq!(l.channel, group_start(1), "clamped at the group's first");
        l.move_focus(1000, &S);
        assert_eq!(l.channel, group_start(2) - 1, "and at its last");
    }

    #[test]
    fn left_shows_the_groups_and_a_group_lists_its_channels() {
        let mut l = ChannelList::open(ListMode::Overlay, Some(group_start(1) + 2), &S);
        l.left();
        assert_eq!((l.pane, l.group_focus), (ListPane::Groups, 1));
        l.move_focus(1, &S);
        assert_eq!(l.select(None, &S), Outcome::Stay);
        assert_eq!(
            (l.pane, l.group, l.channel),
            (ListPane::Channels, 2, group_start(2))
        );
        // Back to the playing group: Right enters the focused group too.
        l.left();
        l.move_focus(-1, &S);
        l.right(&S);
        assert_eq!((l.group, l.channel), (1, group_start(1)));
        // Back from the groups returns to the channels, then closes.
        l.left();
        assert_eq!(l.back(), Outcome::Stay);
        assert_eq!(l.pane, ListPane::Channels);
        assert_eq!(l.back(), Outcome::Close);
    }

    #[test]
    fn ok_plays_and_ok_on_the_playing_channel_closes() {
        let playing = group_start(0) + 1;
        let mut l = ChannelList::open(ListMode::Overlay, Some(playing), &S);
        assert_eq!(l.select(Some(playing), &S), Outcome::Close);
        l.move_focus(1, &S);
        assert_eq!(l.select(Some(playing), &S), Outcome::Play(playing + 1));
        l.follow(group_start(3), &S);
        assert_eq!((l.group, l.channel), (3, group_start(3)));
    }

    #[test]
    fn the_group_of_a_channel() {
        assert_eq!(group_of(&S, 0), 0);
        assert_eq!(group_of(&S, group_start(1)), 1);
        assert_eq!(group_of(&S, group_start(1) - 1), 0);
    }
}
