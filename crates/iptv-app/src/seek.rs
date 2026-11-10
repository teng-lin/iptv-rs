//! Seeking while a programme plays from the archive (catch-up).
//!
//! Archive addresses are time-based, so a seek opens the archive again from
//! the new time (`Playback::CatchUp::offset`); that works the same for every
//! provider and container. Presses add up to one target, shown on the info
//! card, and the archive reopens once the keys pause.
//!
//! Steps are the `catchup.*_step` settings' defaults (the settings are not
//! wired in yet): fast-forward 30 s, rewind 10 s, one time-bar step
//! (Left/Right) 5 min.

use std::cell::Cell;
use std::time::Duration;

use nsh_core::prelude::*;

use crate::{Playback, Ui};

pub const FORWARD_STEP: i32 = 30;
pub const REWIND_STEP: i32 = 10;
pub const TIMEBAR_STEP: i32 = 300;
/// The archive reopens this long after the last seek key.
const COMMIT_AFTER: Duration = Duration::from_millis(900);
/// A seek stops this far before the archive's end (the programme's end,
/// or now for a programme still airing).
const END_MARGIN: i32 = 10;

thread_local! {
    /// The latest seek key; an older commit timer does nothing.
    static PRESSES: Cell<u64> = const { Cell::new(0) };
}

/// The archived programme playing: its start and length in seconds and how
/// far into it the archive can go now.
fn programme(ui: Ui) -> Option<(i32, i32, i32)> {
    let channel = ui.playing.get_untracked()?;
    let Playback::CatchUp { start, offset } = ui.playback.get_untracked() else {
        return None;
    };
    let schedule = crate::schedule();
    let p = schedule.program_at(channel, start);
    let length = (p.end - p.start) * 60;
    let aired = crate::time::wall_unix() - crate::guide::unix_of(&*schedule, p.start);
    let limit = length.min(i32::try_from(aired).unwrap_or(i32::MAX)) - END_MARGIN;
    Some((offset, length, limit.max(0)))
}

/// Seconds into the archived programme: the seek being chosen, else where
/// the stream is. `None` unless a programme plays from the archive.
pub fn elapsed(ui: Ui) -> Option<(i32, i32)> {
    let Playback::CatchUp { offset, start } = ui.playback.get() else {
        return None;
    };
    let channel = ui.playing.get()?;
    let p = crate::schedule().program_at(channel, start);
    let length = (p.end - p.start) * 60;
    let at = ui.seek_target.get().unwrap_or_else(|| {
        let played = i32::try_from(ui.position_ms.get() / 1000).unwrap_or(i32::MAX);
        offset.saturating_add(played)
    });
    Some((at.clamp(0, length), length))
}

/// Moves the seek target by `delta` seconds; the archive reopens there
/// once the keys pause. False when nothing plays from the archive.
pub fn step(ui: Ui, delta: i32) -> bool {
    let Some((offset, _, limit)) = programme(ui) else {
        return false;
    };
    let from = ui.seek_target.get_untracked().unwrap_or_else(|| {
        let played = i32::try_from(ui.position_ms.get_untracked() / 1000).unwrap_or(i32::MAX);
        offset.saturating_add(played)
    });
    ui.seek_target.set(Some((from + delta).clamp(0, limit)));
    ui.update_panels(|p, now| p.show_info(now));
    let press = PRESSES.with(|p| {
        p.set(p.get() + 1);
        p.get()
    });
    let _ = set_timeout(COMMIT_AFTER, move || {
        if PRESSES.with(Cell::get) == press {
            commit(ui);
        }
    });
    true
}

/// Opens the archive at the chosen target.
fn commit(ui: Ui) {
    let Some(target) = ui.seek_target.get_untracked() else {
        return;
    };
    let (Some(channel), Playback::CatchUp { start, .. }) =
        (ui.playing.get_untracked(), ui.playback.get_untracked())
    else {
        ui.seek_target.set(None);
        return;
    };
    log::info!("iptv seek: catch-up to {target} s into the programme");
    ui.position_ms.set(0);
    ui.play(
        channel,
        Playback::CatchUp {
            start,
            offset: target,
        },
    );
    ui.seek_target.set(None);
    ui.update_panels(|p, now| p.show_info(now));
}

/// `m:ss` or `h:mm:ss`.
pub fn hms(secs: i32) -> String {
    let s = secs.max(0);
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::hms;

    #[test]
    fn times_read_as_minutes_or_hours() {
        assert_eq!(hms(0), "0:00");
        assert_eq!(hms(75), "1:15");
        assert_eq!(hms(3600 + 62), "1:01:02");
        assert_eq!(hms(-5), "0:00");
    }
}
