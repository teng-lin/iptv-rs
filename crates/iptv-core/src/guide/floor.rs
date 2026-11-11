//! How far back the guide goes: the past is only for replays.
//!
//! - **The view** ([`view_range`]): from now's default window (the current
//!   half hour and the one before it), unless the list shown has catch-up
//!   channels: then from the oldest archive start among them, within the
//!   stored guide ([`Schedule::browsable`]). Forward, to the end of the
//!   stored guide. Pans, flings, the wheel and ‹ › stay inside it.
//! - **The keys** ([`Focus::floor`]): on a channel with catch-up the focus
//!   goes back to the oldest programme inside its archive window (and the
//!   stored guide: it starts at or after the floor); on one without, to
//!   the programme on air now. Left on that programme opens the groups
//!   ([`Focus::apply`]); hour and day steps, taps and pans never put the
//!   focus before it.

use super::{Focus, Program, Schedule, VISIBLE, home_window};

/// Minutes in a day: archive windows are whole days.
const DAY: i32 = 24 * 60;

/// The minutes the guide's view can show at `now` on `s` (the list on
/// screen), `[first, end)`: see the module's rules.
pub fn view_range(s: &dyn Schedule, now: i32) -> (i32, i32) {
    let (first, end) = s.browsable(now);
    let home = home_window(now);
    let days = i32::from(s.max_catchup_days());
    let first = if days > 0 {
        first.max(now - days * DAY)
    } else {
        home
    };
    (first.min(home), end.max(now + VISIBLE))
}

/// Where `channel`'s replays begin at `now` on `s`: its archive window's
/// start within the stored guide, or `now` without catch-up. Arithmetic
/// only: no programme is read (an older day would be paged in for it).
pub fn floor_of(s: &dyn Schedule, channel: usize, now: i32) -> i32 {
    let days = i32::from(s.catchup_days(channel));
    if days == 0 {
        return now;
    }
    s.browsable(now).0.max(now - days * DAY).min(now)
}

impl Focus {
    /// [`floor_of`] the focused row.
    pub fn floor(&self, s: &dyn Schedule) -> i32 {
        floor_of(s, self.channel, self.now)
    }

    /// Whether the keys may focus `p` on the focused row: it is on air or
    /// later, or a replay (it starts at or after the row's floor).
    pub(crate) fn reachable(&self, p: Program, s: &dyn Schedule) -> bool {
        p.end > self.now || p.start >= self.floor(s)
    }

    /// The programme before the focused one, when the keys may focus it
    /// (else Left opens the groups). Reads only that programme.
    pub(crate) fn previous(&self, s: &dyn Schedule) -> Option<Program> {
        let current = self.program(s);
        let p = s.program_at(self.channel, current.start - 1);
        (p.start < current.start && self.reachable(p, s)).then_some(p)
    }

    /// Keeps the cursor on a programme the keys may focus, at or after the
    /// row's floor. Lifted to now (a row without catch-up) the window goes
    /// to now's default; lifted to the oldest replay it comes along.
    pub(crate) fn lift_to_floor(&mut self, s: &dyn Schedule) {
        let before = self.cursor;
        self.cursor = self.cursor.max(self.floor(s));
        // The programme across the archive's start is not a replay.
        let p = self.program(s);
        if !self.reachable(p, s) {
            self.cursor = p.end.min(self.now);
        }
        if self.cursor == before {
            return;
        }
        if self.cursor == self.now {
            self.window = home_window(self.now);
        } else {
            self.keep_cursor_visible(s);
        }
    }
}

#[cfg(test)]
mod tests;
