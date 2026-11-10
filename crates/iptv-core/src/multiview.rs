//! Multiview: two to four channels at once over the whole screen, one of
//! them *active* (its sound plays, the others are muted).
//!
//! Tiles sit in cells: side by side for two cells (1+1), two by two for
//! three or four. While more tiles fit (the device's decoder cap), one more
//! cell offers to add a channel. The remote moves between cells (moving
//! onto a tile makes it active), OK on a tile opens its options (retry when
//! it failed, replace the channel, full screen, remove), OK on the add cell
//! starts picking a channel. Back closes a dialog, else leaves multiview to
//! the active tile in full screen. A tap makes a tile active; a tap on the
//! active one opens its options.
//!
//! Each tile keeps the player surface it was given (0 to [`MAX_SURFACES`]
//! − 1; the first tile starts on surface 0, the full-screen player), so
//! adding, removing or replacing one never disturbs the others. Adding past
//! the account's connection limit asks first; once the viewer insists it
//! does not ask again in this multiview. Channels are the caller's (`C`);
//! nothing here plays anything.

/// The most tiles (and player surfaces) multiview uses.
pub const MAX_SURFACES: usize = 4;

/// How the cells sit on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// One cell over the whole area.
    Single,
    /// Two cells side by side (1+1).
    SideBySide,
    /// Up to four cells, two by two.
    Grid,
}

/// A rectangle as fractions (0–1) of the multiview area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Layout {
    /// The layout for `cells` cells.
    pub fn for_cells(cells: usize) -> Layout {
        match cells {
            0 | 1 => Layout::Single,
            2 => Layout::SideBySide,
            _ => Layout::Grid,
        }
    }

    /// Where cell `index` goes. Side by side, the halves keep a 16:9 area
    /// 16:9 (centred vertically).
    pub fn cell(self, index: usize) -> Rect {
        let (col, row) = ((index % 2) as f32, ((index / 2) % 2) as f32);
        match self {
            Layout::Single => Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            Layout::SideBySide => Rect {
                x: col * 0.5,
                y: 0.25,
                w: 0.5,
                h: 0.5,
            },
            Layout::Grid => Rect {
                x: col * 0.5,
                y: row * 0.5,
                w: 0.5,
                h: 0.5,
            },
        }
    }
}

/// What a cell holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    /// The tile at this index of [`Multiview::tiles`].
    Tile(usize),
    /// Add a channel.
    Add,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile<C> {
    /// The player surface it plays on (0 to [`MAX_SURFACES`] − 1).
    pub surface: usize,
    pub channel: C,
}

/// A tile's options, in the order they are offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileAction {
    /// Only when its stream failed.
    Retry,
    Replace,
    FullScreen,
    /// Only while another tile stays.
    Remove,
}

/// The connection warning's choices, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarningChoice {
    AddAnyway,
    Cancel,
}

pub const WARNING_CHOICES: [WarningChoice; 2] = [WarningChoice::AddAnyway, WarningChoice::Cancel];

/// A dialog over the tiles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    /// A tile's options; `focus` indexes `items`.
    Options {
        tile: usize,
        items: Vec<TileAction>,
        focus: usize,
    },
    /// Adding one more stream exceeds what the account allows (`allowed`
    /// at once); `focus` indexes [`WARNING_CHOICES`].
    Warning { allowed: u32, focus: usize },
}

/// What the channel being picked is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Add,
    /// The channel of the tile at this index.
    Replace(usize),
}

/// The remote's keys as multiview takes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Ok,
    Back,
    /// The options of the active tile.
    Menu,
}

/// What the caller does after a step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome<C> {
    Stay,
    /// Open the channel list; the choice goes to [`Multiview::picked`]
    /// (closing it without one to [`Multiview::cancel_pick`]).
    PickChannel,
    /// Open the stream on this surface again.
    Retry(usize),
    /// Leave multiview; this channel (on `surface`) plays full screen.
    Leave {
        channel: C,
        surface: usize,
    },
}

/// What bounds the number of tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Streams this device decodes at once (1 to [`MAX_SURFACES`]).
    pub decoders: usize,
    /// Streams the account allows at once, when it says.
    pub connections: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Multiview<C> {
    tiles: Vec<Tile<C>>,
    /// The focused cell (an index into [`Multiview::cells`]).
    focus: usize,
    /// The tile whose sound plays.
    active: usize,
    limits: Limits,
    /// The viewer chose to go past the account's limit.
    insisted: bool,
    dialog: Option<Dialog>,
    pick: Option<Pick>,
}

impl<C: Clone + PartialEq> Multiview<C> {
    /// Multiview with `channel` (playing full screen, on surface 0) as its
    /// first tile, active; the focus is on the add cell when there is one.
    pub fn open(channel: C, limits: Limits) -> Self {
        let limits = Limits {
            decoders: limits.decoders.clamp(1, MAX_SURFACES),
            ..limits
        };
        let mut mv = Multiview {
            tiles: vec![Tile {
                surface: 0,
                channel,
            }],
            focus: 0,
            active: 0,
            limits,
            insisted: false,
            dialog: None,
            pick: None,
        };
        if mv.can_add() {
            mv.focus = 1;
        }
        mv
    }

    pub fn tiles(&self) -> &[Tile<C>] {
        &self.tiles
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Another tile fits on this device.
    pub fn can_add(&self) -> bool {
        self.tiles.len() < self.limits.decoders
    }

    /// The tiles in order, then the add cell while another fits.
    pub fn cells(&self) -> Vec<Cell> {
        let mut cells: Vec<Cell> = (0..self.tiles.len()).map(Cell::Tile).collect();
        if self.can_add() {
            cells.push(Cell::Add);
        }
        cells
    }

    pub fn cell_count(&self) -> usize {
        self.tiles.len() + usize::from(self.can_add())
    }

    pub fn layout(&self) -> Layout {
        Layout::for_cells(self.cell_count())
    }

    pub fn focus(&self) -> Cell {
        if self.focus < self.tiles.len() {
            Cell::Tile(self.focus)
        } else {
            Cell::Add
        }
    }

    /// The index of the focused cell.
    pub fn focus_index(&self) -> usize {
        self.focus
    }

    /// The tile whose sound plays.
    pub fn active(&self) -> &Tile<C> {
        &self.tiles[self.active]
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }

    pub fn pick(&self) -> Option<Pick> {
        self.pick
    }

    /// The tile on `surface`, by index.
    pub fn tile_on(&self, surface: usize) -> Option<usize> {
        self.tiles.iter().position(|t| t.surface == surface)
    }

    /// What each surface plays (by surface number).
    pub fn assignments(&self) -> [Option<C>; MAX_SURFACES] {
        let mut out: [Option<C>; MAX_SURFACES] = Default::default();
        for t in &self.tiles {
            out[t.surface] = Some(t.channel.clone());
        }
        out
    }

    /// Adding one more stream goes past the account's limit.
    pub fn over_connections(&self) -> Option<u32> {
        self.limits
            .connections
            .filter(|&allowed| self.tiles.len() as u32 >= allowed)
    }

    /// A key; `failed(surface)` says whether a surface's stream failed
    /// (its options then offer Retry).
    pub fn key(&mut self, key: Key, failed: impl Fn(usize) -> bool) -> Outcome<C> {
        if let Some(dialog) = &mut self.dialog {
            let (focus, len, across) = match dialog {
                Dialog::Options { items, focus, .. } => (focus, items.len(), false),
                Dialog::Warning { focus, .. } => (focus, WARNING_CHOICES.len(), true),
            };
            let step = |focus: &mut usize, by: isize| {
                *focus = (*focus as isize + by).clamp(0, len as isize - 1) as usize;
            };
            match key {
                Key::Up if !across => step(focus, -1),
                Key::Down if !across => step(focus, 1),
                Key::Left if across => step(focus, -1),
                Key::Right if across => step(focus, 1),
                Key::Ok => {
                    let at = *focus;
                    return self.choose(at);
                }
                Key::Back | Key::Menu => self.dialog = None,
                _ => {}
            }
            return Outcome::Stay;
        }
        let n = self.cell_count();
        let (row, col) = (self.focus / 2, self.focus % 2);
        let to = match key {
            Key::Left if col == 1 => Some(self.focus - 1),
            Key::Right if col == 0 && self.focus + 1 < n => Some(self.focus + 1),
            Key::Up if row == 1 => Some(self.focus - 2),
            Key::Down if row == 0 && self.focus + 2 < n => Some(self.focus + 2),
            // Three cells: the one below the right column is the third.
            Key::Down if row == 0 && n == 3 => Some(2),
            Key::Ok => return self.press(self.focus, &failed),
            Key::Menu => {
                self.open_options(self.active, &failed);
                return Outcome::Stay;
            }
            Key::Back => return self.leave(self.active),
            _ => None,
        };
        if let Some(to) = to {
            self.set_focus(to);
        }
        Outcome::Stay
    }

    /// A tap on cell `index`: a tile becomes active, the active one opens
    /// its options; the add cell adds. With a dialog open, any tap (also
    /// beside the cells) only closes it.
    pub fn tap(&mut self, index: usize, failed: impl Fn(usize) -> bool) -> Outcome<C> {
        if self.dialog.take().is_some() || index >= self.cell_count() {
            return Outcome::Stay;
        }
        let again = index == self.focus && index == self.active;
        self.set_focus(index);
        if again || index == self.tiles.len() {
            self.press(index, &failed)
        } else {
            Outcome::Stay
        }
    }

    /// Item `index` of the open dialog (OK on it, or a tap).
    pub fn choose(&mut self, index: usize) -> Outcome<C> {
        match self.dialog.take() {
            None => Outcome::Stay,
            Some(Dialog::Warning { .. }) => match WARNING_CHOICES.get(index) {
                Some(WarningChoice::AddAnyway) => {
                    self.insisted = true;
                    self.pick = Some(Pick::Add);
                    Outcome::PickChannel
                }
                _ => Outcome::Stay,
            },
            Some(Dialog::Options { tile, items, .. }) => {
                let Some(action) = items.get(index) else {
                    return Outcome::Stay;
                };
                match action {
                    TileAction::Retry => Outcome::Retry(self.tiles[tile].surface),
                    TileAction::Replace => {
                        self.pick = Some(Pick::Replace(tile));
                        Outcome::PickChannel
                    }
                    TileAction::FullScreen => self.leave(tile),
                    TileAction::Remove => {
                        self.remove(tile);
                        Outcome::Stay
                    }
                }
            }
        }
    }

    /// The channel list chose `channel` for the pending pick. A channel
    /// already on a tile only focuses that tile.
    pub fn picked(&mut self, channel: C) -> Outcome<C> {
        let Some(pick) = self.pick.take() else {
            return Outcome::Stay;
        };
        if let Some(at) = self.tiles.iter().position(|t| t.channel == channel) {
            self.set_focus(at);
            return Outcome::Stay;
        }
        match pick {
            Pick::Add if self.can_add() => {
                let surface = (0..MAX_SURFACES)
                    .find(|s| self.tile_on(*s).is_none())
                    .unwrap_or(0);
                self.tiles.push(Tile { surface, channel });
                self.set_focus(self.tiles.len() - 1);
            }
            Pick::Add => {}
            Pick::Replace(tile) => {
                if let Some(t) = self.tiles.get_mut(tile) {
                    t.channel = channel;
                    self.set_focus(tile);
                }
            }
        }
        Outcome::Stay
    }

    /// The channel list closed without a choice.
    pub fn cancel_pick(&mut self) {
        self.pick = None;
    }

    /// Keeps the tiles `f` keeps (it may change their channel, e.g. when
    /// the data changed); `false` when none is left.
    pub fn retain(&mut self, mut f: impl FnMut(usize, &mut C) -> bool) -> bool {
        let mut at = 0;
        while at < self.tiles.len() {
            let t = &mut self.tiles[at];
            if f(t.surface, &mut t.channel) {
                at += 1;
            } else {
                self.remove(at);
            }
        }
        !self.tiles.is_empty()
    }

    fn set_focus(&mut self, index: usize) {
        self.focus = index.min(self.cell_count().saturating_sub(1));
        if self.focus < self.tiles.len() {
            self.active = self.focus;
        }
    }

    /// OK on cell `index`.
    fn press(&mut self, index: usize, failed: &impl Fn(usize) -> bool) -> Outcome<C> {
        if index < self.tiles.len() {
            self.open_options(index, failed);
            Outcome::Stay
        } else {
            self.begin_add()
        }
    }

    fn begin_add(&mut self) -> Outcome<C> {
        if !self.can_add() {
            return Outcome::Stay;
        }
        if let Some(allowed) = self.over_connections().filter(|_| !self.insisted) {
            self.dialog = Some(Dialog::Warning { allowed, focus: 1 });
            return Outcome::Stay;
        }
        self.pick = Some(Pick::Add);
        Outcome::PickChannel
    }

    fn open_options(&mut self, tile: usize, failed: &impl Fn(usize) -> bool) {
        let Some(t) = self.tiles.get(tile) else {
            return;
        };
        let mut items = Vec::with_capacity(4);
        if failed(t.surface) {
            items.push(TileAction::Retry);
        }
        items.extend([TileAction::Replace, TileAction::FullScreen]);
        if self.tiles.len() > 1 {
            items.push(TileAction::Remove);
        }
        self.dialog = Some(Dialog::Options {
            tile,
            items,
            focus: 0,
        });
    }

    fn leave(&mut self, tile: usize) -> Outcome<C> {
        let t = &self.tiles[tile];
        Outcome::Leave {
            channel: t.channel.clone(),
            surface: t.surface,
        }
    }

    fn remove(&mut self, tile: usize) {
        if tile >= self.tiles.len() {
            return;
        }
        self.tiles.remove(tile);
        self.pick = match self.pick {
            Some(Pick::Replace(t)) if t == tile => None,
            Some(Pick::Replace(t)) if t > tile => Some(Pick::Replace(t - 1)),
            p => p,
        };
        if self.tiles.is_empty() {
            self.active = 0;
            self.focus = 0;
            return;
        }
        if self.active > tile || self.active >= self.tiles.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.set_focus(self.active);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESKTOP: Limits = Limits {
        decoders: 4,
        connections: None,
    };
    const BOX: Limits = Limits {
        decoders: 2,
        connections: None,
    };

    fn ok(_: usize) -> bool {
        false
    }

    /// Adds `channel` through the add cell.
    fn add(mv: &mut Multiview<u32>, channel: u32) {
        let add = mv.cell_count() - 1;
        assert_eq!(mv.tap(add, ok), Outcome::PickChannel);
        assert_eq!(mv.pick(), Some(Pick::Add));
        assert_eq!(mv.picked(channel), Outcome::Stay);
    }

    fn surfaces(mv: &Multiview<u32>) -> Vec<(usize, u32)> {
        mv.tiles().iter().map(|t| (t.surface, t.channel)).collect()
    }

    #[test]
    fn layouts_follow_the_cells() {
        let mut mv = Multiview::open(10, DESKTOP);
        // One tile and the add cell: side by side.
        assert_eq!(mv.cells(), vec![Cell::Tile(0), Cell::Add]);
        assert_eq!(mv.layout(), Layout::SideBySide);
        assert_eq!(mv.focus(), Cell::Add, "opening offers to add");
        add(&mut mv, 11);
        assert_eq!(mv.layout(), Layout::Grid, "two tiles and the add cell");
        add(&mut mv, 12);
        add(&mut mv, 13);
        assert_eq!(mv.cells().len(), 4, "full: no add cell");
        assert!(!mv.can_add());
        assert_eq!(mv.layout(), Layout::Grid);

        let side = Layout::SideBySide;
        assert_eq!(side.cell(1).x, 0.5);
        assert_eq!((side.cell(0).y, side.cell(0).h), (0.25, 0.5));
        let grid = Layout::Grid;
        assert_eq!((grid.cell(3).x, grid.cell(3).y), (0.5, 0.5));
        assert_eq!(Layout::for_cells(1), Layout::Single);
        assert_eq!(Layout::Single.cell(0).w, 1.0);
    }

    #[test]
    fn the_device_caps_the_tiles() {
        let mut mv = Multiview::open(10, BOX);
        add(&mut mv, 11);
        assert_eq!(mv.cells(), vec![Cell::Tile(0), Cell::Tile(1)]);
        assert_eq!(mv.layout(), Layout::SideBySide, "1+1 at the cap");
        // No add cell to press; a stale pick adds nothing either.
        assert_eq!(mv.tap(2, ok), Outcome::Stay);
        mv.pick = Some(Pick::Add);
        mv.picked(12);
        assert_eq!(mv.tiles().len(), 2);
        // A cap of one: the tile alone, nothing to add.
        let single = Multiview::open(
            1u32,
            Limits {
                decoders: 0,
                connections: None,
            },
        );
        assert_eq!(single.limits().decoders, 1);
        assert_eq!(single.layout(), Layout::Single);
        assert_eq!(single.focus(), Cell::Tile(0));
    }

    #[test]
    fn moving_the_focus_moves_the_sound() {
        let mut mv = Multiview::open(10, DESKTOP);
        add(&mut mv, 11);
        add(&mut mv, 12);
        // Cells: 10 11 / 12 Add. The added tile became active.
        assert_eq!(mv.active().channel, 12);
        assert_eq!(mv.focus(), Cell::Tile(2));
        mv.key(Key::Right, ok);
        assert_eq!(mv.focus(), Cell::Add);
        assert_eq!(mv.active().channel, 12, "the add cell keeps the sound");
        mv.key(Key::Up, ok);
        assert_eq!(mv.focus(), Cell::Tile(1));
        assert_eq!(mv.active().channel, 11);
        mv.key(Key::Up, ok);
        assert_eq!(mv.focus(), Cell::Tile(1), "no wrap");
        mv.key(Key::Left, ok);
        assert_eq!(mv.active().channel, 10);
        mv.key(Key::Down, ok);
        assert_eq!(mv.active().channel, 12);

        // Three cells (two tiles and add): Down from the right column goes
        // to the third.
        let mut three = Multiview::open(1u32, DESKTOP);
        add(&mut three, 2);
        three.key(Key::Up, ok);
        three.key(Key::Right, ok);
        assert_eq!(three.focus(), Cell::Tile(1));
        three.key(Key::Down, ok);
        assert_eq!(three.focus(), Cell::Add);
    }

    #[test]
    fn taps_make_active_then_open_options() {
        let mut mv = Multiview::open(10, DESKTOP);
        add(&mut mv, 11);
        assert_eq!(mv.tap(0, ok), Outcome::Stay);
        assert_eq!(mv.active().channel, 10);
        assert!(mv.dialog().is_none(), "the first tap only makes it active");
        mv.tap(0, ok);
        assert!(matches!(mv.dialog(), Some(Dialog::Options { tile: 0, .. })));
        // A tap elsewhere closes the dialog first.
        assert_eq!(mv.tap(1, ok), Outcome::Stay);
        assert!(mv.dialog().is_none());
        assert_eq!(mv.active().channel, 10);
    }

    #[test]
    fn options_offer_what_applies() {
        let mut mv = Multiview::open(10, DESKTOP);
        mv.key(Key::Left, ok);
        mv.key(Key::Ok, ok);
        let items = |mv: &Multiview<u32>| match mv.dialog() {
            Some(Dialog::Options { items, .. }) => items.clone(),
            _ => vec![],
        };
        assert_eq!(
            items(&mv),
            vec![TileAction::Replace, TileAction::FullScreen],
            "the last tile cannot be removed"
        );
        mv.key(Key::Back, ok);
        assert!(mv.dialog().is_none(), "Back closes the dialog only");
        add(&mut mv, 11);
        mv.key(Key::Menu, |s| s == 1);
        assert_eq!(
            items(&mv),
            vec![
                TileAction::Retry,
                TileAction::Replace,
                TileAction::FullScreen,
                TileAction::Remove
            ]
        );
        assert_eq!(mv.key(Key::Ok, ok), Outcome::Retry(1));
    }

    #[test]
    fn remove_and_replace_keep_the_other_surfaces() {
        let mut mv = Multiview::open(10, DESKTOP);
        add(&mut mv, 11);
        add(&mut mv, 12);
        assert_eq!(surfaces(&mv), vec![(0, 10), (1, 11), (2, 12)]);
        // Remove the first tile (surface 0): the others keep theirs.
        mv.tap(0, ok);
        mv.tap(0, ok);
        let remove = 2;
        assert_eq!(mv.choose(remove), Outcome::Stay);
        assert_eq!(surfaces(&mv), vec![(1, 11), (2, 12)]);
        assert_eq!(mv.active().channel, 11);
        // Surface 0 is free again for the next one.
        add(&mut mv, 13);
        assert_eq!(surfaces(&mv), vec![(1, 11), (2, 12), (0, 13)]);
        let a = mv.assignments();
        assert_eq!(a, [Some(13), Some(11), Some(12), None]);
        // Replace the second tile's channel: same surface.
        mv.tap(1, ok);
        mv.tap(1, ok);
        assert_eq!(mv.choose(0), Outcome::PickChannel);
        assert_eq!(mv.pick(), Some(Pick::Replace(1)));
        mv.picked(20);
        assert_eq!(surfaces(&mv), vec![(1, 11), (2, 20), (0, 13)]);
        assert_eq!(mv.active().channel, 20);
        // Picking a channel already on a tile only focuses it.
        mv.key(Key::Ok, ok);
        mv.choose(0);
        mv.picked(11);
        assert_eq!(surfaces(&mv), vec![(1, 11), (2, 20), (0, 13)]);
        assert_eq!(mv.active().channel, 11);
        // A cancelled pick changes nothing.
        add(&mut mv, 14);
        assert!(!mv.can_add());
        mv.key(Key::Ok, ok);
        mv.choose(0);
        mv.cancel_pick();
        assert_eq!(mv.picked(30), Outcome::Stay);
        assert_eq!(mv.tiles().len(), 4);
    }

    #[test]
    fn back_and_full_screen_leave_with_a_tile() {
        let mut mv = Multiview::open(10, DESKTOP);
        add(&mut mv, 11);
        assert_eq!(
            mv.key(Key::Back, ok),
            Outcome::Leave {
                channel: 11,
                surface: 1
            }
        );
        mv.tap(0, ok);
        mv.tap(0, ok);
        assert_eq!(
            mv.choose(1),
            Outcome::Leave {
                channel: 10,
                surface: 0
            }
        );
    }

    #[test]
    fn the_connection_limit_warns_before_adding() {
        let limits = Limits {
            decoders: 4,
            connections: Some(2),
        };
        let mut mv = Multiview::open(10, limits);
        assert_eq!(mv.over_connections(), None);
        add(&mut mv, 11);
        assert_eq!(mv.over_connections(), Some(2));
        // The third stream: a warning, Cancel focused.
        let add_cell = 2;
        assert_eq!(mv.tap(add_cell, ok), Outcome::Stay);
        assert_eq!(
            mv.dialog(),
            Some(&Dialog::Warning {
                allowed: 2,
                focus: 1
            })
        );
        assert_eq!(mv.key(Key::Ok, ok), Outcome::Stay, "Cancel");
        assert_eq!(mv.tiles().len(), 2);
        assert!(mv.pick().is_none());
        // Again, and insist.
        mv.key(Key::Ok, ok);
        mv.key(Key::Left, ok);
        assert_eq!(mv.key(Key::Ok, ok), Outcome::PickChannel);
        mv.picked(12);
        assert_eq!(mv.tiles().len(), 3);
        // Insisting once is enough for this multiview.
        mv.key(Key::Right, ok);
        assert_eq!(mv.key(Key::Ok, ok), Outcome::PickChannel);
        // Replacing opens no new stream: no warning.
        mv.cancel_pick();
        let mut at_limit = Multiview::open(
            1u32,
            Limits {
                decoders: 4,
                connections: Some(1),
            },
        );
        at_limit.key(Key::Left, ok);
        at_limit.key(Key::Ok, ok);
        assert_eq!(at_limit.choose(0), Outcome::PickChannel);
        assert_eq!(at_limit.pick(), Some(Pick::Replace(0)));
    }

    #[test]
    fn data_changes_keep_or_drop_tiles() {
        let mut mv = Multiview::open(10, DESKTOP);
        add(&mut mv, 11);
        add(&mut mv, 12);
        mv.key(Key::Up, ok);
        mv.key(Key::Right, ok); // tile 1 (11) active
        assert!(mv.retain(|_, ch| {
            *ch += 100;
            *ch != 110
        }));
        assert_eq!(surfaces(&mv), vec![(1, 111), (2, 112)]);
        assert_eq!(mv.active().channel, 111);
        assert!(!mv.retain(|_, _| false));
        assert!(mv.tiles().is_empty());
    }
}
