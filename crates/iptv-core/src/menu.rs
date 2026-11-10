//! Context menus (Long OK / Menu): a list of items with enabled state, focus
//! that skips disabled items, and the content the menu was opened on so
//! focus returns there when it closes.
//!
//! [`program_menu`] builds the guide's programme menu from the programme's
//! airing state and the channel's catch-up window: actions that cannot work
//! are absent or disabled, never offered as if they would (AC04). The item
//! order is iptv-rs's own and is revisited with the reference UX's runtime
//! findings (`~/src/tivimate/docs/08-*`).

use crate::guide::{Airing, Program};

/// Menu actions. Labels are the shell's (i18n), not stored here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuItem {
    /// Play the channel live.
    Watch,
    /// Play a live programme from its start (catch-up).
    WatchFromStart,
    /// Play an ended programme from the archive (catch-up).
    PlayRecorded,
    Record,
    Reminder,
    Description,
    ToggleFavorite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub item: MenuItem,
    pub enabled: bool,
}

/// What the menu was opened on: focus returns here on close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Origin {
    pub channel: usize,
    pub program_start: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub entries: Vec<Entry>,
    pub focus: usize,
    pub origin: Origin,
}

impl Menu {
    /// A menu focused on its first enabled entry.
    pub fn new(entries: Vec<Entry>, origin: Origin) -> Menu {
        let focus = entries.iter().position(|e| e.enabled).unwrap_or(0);
        Menu {
            entries,
            focus,
            origin,
        }
    }

    pub fn focused(&self) -> Option<MenuItem> {
        self.entries
            .get(self.focus)
            .filter(|e| e.enabled)
            .map(|e| e.item)
    }

    /// Moves to the next enabled entry in `step`'s direction; stays at the
    /// ends (no wrap).
    pub fn move_focus(&mut self, step: isize) {
        let mut at = self.focus as isize;
        loop {
            at += step.signum();
            match self.entries.get(at as usize) {
                _ if at < 0 => return,
                None => return,
                Some(e) if e.enabled => {
                    self.focus = at as usize;
                    return;
                }
                Some(_) => {}
            }
        }
    }

    /// A tap: focus the entry and return it if it is enabled.
    pub fn tap(&mut self, index: usize) -> Option<MenuItem> {
        let entry = self.entries.get(index).filter(|e| e.enabled)?;
        self.focus = index;
        Some(entry.item)
    }
}

/// The guide's programme menu. `catchup_days`: the channel's archive window
/// (0 = no catch-up); `now`: minutes on the guide clock.
pub fn program_menu(
    program: Program,
    catchup_days: u8,
    now: i32,
    favorite_known: bool,
) -> Vec<Entry> {
    let in_archive = catchup_days > 0 && program.start >= now - i32::from(catchup_days) * 24 * 60;
    let on = |item| Entry {
        item,
        enabled: true,
    };
    let off = |item| Entry {
        item,
        enabled: false,
    };
    let mut entries = match program.airing() {
        Airing::Live => {
            let mut e = vec![on(MenuItem::Watch)];
            if catchup_days > 0 {
                e.push(on(MenuItem::WatchFromStart));
            }
            e.push(off(MenuItem::Record));
            e
        }
        Airing::Ended if in_archive => vec![on(MenuItem::PlayRecorded)],
        // Past without an archive: nothing to play; only details remain.
        Airing::Ended => vec![],
        Airing::Upcoming => vec![off(MenuItem::Reminder), off(MenuItem::Record)],
    };
    entries.push(on(MenuItem::Description));
    entries.push(Entry {
        item: MenuItem::ToggleFavorite,
        enabled: favorite_known,
    });
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i32 = 24 * 60;

    fn items(entries: &[Entry]) -> Vec<MenuItem> {
        entries.iter().map(|e| e.item).collect()
    }

    #[test]
    fn live_programmes_offer_watch_and_from_start_only_with_catchup() {
        let live = Program {
            start: -10,
            end: 50,
        };
        let with = program_menu(live, 3, 0, true);
        assert_eq!(
            &items(&with)[..2],
            &[MenuItem::Watch, MenuItem::WatchFromStart]
        );
        let without = program_menu(live, 0, 0, true);
        assert!(!items(&without).contains(&MenuItem::WatchFromStart));
    }

    #[test]
    fn ended_programmes_play_only_inside_the_archive_window() {
        let ended = Program {
            start: -120,
            end: -60,
        };
        assert_eq!(
            items(&program_menu(ended, 3, 0, true))[0],
            MenuItem::PlayRecorded
        );
        let old = Program {
            start: -4 * DAY,
            end: -4 * DAY + 30,
        };
        assert!(!items(&program_menu(old, 3, 0, true)).contains(&MenuItem::PlayRecorded));
        assert!(!items(&program_menu(ended, 0, 0, true)).contains(&MenuItem::PlayRecorded));
    }

    #[test]
    fn future_programmes_never_offer_playback() {
        let future = Program { start: 60, end: 90 };
        let m = items(&program_menu(future, 3, 0, true));
        assert!(!m.contains(&MenuItem::Watch) && !m.contains(&MenuItem::PlayRecorded));
        assert!(m.contains(&MenuItem::Description));
    }

    #[test]
    fn focus_skips_disabled_entries_and_stops_at_the_ends() {
        let origin = Origin {
            channel: 4,
            program_start: 60,
        };
        let future = Program { start: 60, end: 90 };
        let mut m = Menu::new(program_menu(future, 0, 0, true), origin);
        assert_eq!(
            m.focused(),
            Some(MenuItem::Description),
            "reminder/record disabled"
        );
        m.move_focus(-1);
        assert_eq!(
            m.focused(),
            Some(MenuItem::Description),
            "nothing enabled above"
        );
        m.move_focus(1);
        assert_eq!(m.focused(), Some(MenuItem::ToggleFavorite));
        m.move_focus(1);
        assert_eq!(m.focused(), Some(MenuItem::ToggleFavorite));
        assert_eq!(m.tap(0), None, "a disabled entry does nothing");
        assert_eq!(m.tap(2), Some(MenuItem::Description));
        assert_eq!(m.origin, origin);
    }
}
