//! The favourites editor for one group: a channel list with a star each.
//! OK toggles the focused channel, Left clears the whole group, Right sets
//! the whole group (the reference UX's observed editor, tivimate docs/09);
//! changes apply immediately. Channels are indices into the schedule; the
//! store keys favourites by stable channel id.

use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FavoritesEditor {
    pub group: usize,
    /// The group's first channel and how many it has.
    pub first: usize,
    pub len: usize,
    /// Focused row, relative to `first`.
    pub focus: usize,
}

impl FavoritesEditor {
    pub fn new(group: usize, first: usize, len: usize) -> Self {
        FavoritesEditor {
            group,
            first,
            len,
            focus: 0,
        }
    }

    pub fn channels(&self) -> std::ops::Range<usize> {
        self.first..self.first + self.len
    }

    /// The focused channel, if the group has any.
    pub fn focused(&self) -> Option<usize> {
        (self.len > 0).then(|| self.first + self.focus)
    }

    pub fn move_focus(&mut self, step: isize) {
        let last = self.len.saturating_sub(1) as isize;
        self.focus = (self.focus as isize + step).clamp(0, last) as usize;
    }

    /// OK: star or unstar the focused channel.
    pub fn toggle(&self, favorites: &mut BTreeSet<usize>) {
        if let Some(ch) = self.focused() {
            if !favorites.remove(&ch) {
                favorites.insert(ch);
            }
        }
    }

    /// Left: no channel of this group stays a favourite.
    pub fn clear_group(&self, favorites: &mut BTreeSet<usize>) {
        favorites.retain(|ch| !self.channels().contains(ch));
    }

    /// Right: every channel of this group becomes a favourite.
    pub fn set_group(&self, favorites: &mut BTreeSet<usize>) {
        favorites.extend(self.channels());
    }

    /// How many of the group's channels are favourites.
    pub fn starred(&self, favorites: &BTreeSet<usize>) -> usize {
        favorites.range(self.channels()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_toggles_the_focused_channel() {
        let mut e = FavoritesEditor::new(1, 10, 5);
        let mut fav = BTreeSet::new();
        e.move_focus(2);
        e.toggle(&mut fav);
        assert_eq!(fav, BTreeSet::from([12]));
        e.toggle(&mut fav);
        assert!(fav.is_empty());
    }

    #[test]
    fn left_and_right_clear_and_set_only_this_group() {
        let e = FavoritesEditor::new(1, 10, 5);
        let mut fav = BTreeSet::from([3, 11, 40]);
        e.set_group(&mut fav);
        assert_eq!(e.starred(&fav), 5);
        assert!(
            fav.contains(&3) && fav.contains(&40),
            "other groups untouched"
        );
        e.clear_group(&mut fav);
        assert_eq!(fav, BTreeSet::from([3, 40]));
        e.clear_group(&mut fav);
        assert_eq!(
            fav,
            BTreeSet::from([3, 40]),
            "clearing an empty group is harmless"
        );
    }

    #[test]
    fn focus_clamps_and_empty_groups_do_nothing() {
        let mut e = FavoritesEditor::new(0, 0, 3);
        e.move_focus(-4);
        assert_eq!(e.focused(), Some(0));
        e.move_focus(10);
        assert_eq!(e.focused(), Some(2));
        let empty = FavoritesEditor::new(0, 7, 0);
        let mut fav = BTreeSet::new();
        empty.toggle(&mut fav);
        empty.set_group(&mut fav);
        assert!(fav.is_empty());
        assert_eq!(empty.focused(), None);
    }
}
