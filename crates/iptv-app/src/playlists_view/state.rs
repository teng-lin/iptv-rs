//! The playlists screen's state: the stored playlists and an "Add playlist"
//! row, a sheet of actions for one playlist, a delete confirmation, and
//! text entry (rename, guide link). Focus is kept by identity (a
//! playlist's id), never by list position. Pure: the view feeds it remote
//! keys and taps and carries out the [`Effect`]s it returns.

use iptv_core::wizard::{Field, FieldError, validate_field};
use iptv_providers::SecretUrl;
use iptv_store_sqlite::SourceId;

use crate::data::Playlist;

/// A focusable row of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Playlist(SourceId),
    Add,
}

/// What can be done to one playlist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Show,
    Refresh,
    Rename,
    GuideLink,
    /// Remove the guide link: only the provider's guides load.
    ProviderGuide,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    List,
    Actions {
        id: SourceId,
        focus: Act,
    },
    /// Delete `id`? Keeping it is focused first.
    Confirm {
        id: SourceId,
        delete: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    Rename,
    GuideLink,
}

/// Text entry over the actions sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Editing {
    pub id: SourceId,
    pub kind: EditKind,
    pub text: String,
    /// The text started as the current value. A guide link carrying
    /// credentials starts empty instead (it is never shown).
    pub prefilled: bool,
}

/// Why the last entry changed nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    NameEmpty,
    Link(FieldError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Ok,
    Back,
    Menu,
}

/// What the screen asks the app to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Open the add-playlist wizard.
    Add,
    Show(SourceId),
    Refresh(SourceId),
    Rename(SourceId, String),
    SetGuide(SourceId, Option<SecretUrl>),
    Delete(SourceId),
    Close,
}

/// Whether `link` can be shown as it is: no user info, no query (where
/// accounts and tokens go).
pub fn plain_link(link: &str) -> bool {
    let rest = link.split_once("://").map_or(link, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    !authority.contains('@') && !link.contains('?')
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    rows: Vec<Playlist>,
    focus: Item,
    layer: Layer,
    editing: Option<Editing>,
    notice: Option<Notice>,
}

impl Screen {
    /// The list with the playlist on screen (`shown`) focused.
    pub fn new(rows: Vec<Playlist>, shown: Option<SourceId>) -> Self {
        let focus = shown
            .filter(|id| rows.iter().any(|r| r.id == *id))
            .or_else(|| rows.first().map(|r| r.id))
            .map_or(Item::Add, Item::Playlist);
        Self {
            rows,
            focus,
            layer: Layer::List,
            editing: None,
            notice: None,
        }
    }

    pub fn rows(&self) -> &[Playlist] {
        &self.rows
    }

    pub fn row(&self, id: SourceId) -> Option<&Playlist> {
        self.rows.iter().find(|r| r.id == id)
    }

    pub fn focus(&self) -> Item {
        self.focus
    }

    pub fn layer(&self) -> Layer {
        self.layer
    }

    pub fn editing(&self) -> Option<&Editing> {
        self.editing.as_ref()
    }

    pub fn notice(&self) -> Option<Notice> {
        self.notice
    }

    /// The list's rows in order: the playlists, then "Add playlist".
    pub fn items(&self) -> Vec<Item> {
        let mut items: Vec<Item> = self.rows.iter().map(|r| Item::Playlist(r.id)).collect();
        items.push(Item::Add);
        items
    }

    /// The actions offered for `id`.
    pub fn acts(&self, id: SourceId) -> Vec<Act> {
        let linked = self.row(id).is_some_and(|r| r.guide_link.is_some());
        let mut acts = vec![Act::Show, Act::Refresh, Act::Rename, Act::GuideLink];
        if linked {
            acts.push(Act::ProviderGuide);
        }
        acts.push(Act::Delete);
        acts
    }

    /// New rows from the store: focus stays on the same playlist; when it
    /// is gone, on the row that took its place.
    pub fn set_rows(&mut self, rows: Vec<Playlist>) {
        let at = self.items().iter().position(|i| *i == self.focus);
        self.rows = rows;
        let items = self.items();
        if !items.contains(&self.focus) {
            let index = at.unwrap_or(0).min(items.len() - 1);
            // The row after a deleted one moved up; the add row stays last.
            self.focus = items[index];
        }
        let gone = |id| !self.rows.iter().any(|r| r.id == id);
        match self.layer {
            Layer::Actions { id, .. } | Layer::Confirm { id, .. } if gone(id) => {
                self.layer = Layer::List;
                self.editing = None;
            }
            Layer::Actions { id, focus } if !self.acts(id).contains(&focus) => {
                self.layer = Layer::Actions {
                    id,
                    focus: Act::Show,
                };
            }
            _ => {}
        }
    }

    pub fn key(&mut self, key: Key) -> Option<Effect> {
        self.notice = None;
        if self.editing.is_some() {
            return matches!(key, Key::Ok | Key::Back)
                .then(|| self.commit())
                .flatten();
        }
        match self.layer {
            Layer::List => self.list_key(key),
            Layer::Actions { id, focus } => self.actions_key(id, focus, key),
            Layer::Confirm { id, delete } => match key {
                Key::Up | Key::Down => {
                    self.layer = Layer::Confirm {
                        id,
                        delete: !delete,
                    };
                    None
                }
                Key::Ok => self.confirm(id, delete),
                Key::Back | Key::Menu => self.confirm(id, false),
            },
        }
    }

    fn list_key(&mut self, key: Key) -> Option<Effect> {
        let items = self.items();
        let at = items.iter().position(|i| *i == self.focus).unwrap_or(0);
        match (key, self.focus) {
            (Key::Up, _) => self.focus = items[at.saturating_sub(1)],
            (Key::Down, _) => self.focus = items[(at + 1).min(items.len() - 1)],
            (Key::Ok, Item::Add) => return Some(Effect::Add),
            (Key::Ok | Key::Menu, Item::Playlist(id)) => {
                self.layer = Layer::Actions {
                    id,
                    focus: Act::Show,
                };
            }
            (Key::Back, _) => return Some(Effect::Close),
            (Key::Menu, Item::Add) => {}
        }
        None
    }

    fn actions_key(&mut self, id: SourceId, focus: Act, key: Key) -> Option<Effect> {
        let acts = self.acts(id);
        let at = acts.iter().position(|a| *a == focus).unwrap_or(0);
        let to = match key {
            Key::Up => acts[at.saturating_sub(1)],
            Key::Down => acts[(at + 1).min(acts.len() - 1)],
            Key::Ok => return self.act(id, focus),
            Key::Back | Key::Menu => {
                self.layer = Layer::List;
                return None;
            }
        };
        self.layer = Layer::Actions { id, focus: to };
        None
    }

    /// OK (or a tap) on `act` of playlist `id`.
    pub fn act(&mut self, id: SourceId, act: Act) -> Option<Effect> {
        let row = self.row(id)?.clone();
        self.notice = None;
        self.layer = Layer::Actions { id, focus: act };
        let to_list = |s: &mut Self, effect| {
            s.layer = Layer::List;
            s.focus = Item::Playlist(id);
            Some(effect)
        };
        match act {
            Act::Show => to_list(self, Effect::Show(id)),
            Act::Refresh => to_list(self, Effect::Refresh(id)),
            Act::ProviderGuide => to_list(self, Effect::SetGuide(id, None)),
            Act::Rename => {
                self.editing = Some(Editing {
                    id,
                    kind: EditKind::Rename,
                    text: row.name,
                    prefilled: true,
                });
                None
            }
            Act::GuideLink => {
                let shown = row
                    .guide_link
                    .map(|l| l.expose().to_owned())
                    .filter(|l| plain_link(l));
                self.editing = Some(Editing {
                    id,
                    kind: EditKind::GuideLink,
                    prefilled: shown.is_some(),
                    text: shown.unwrap_or_default(),
                });
                None
            }
            Act::Delete => {
                self.layer = Layer::Confirm { id, delete: false };
                None
            }
        }
    }

    /// The confirmation's answer: delete, or back to the actions.
    pub fn confirm(&mut self, id: SourceId, delete: bool) -> Option<Effect> {
        if delete {
            self.layer = Layer::List;
            self.focus = Item::Playlist(id);
            return Some(Effect::Delete(id));
        }
        self.layer = Layer::Actions {
            id,
            focus: Act::Delete,
        };
        None
    }

    /// A tap on a list row: focus it and OK.
    pub fn tap(&mut self, item: Item) -> Option<Effect> {
        self.notice = None;
        self.editing = None;
        self.layer = Layer::List;
        self.focus = item;
        self.list_key(Key::Ok)
    }

    /// A tap beside an open sheet: as Back.
    pub fn dismiss(&mut self) -> Option<Effect> {
        match self.layer {
            Layer::List => None,
            _ => self.key(Key::Back),
        }
    }

    pub fn set_text(&mut self, text: &str) {
        if let Some(e) = &mut self.editing {
            e.text = text.to_owned();
        }
    }

    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }

    /// Ends text entry and decides what the text changes.
    pub fn commit(&mut self) -> Option<Effect> {
        let editing = self.editing.take()?;
        let id = editing.id;
        let row = self.row(id)?.clone();
        let text = editing.text.trim();
        let effect = match editing.kind {
            EditKind::Rename if text.is_empty() => {
                self.notice = Some(Notice::NameEmpty);
                None
            }
            EditKind::Rename if text == row.name => None,
            EditKind::Rename => Some(Effect::Rename(id, text.to_owned())),
            // Emptied a shown link: remove it; an empty start: no change.
            EditKind::GuideLink if text.is_empty() => editing
                .prefilled
                .then_some(Effect::SetGuide(id, None))
                .filter(|_| row.guide_link.is_some()),
            EditKind::GuideLink => match validate_field(Field::EpgUrl, text) {
                Err(e) => {
                    self.notice = Some(Notice::Link(e));
                    None
                }
                Ok(()) if row.guide_link.as_ref().is_some_and(|l| l.expose() == text) => None,
                Ok(()) => Some(Effect::SetGuide(id, Some(SecretUrl::new(text)))),
            },
        };
        if effect.is_some() {
            self.layer = Layer::List;
            self.focus = Item::Playlist(id);
        }
        effect
    }
}

#[cfg(test)]
mod tests {
    use iptv_providers::model::SourceKind;

    use super::*;

    const A: SourceId = SourceId(1);
    const B: SourceId = SourceId(2);
    const C: SourceId = SourceId(3);

    fn row(id: SourceId, name: &str) -> Playlist {
        Playlist {
            id,
            name: name.to_owned(),
            kind: SourceKind::M3u,
            channels: 10,
            updated_at: Some(1_800_000_000),
            failed: None,
            guide_link: None,
        }
    }

    fn rows() -> Vec<Playlist> {
        vec![row(A, "First"), row(B, "Second"), row(C, "Third")]
    }

    fn linked(link: &str) -> Vec<Playlist> {
        let mut rows = rows();
        rows[1].guide_link = Some(SecretUrl::new(link));
        rows
    }

    #[test]
    fn opens_on_the_shown_playlist_and_moves_by_identity() {
        let mut s = Screen::new(rows(), Some(B));
        assert_eq!(s.focus(), Item::Playlist(B));
        s.key(Key::Down);
        s.key(Key::Down);
        assert_eq!(s.focus(), Item::Add);
        s.key(Key::Down);
        assert_eq!(s.focus(), Item::Add, "no wrap");
        assert_eq!(s.key(Key::Ok), Some(Effect::Add));
        assert_eq!(Screen::new(vec![], None).focus(), Item::Add);
        assert_eq!(
            Screen::new(rows(), Some(SourceId(9))).focus(),
            Item::Playlist(A)
        );
        assert_eq!(s.key(Key::Back), Some(Effect::Close));
    }

    #[test]
    fn new_rows_keep_the_focused_playlist() {
        let mut s = Screen::new(rows(), Some(C));
        // B deleted elsewhere: C keeps the focus though its index moved.
        s.set_rows(vec![row(A, "First"), row(C, "Third")]);
        assert_eq!(s.focus(), Item::Playlist(C));
        // The focused playlist is deleted: the row that took its place.
        let mut s = Screen::new(rows(), Some(B));
        s.set_rows(vec![row(A, "First"), row(C, "Third")]);
        assert_eq!(s.focus(), Item::Playlist(C));
        let mut s = Screen::new(rows(), Some(C));
        s.set_rows(vec![row(A, "First"), row(B, "Second")]);
        assert_eq!(s.focus(), Item::Add);
        s.set_rows(vec![]);
        assert_eq!(s.focus(), Item::Add);
    }

    #[test]
    fn ok_opens_the_actions_and_show_closes_them() {
        let mut s = Screen::new(rows(), Some(A));
        s.key(Key::Down);
        assert_eq!(s.key(Key::Ok), None);
        assert_eq!(
            s.layer(),
            Layer::Actions {
                id: B,
                focus: Act::Show
            }
        );
        assert_eq!(
            s.acts(B),
            [
                Act::Show,
                Act::Refresh,
                Act::Rename,
                Act::GuideLink,
                Act::Delete
            ]
        );
        assert_eq!(s.key(Key::Ok), Some(Effect::Show(B)));
        assert_eq!(s.layer(), Layer::List);
        // Menu opens them too; Back closes them, keeping the focus.
        s.key(Key::Menu);
        s.key(Key::Down);
        assert_eq!(s.key(Key::Ok), Some(Effect::Refresh(B)));
        s.key(Key::Ok);
        s.key(Key::Back);
        assert_eq!((s.layer(), s.focus()), (Layer::List, Item::Playlist(B)));
    }

    #[test]
    fn delete_asks_first_and_keeping_is_the_default() {
        let mut s = Screen::new(rows(), Some(A));
        s.key(Key::Ok);
        for _ in 0..10 {
            s.key(Key::Down);
        }
        assert_eq!(
            s.layer(),
            Layer::Actions {
                id: A,
                focus: Act::Delete
            }
        );
        s.key(Key::Ok);
        assert_eq!(
            s.layer(),
            Layer::Confirm {
                id: A,
                delete: false
            }
        );
        // OK on the default keeps it; Back too.
        assert_eq!(s.key(Key::Ok), None);
        s.key(Key::Ok);
        assert_eq!(s.key(Key::Back), None);
        assert!(matches!(s.layer(), Layer::Actions { .. }));
        s.key(Key::Ok);
        s.key(Key::Down);
        assert_eq!(s.key(Key::Ok), Some(Effect::Delete(A)));
        assert_eq!(s.layer(), Layer::List);
    }

    #[test]
    fn rename_ignores_an_empty_or_unchanged_name() {
        let mut s = Screen::new(rows(), Some(A));
        s.act(A, Act::Rename);
        assert_eq!(s.editing().map(|e| e.text.as_str()), Some("First"));
        s.set_text("  ");
        assert_eq!(s.key(Key::Ok), None);
        assert_eq!(s.notice(), Some(Notice::NameEmpty));
        assert!(matches!(s.layer(), Layer::Actions { .. }));
        s.act(A, Act::Rename);
        assert_eq!(s.commit(), None, "unchanged");
        s.act(A, Act::Rename);
        s.set_text(" Evening ");
        assert_eq!(s.key(Key::Back), Some(Effect::Rename(A, "Evening".into())));
        assert_eq!(s.editing(), None);
        // Cancel changes nothing.
        s.act(A, Act::Rename);
        s.set_text("Other");
        s.cancel_edit();
        assert_eq!(s.editing(), None);
    }

    #[test]
    fn a_guide_link_is_validated_and_can_be_removed() {
        let mut s = Screen::new(rows(), Some(A));
        s.act(A, Act::GuideLink);
        assert_eq!(s.editing().map(|e| e.prefilled), Some(false));
        assert_eq!(s.commit(), None, "nothing typed, nothing set");
        s.act(A, Act::GuideLink);
        s.set_text("guide.example.test/epg.xml");
        assert_eq!(s.commit(), None);
        assert_eq!(s.notice(), Some(Notice::Link(FieldError::MissingScheme)));
        s.act(A, Act::GuideLink);
        s.set_text("http://guide.example.test/epg.xml");
        assert_eq!(
            s.commit(),
            Some(Effect::SetGuide(
                A,
                Some(SecretUrl::new("http://guide.example.test/epg.xml"))
            ))
        );
        // A plain link is shown for editing; emptying it removes it.
        let mut s = Screen::new(linked("http://guide.example.test/epg.xml"), Some(B));
        assert!(s.acts(B).contains(&Act::ProviderGuide));
        s.act(B, Act::GuideLink);
        let e = s.editing().unwrap();
        assert_eq!(
            (e.text.as_str(), e.prefilled),
            ("http://guide.example.test/epg.xml", true)
        );
        assert_eq!(s.commit(), None, "unchanged");
        s.act(B, Act::GuideLink);
        s.set_text("");
        assert_eq!(s.commit(), Some(Effect::SetGuide(B, None)));
        assert_eq!(
            s.act(B, Act::ProviderGuide),
            Some(Effect::SetGuide(B, None))
        );
    }

    #[test]
    fn a_link_with_credentials_is_never_put_in_the_field() {
        let mut s = Screen::new(
            linked("http://guide.example.test/xmltv.php?username=u&password=p"),
            Some(B),
        );
        s.act(B, Act::GuideLink);
        let e = s.editing().unwrap();
        assert_eq!((e.text.as_str(), e.prefilled), ("", false));
        // Left empty, the link stays.
        assert_eq!(s.commit(), None);
        assert!(plain_link("https://guide.example.test/a.xml.gz"));
        assert!(!plain_link("http://user:pass@guide.example.test/a.xml"));
        assert!(!plain_link("http://guide.example.test/a?token=x"));
    }

    #[test]
    fn taps_and_dismissals() {
        let mut s = Screen::new(rows(), Some(A));
        assert_eq!(s.tap(Item::Add), Some(Effect::Add));
        assert_eq!(s.tap(Item::Playlist(C)), None);
        assert_eq!(
            s.layer(),
            Layer::Actions {
                id: C,
                focus: Act::Show
            }
        );
        assert_eq!(s.dismiss(), None);
        assert_eq!((s.layer(), s.focus()), (Layer::List, Item::Playlist(C)));
        assert_eq!(s.dismiss(), None, "nothing to dismiss");
        // The playlist of an open sheet was deleted elsewhere.
        s.act(C, Act::Delete);
        s.set_rows(vec![row(A, "First")]);
        assert_eq!((s.layer(), s.focus()), (Layer::List, Item::Add));
    }
}
