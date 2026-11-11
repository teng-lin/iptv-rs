use super::super::moves::{DAY as DAY_STEP, HOUR};
use super::super::{Move, Pane, Program, SyntheticSchedule, home_window};
use super::*;
use crate::timeline::Timeline;

const S: SyntheticSchedule = SyntheticSchedule;

/// The synthetic programmes with chosen archive windows (`days[c]` for
/// channel `c`) and stored guide (`first` minutes back from now 0).
struct Archive {
    days: Vec<u8>,
    first: i32,
}

impl Schedule for Archive {
    fn browsable(&self, now: i32) -> (i32, i32) {
        (now - self.first, now + super::super::HORIZON)
    }
    fn channel_count(&self) -> usize {
        self.days.len()
    }
    fn group_count(&self) -> usize {
        1
    }
    fn group_start(&self, _group: usize) -> usize {
        0
    }
    fn program_at(&self, channel: usize, minute: i32) -> Program {
        S.program_at(channel, minute)
    }
    fn programs_between(&self, channel: usize, from: i32, minutes: i32) -> Vec<Program> {
        S.programs_between(channel, from, minutes)
    }
    fn catchup_days(&self, channel: usize) -> u8 {
        self.days[channel]
    }
    fn channel_name(&self, channel: usize) -> String {
        format!("Archive {channel}")
    }
    fn group_name(&self, _group: usize) -> String {
        "All".to_owned()
    }
    fn title(&self, _channel: usize, _program: Program) -> String {
        String::new()
    }
}

/// Rows 0 and 1 replay a day, row 2 has no catch-up; a week is stored.
fn mixed() -> Archive {
    Archive {
        days: vec![1, 1, 0],
        first: 7 * DAY,
    }
}

fn on(channel: usize) -> Focus {
    Focus {
        channel,
        ..Focus::at(0)
    }
}

#[test]
fn a_catch_up_row_walks_back_to_its_archive_start_then_opens_the_groups() {
    let s = mixed();
    let mut f = on(0);
    let mut steps = 0;
    while f.pane == Pane::Guide {
        let before = f.program(&s);
        f.apply(Move::Left, &s);
        if f.pane == Pane::Guide {
            assert_eq!(f.program(&s).end, before.start, "one programme back");
            let start = f.program(&s).start;
            assert!(f.window <= start && start < f.window + VISIBLE);
        }
        steps += 1;
        assert!(steps < 200);
    }
    // The last programme reached was the oldest starting in the archive.
    let oldest = f.program(&s);
    assert_eq!(f.floor(&s), -DAY);
    assert!(oldest.start >= -DAY, "{oldest:?}");
    assert!(S.program_at(0, oldest.start - 1).start < -DAY, "{oldest:?}");
    assert!(steps > 10, "a day of programmes: {steps}");
}

#[test]
fn a_row_without_catch_up_opens_the_groups_from_the_programme_on_air() {
    let s = mixed();
    let mut f = on(2);
    let live = f.program(&s);
    f.apply(Move::Left, &s);
    assert_eq!(f.pane, Pane::Groups, "Left at now");
    // From a later programme Left walks back to now's first.
    let mut f = on(2);
    for _ in 0..3 {
        f.apply(Move::Right, &s);
    }
    for _ in 0..3 {
        f.apply(Move::Left, &s);
        assert_eq!(f.pane, Pane::Guide);
    }
    assert_eq!(f.program(&s), live);
    f.apply(Move::Left, &s);
    assert_eq!(f.pane, Pane::Groups);
    // Earlier programmes by the action never pass now either.
    let mut f = on(2);
    f.enter_past(&s);
    assert_eq!((f.pane, f.program(&s)), (Pane::Guide, live));
}

#[test]
fn the_stored_guide_bounds_a_longer_archive() {
    // Seven days of archive, two of guide kept.
    let s = Archive {
        days: vec![7],
        first: 2 * DAY,
    };
    let mut f = on(0);
    assert_eq!(f.floor(&s), -2 * DAY);
    assert_eq!(view_range(&s, 0).0, -2 * DAY);
    while f.pane == Pane::Guide {
        f.apply(Move::Left, &s);
    }
    let start = f.program(&s).start;
    assert!(start >= -2 * DAY && S.program_at(0, start - 1).start < -2 * DAY);
}

#[test]
fn rows_keep_the_time_column_but_not_before_their_floor() {
    let s = mixed();
    let mut f = on(0);
    for _ in 0..5 {
        f.apply(Move::Left, &s);
    }
    let cursor = f.cursor;
    assert!(cursor < 0);
    // Catch-up to catch-up: the same time column.
    f.apply(Move::Down, &s);
    assert_eq!((f.channel, f.cursor), (1, cursor));
    // Into a row without catch-up: its programme on air.
    f.apply(Move::Down, &s);
    assert_eq!((f.channel, f.cursor), (2, 0));
    assert_eq!(f.program(&s).airing(0), super::super::Airing::Live);
    let start = f.program(&s).start;
    assert!(f.window <= start.max(0) && 0 < f.window + VISIBLE);
}

#[test]
fn hour_and_day_steps_stop_at_the_floor() {
    let s = mixed();
    // No catch-up: nothing before now's programme.
    let mut f = on(2);
    f.step_time(-HOUR, &s);
    f.step_time(-DAY_STEP, &s);
    assert_eq!((f.cursor, f.window), (0, home_window(0)));
    // Catch-up: back to the oldest replay, and on to the guide's end.
    let mut f = on(0);
    for _ in 0..5 {
        f.step_time(-DAY_STEP, &s);
    }
    let start = f.program(&s).start;
    assert!(start >= -DAY && S.program_at(0, start - 1).start < -DAY);
    assert!(f.window <= f.cursor && f.cursor < f.window + VISIBLE);
    for _ in 0..5 {
        f.step_time(DAY_STEP, &s);
    }
    let (_, end) = view_range(&s, 0);
    assert_eq!(f.window, end - VISIBLE);
}

#[test]
fn a_tap_on_a_past_programme_without_replay_focuses_now() {
    let s = mixed();
    let mut f = on(0);
    f.tap(2, -90, &s);
    assert_eq!((f.channel, f.cursor), (2, 0));
    f.tap(1, -90, &s);
    assert_eq!((f.channel, f.cursor), (1, -90), "a replay is focused");
}

#[test]
fn the_view_reaches_back_only_for_catch_up_lists() {
    let none = Archive {
        days: vec![0, 0],
        first: 7 * DAY,
    };
    let (first, end) = view_range(&none, 0);
    assert_eq!((first, end), (home_window(0), super::super::HORIZON));
    assert_eq!(view_range(&mixed(), 0).0, -DAY);
    // Pans and their settled windows stay inside.
    for (s, lo) in [(&none, home_window(0)), (&mixed(), -DAY)] {
        let mut t = Timeline::new(home_window(0), view_range(s, 0));
        t.press();
        t.drag(1.0e6, 0.0, 8.0);
        assert_eq!(t.view(), lo as f32);
        let mut f = on(0);
        f.pan_to(-10 * DAY, s);
        assert_eq!(f.window, lo);
    }
    // A pan of a catch-up list leaves a row without one on now's programme.
    let mut f = on(2);
    f.pan_to(-DAY, &mixed());
    assert_eq!((f.window, f.cursor), (-DAY, 0));
}

#[test]
fn a_synthetic_guide_has_catch_up_rows_and_rows_without() {
    // Even channels replay three days, odd ones none.
    assert_eq!(floor_of(&S, 1, 0), 0);
    assert_eq!(floor_of(&S, 0, 0), -3 * DAY);
    assert_eq!(view_range(&S, 0).0, -3 * DAY);
}
