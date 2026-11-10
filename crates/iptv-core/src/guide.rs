//! The guide's data and its remote-control focus model, free of UI so the
//! navigation rules are unit-tested on the host.
//!
//! Focus is owned by the app: NSH's Android backend gives ordinary views no
//! D-pad focus, so the keymap feeds [`Focus::apply`] and the UI draws the
//! focused cell from it. Focus and playback are separate states.

/// Synthetic channels in the guide.
pub const CHANNELS: usize = 5000;
/// Synthetic groups in the side column.
pub const GROUPS: usize = 24;
/// Minutes of guide data, from "now" (minute 0).
pub const HORIZON: i32 = 12 * 60;
/// Minutes visible across the guide at once.
pub const VISIBLE: i32 = 150;
/// The time bar's step when the window has to move.
pub const STEP: i32 = 30;
/// How far before "now" the guide can show (the window's lower bound).
pub const PAST: i32 = 30;
/// How far back the guide has data and can be browsed (Long Left). Not tied
/// to catch-up: past programmes without an archive are shown too.
pub const HISTORY: i32 = 7 * 24 * 60;

/// Where a programme sits relative to now (minute 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Airing {
    Ended,
    Live,
    Upcoming,
}

impl Program {
    pub fn airing(&self) -> Airing {
        if self.end <= 0 {
            Airing::Ended
        } else if self.start <= 0 {
            Airing::Live
        } else {
            Airing::Upcoming
        }
    }

    /// Played fraction of a live programme (0 otherwise).
    pub fn progress(&self) -> f32 {
        match self.airing() {
            Airing::Live => -self.start as f32 / (self.end - self.start) as f32,
            _ => 0.0,
        }
    }
}

/// A programme: `[start, end)` in minutes from now (`start` can be < 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Program {
    pub start: i32,
    pub end: i32,
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x
}

/// The `n`th programme of `channel`, deterministic and cheap to recompute.
fn program_seq(channel: usize) -> impl Iterator<Item = Program> {
    const LENGTHS: [i32; 8] = [15, 30, 30, 45, 60, 60, 90, 120];
    let offset = (mix(channel as u64) % 60) as i32;
    // Starts before the guide's earliest minute (-HISTORY), so no row is blank.
    let mut start = -offset - HISTORY - 60;
    (0u64..).map(move |n| {
        let len = LENGTHS[(mix((channel as u64) << 20 | n) % 8) as usize];
        let p = Program {
            start,
            end: start + len,
        };
        start += len;
        p
    })
}

/// Synthetic catch-up window: even channels keep a 3-day archive, odd
/// channels have none (so both cases are always on screen).
pub fn catchup_days(channel: usize) -> u8 {
    if channel % 2 == 0 { 3 } else { 0 }
}

/// The programme of `channel` airing at `minute` (clamped to the guide's data).
pub fn program_at(channel: usize, minute: i32) -> Program {
    let minute = minute.clamp(-HISTORY, HORIZON - 1);
    program_seq(channel)
        .find(|p| p.end > minute)
        .expect("endless")
}

/// The programmes of `channel` overlapping `[from, from + VISIBLE)`.
pub fn visible_programs(channel: usize, from: i32) -> Vec<Program> {
    let to = from + VISIBLE;
    program_seq(channel)
        .skip_while(|p| p.end <= from)
        .take_while(|p| p.start < to)
        .collect()
}

/// Which surface owns the D-pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Groups,
    Guide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Up,
    Down,
    Left,
    Right,
}

/// The remote's focus: the pane, the focused group and channel, and the
/// time cursor that Up/Down keep, so moving between rows stays in the same
/// time column (TiviMate-style guide navigation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Focus {
    pub pane: Pane,
    pub group: usize,
    pub channel: usize,
    pub cursor: i32,
    /// The first minute shown across the guide.
    pub window: i32,
}

impl Default for Focus {
    /// The guide at the first channel, now, with half an hour of the past shown.
    fn default() -> Self {
        Focus {
            pane: Pane::Guide,
            group: 0,
            channel: 0,
            cursor: 0,
            window: -PAST,
        }
    }
}

/// The first channel of `group`.
pub fn group_start(group: usize) -> usize {
    group * CHANNELS.div_ceil(GROUPS)
}

impl Focus {
    pub fn apply(&mut self, m: Move) {
        match (self.pane, m) {
            (Pane::Groups, Move::Up) => self.group = self.group.saturating_sub(1),
            (Pane::Groups, Move::Down) => self.group = (self.group + 1).min(GROUPS - 1),
            (Pane::Groups, Move::Right) => self.enter_group(),
            (Pane::Groups, Move::Left) => {}
            (Pane::Guide, Move::Up) => self.channel = self.channel.saturating_sub(1),
            (Pane::Guide, Move::Down) => self.channel = (self.channel + 1).min(CHANNELS - 1),
            // Left on the live programme shows the groups; anywhere else it
            // steps to the previous programme (the past is entered by Long Left).
            (Pane::Guide, Move::Left) => {
                if self.program().airing() == Airing::Live {
                    self.pane = Pane::Groups;
                } else {
                    self.step_back();
                }
            }
            (Pane::Guide, Move::Right) => {
                let current = program_at(self.channel, self.cursor);
                if current.end < HORIZON {
                    self.cursor = current.end;
                }
            }
        }
        self.keep_cursor_visible();
    }

    /// Long Left: into the past from the live programme (or one programme
    /// back from anywhere else), never to the groups.
    pub fn enter_past(&mut self) {
        if self.pane == Pane::Guide {
            self.step_back();
            self.keep_cursor_visible();
        }
    }

    /// Back on a past or future programme: the channel's live programme and
    /// the default window. Returns false when already there.
    pub fn return_to_now(&mut self) -> bool {
        if self.pane != Pane::Guide || self.program().airing() == Airing::Live {
            return false;
        }
        self.cursor = 0;
        self.window = -PAST;
        true
    }

    fn step_back(&mut self) {
        let current = self.program();
        if current.start > -HISTORY {
            self.cursor = program_at(self.channel, current.start - 1)
                .start
                .max(-HISTORY);
        }
    }

    /// OK or Right on a group: the guide at its first channel, now.
    pub fn enter_group(&mut self) {
        self.pane = Pane::Guide;
        self.channel = group_start(self.group).min(CHANNELS - 1);
        self.cursor = 0;
        self.window = -PAST;
    }

    /// A tap on a group: the same as focusing it and pressing OK.
    pub fn tap_group(&mut self, group: usize) {
        self.group = group.min(GROUPS - 1);
        self.enter_group();
    }

    /// A tap on the programme of `channel` airing at `minute`: it gets the
    /// focus where it is (the window does not move, the cell is on screen).
    pub fn tap(&mut self, channel: usize, minute: i32) {
        self.pane = Pane::Guide;
        self.channel = channel.min(CHANNELS - 1);
        self.cursor = minute.clamp(-HISTORY, HORIZON - 1);
    }

    /// The time window moved by touch: the cursor follows into it.
    pub fn shift_window(&mut self, minutes: i32) {
        self.window = (self.window + minutes).clamp(-HISTORY, HORIZON - VISIBLE);
        let lo = self.window;
        self.cursor = self.cursor.clamp(lo, (self.window + VISIBLE - 1).max(lo));
    }

    /// After a touch scroll, the first key focuses a row on screen instead
    /// of jumping back to a row the user scrolled away from.
    pub fn adopt_rows(&mut self, first: usize, last: usize) {
        if self.channel < first || self.channel > last {
            self.channel = if self.channel < first { first } else { last };
        }
    }

    /// The focused programme (in the guide pane).
    pub fn program(&self) -> Program {
        program_at(self.channel, self.cursor)
    }

    fn keep_cursor_visible(&mut self) {
        // The focused programme's start (now, for a live one) stays on
        // screen; the window moves in steps.
        let p = self.program();
        let anchor = if p.airing() == Airing::Live {
            0
        } else {
            p.start
        };
        // In (or before) the first column: step back so a column of context
        // shows before it (half an hour of the past when it is live).
        if anchor < self.window + STEP {
            self.window = (anchor - STEP).div_euclid(STEP) * STEP;
        }
        while anchor >= self.window + VISIBLE - STEP {
            self.window += STEP;
        }
        self.window = self.window.clamp(-HISTORY, HORIZON - VISIBLE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules_are_contiguous_and_cover_now() {
        for ch in [0, 1, 77, CHANNELS - 1] {
            let ps = visible_programs(ch, 0);
            assert!(ps[0].start <= 0 && ps[0].end > 0);
            for w in ps.windows(2) {
                assert_eq!(w[0].end, w[1].start);
            }
            assert!(ps.last().unwrap().end >= VISIBLE);
        }
    }

    #[test]
    fn up_down_keep_the_time_column() {
        let mut f = Focus::default();
        for _ in 0..4 {
            f.apply(Move::Right);
        }
        let cursor = f.cursor;
        assert!(cursor > 0);
        for _ in 0..50 {
            f.apply(Move::Down);
            assert_eq!(f.cursor, cursor, "Down must not drift the time cursor");
            let p = f.program();
            assert!(p.start <= cursor && cursor < p.end);
        }
        assert_eq!(f.channel, 50);
    }

    #[test]
    fn left_from_the_current_programme_moves_to_groups_and_back() {
        let mut f = Focus {
            channel: 9,
            ..Focus::default()
        };
        f.apply(Move::Right);
        f.apply(Move::Left);
        assert_eq!(f.pane, Pane::Guide);
        f.apply(Move::Left);
        assert_eq!(f.pane, Pane::Groups);
        f.apply(Move::Down);
        f.apply(Move::Right);
        assert_eq!(
            (f.pane, f.channel, f.cursor),
            (Pane::Guide, group_start(1), 0)
        );
    }

    #[test]
    fn the_window_follows_the_cursor_within_the_horizon() {
        let mut f = Focus::default();
        for _ in 0..500 {
            f.apply(Move::Right);
            let start = f.program().start.max(0);
            assert!(f.window <= start && start < f.window + VISIBLE);
        }
        assert!(f.window <= HORIZON - VISIBLE);
    }

    #[test]
    fn the_guide_opens_with_the_past_half_hour_shown() {
        let f = Focus::default();
        assert_eq!((f.window, f.cursor), (-PAST, 0));
        assert_eq!(f.program().airing(), Airing::Live);
    }

    #[test]
    fn airing_and_progress() {
        let live = Program {
            start: -15,
            end: 45,
        };
        assert_eq!(live.airing(), Airing::Live);
        assert!((live.progress() - 0.25).abs() < 1e-6);
        assert_eq!(Program { start: -60, end: 0 }.airing(), Airing::Ended);
        assert_eq!(Program { start: 10, end: 40 }.airing(), Airing::Upcoming);
    }

    #[test]
    fn a_tap_focuses_in_place_without_moving_the_window() {
        let mut f = Focus {
            pane: Pane::Groups,
            ..Focus::default()
        };
        f.tap(321, 50);
        assert_eq!((f.pane, f.channel, f.window), (Pane::Guide, 321, -PAST));
        let p = f.program();
        assert!(p.start <= 50 && 50 < p.end);
    }

    #[test]
    fn shifting_the_window_by_touch_pulls_the_cursor_along() {
        let mut f = Focus::default();
        f.shift_window(3 * STEP);
        assert_eq!(f.window, 60);
        assert!(f.cursor >= 60 && f.cursor < 60 + VISIBLE);
        // Touch can browse back into history too (Long Left's range).
        f.shift_window(-10 * STEP);
        assert_eq!(f.window, 60 - 10 * STEP);
        assert!(f.cursor >= f.window && f.cursor < f.window + VISIBLE);
        f.shift_window(-HISTORY * 2);
        assert_eq!(f.window, -HISTORY);
        f.shift_window(HISTORY + HORIZON);
        assert_eq!(f.window, HORIZON - VISIBLE);
    }

    #[test]
    fn keys_after_a_touch_scroll_start_from_a_visible_row() {
        let mut f = Focus {
            channel: 10,
            ..Focus::default()
        };
        f.adopt_rows(1740, 1760);
        assert_eq!(f.channel, 1740);
        f.channel = 4000;
        f.adopt_rows(1740, 1760);
        assert_eq!(f.channel, 1760);
        f.adopt_rows(1700, 1800);
        assert_eq!(f.channel, 1760, "a visible row keeps the focus");
    }

    #[test]
    fn stepping_left_back_to_now_shows_the_past_again() {
        let mut f = Focus::default();
        for _ in 0..20 {
            f.apply(Move::Right);
        }
        assert!(f.window > 0);
        while f.program().start > 0 {
            f.apply(Move::Left);
        }
        assert_eq!(f.window, -PAST);
    }

    #[test]
    fn right_then_left_into_the_groups_keeps_the_past_shown() {
        // The device sequence: Channel 15, Right x3, Left x4 (the last opens the groups).
        for channel in [14, 0, 1, 2, 77] {
            let mut f = Focus {
                channel,
                ..Focus::default()
            };
            for _ in 0..3 {
                f.apply(Move::Right);
            }
            for _ in 0..4 {
                f.apply(Move::Left);
            }
            assert_eq!(f.window, -PAST, "channel {channel}: {f:?}");
        }
    }

    #[test]
    fn long_left_enters_the_past_and_left_right_step_programmes() {
        let mut f = Focus {
            channel: 3,
            ..Focus::default()
        };
        let live = f.program();
        f.enter_past();
        let first = f.program();
        assert_eq!(first.end, live.start, "the programme before the live one");
        assert_eq!(f.pane, Pane::Guide);
        f.apply(Move::Left);
        assert_eq!(
            f.program().end,
            first.start,
            "short Left steps back in the past"
        );
        f.apply(Move::Right);
        assert_eq!(f.program(), first);
        let start = f.program().start;
        assert!(
            f.window <= start && start < f.window + VISIBLE,
            "the grid follows into the past"
        );
    }

    #[test]
    fn up_down_keep_the_past_time_column_and_back_returns_to_now() {
        let mut f = Focus {
            channel: 10,
            ..Focus::default()
        };
        for _ in 0..6 {
            f.enter_past();
        }
        let cursor = f.cursor;
        assert!(cursor < 0);
        f.apply(Move::Down);
        assert_eq!(f.cursor, cursor);
        assert_eq!(f.program().airing(), Airing::Ended);
        assert!(f.return_to_now());
        assert_eq!(
            (f.cursor, f.window, f.program().airing()),
            (0, -PAST, Airing::Live)
        );
        assert!(!f.return_to_now(), "already at now");
    }

    #[test]
    fn the_past_reaches_the_history_limit_regardless_of_catchup() {
        let mut f = Focus {
            channel: 1,
            ..Focus::default()
        }; // odd: no catch-up
        assert_eq!(catchup_days(1), 0);
        for _ in 0..2000 {
            f.enter_past();
        }
        assert!(
            f.program().start <= -HISTORY + 120,
            "a week back: {:?}",
            f.program()
        );
        assert!(f.window >= -HISTORY);
    }

    #[test]
    fn edges_clamp() {
        let mut f = Focus::default();
        f.apply(Move::Up);
        assert_eq!(f.channel, 0);
        f.channel = CHANNELS - 1;
        f.apply(Move::Down);
        assert_eq!(f.channel, CHANNELS - 1);
    }
}
