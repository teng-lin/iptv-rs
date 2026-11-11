//! Favourite and recently watched channels, kept by channel key per source
//! in the store (its `favourites` and `watch_history` tables), so they
//! survive refreshes and restarts. `Ui::favorites` holds their catalog
//! channels on screen (stars, search ranking); the Favourites and Recently
//! watched lists are built from here (`lists.rs`).
//!
//! The in-memory lists are the truth while a source is shown; writes go to
//! one writer thread in order, so a toggle never waits for the store. They
//! are read when a source is shown. The synthetic guide keeps them in
//! memory only.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver, Sender};

use iptv_core::favorites::{Favorites, History, KeyIndex, RECENT_LIMIT, Recent, diff};
use iptv_core::guide::Schedule;
use iptv_providers::ContentId;
use iptv_providers::model::ContentKind;
use iptv_store_sqlite::{ChannelOrder, ChannelQuery, SourceId};

use crate::Ui;

#[derive(Default)]
struct State {
    source: Option<SourceId>,
    favorites: Favorites,
    recent: Recent,
    /// The catalog on screen, by key.
    index: KeyIndex,
    /// The source has movies or shows.
    has_vod: bool,
}

/// A store write, in the order made.
enum Write {
    MoveFavourite {
        source: SourceId,
        key: String,
        index: usize,
    },
    Favourite {
        source: SourceId,
        key: String,
        on: bool,
    },
    Watched {
        source: SourceId,
        key: String,
        at: i64,
    },
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
    static WRITER: RefCell<Option<Sender<Write>>> = const { RefCell::new(None) };
}

/// The source whose catalog is about to be shown (`data.rs`, before
/// `set_schedule`): its saved favourites and recent channels. The same
/// source keeps its in-memory lists (writes may still be on their way).
pub fn set_source(source: Option<SourceId>) {
    let Some(id) = source else {
        return; // the synthetic guide: memory only
    };
    let same = STATE.with(|s| s.borrow().source == source);
    let store = match crate::data::open_store() {
        Ok(store) => store,
        Err(e) => {
            log::warn!("iptv favourites: store unavailable ({e})");
            return;
        }
    };
    let has_vod = store
        .import_status(id)
        .is_ok_and(|s| s.counts.vod + s.counts.series > 0);
    if same {
        STATE.with(|s| s.borrow_mut().has_vod = has_vod);
        return;
    }
    let query = ChannelQuery {
        favourites_only: true,
        include_hidden: true,
        order: ChannelOrder::Effective,
        ..ChannelQuery::default()
    };
    let favorites = match store.load_channels(id, &query) {
        Ok(rows) => {
            Favorites::from_keys(rows.into_iter().map(|r| r.channel.id.as_str().to_owned()))
        }
        Err(e) => {
            log::warn!("iptv favourites: could not read them ({e})");
            Favorites::default()
        }
    };
    let recent = match store.recent(Some(id), Some(ContentKind::Live), RECENT_LIMIT as u32) {
        Ok(entries) => {
            Recent::from_keys(entries.into_iter().map(|e| e.item_id.as_str().to_owned()))
        }
        Err(e) => {
            log::warn!("iptv favourites: could not read the history ({e})");
            Recent::default()
        }
    };
    log::info!(
        "iptv favourites: source {} has {} favourite(s), {} recent",
        id.0,
        favorites.keys().len(),
        recent.keys().len()
    );
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.source = source;
        s.favorites = favorites;
        s.recent = recent;
        s.has_vod = has_vod;
    });
}

/// A new catalog is on screen (`set_schedule`): index its keys and show
/// its favourites.
pub fn catalog_changed(ui: Ui, catalog: &dyn Schedule) {
    let index = KeyIndex::build(catalog);
    STATE.with(|s| s.borrow_mut().index = index);
    publish(ui);
}

fn publish(ui: Ui) {
    let (set, has_vod) = STATE.with(|s| {
        let s = s.borrow();
        (s.favorites.set_of(&s.index), s.has_vod)
    });
    ui.favorites.set(set);
    ui.has_vod.set(has_vod);
}

/// The favourites on screen as catalog channels, in their order.
pub fn favorite_channels() -> Vec<usize> {
    STATE.with(|s| {
        let s = s.borrow();
        s.favorites.channels(&s.index)
    })
}

/// The recently watched channels on screen, newest first.
pub fn recent_channels() -> Vec<usize> {
    STATE.with(|s| {
        let s = s.borrow();
        s.recent.channels(&s.index)
    })
}

/// The player's History: the recently watched channels (newest first) and
/// the row of `playing` among them.
pub fn history(playing: Option<usize>) -> History {
    STATE.with(|s| {
        let s = s.borrow();
        s.recent.history(&s.index, playing)
    })
}

/// The catalog channel with `key`.
pub fn channel_of(key: &str) -> Option<usize> {
    STATE.with(|s| s.borrow().index.get(key))
}

/// Changes favourites through their catalog channels (the editor works on
/// sets of them): what `f` adds or removes is saved by key.
pub fn edit(ui: Ui, f: impl FnOnce(&mut BTreeSet<usize>)) {
    let before = ui.favorites.get_untracked();
    let mut after = before.clone();
    f(&mut after);
    let (added, removed) = diff(&before, &after);
    if added.is_empty() && removed.is_empty() {
        return;
    }
    let catalog = crate::schedule();
    for (channels, on) in [(added, true), (removed, false)] {
        for key in channels.into_iter().filter_map(|c| catalog.channel_key(c)) {
            set_key(&key, on);
        }
    }
    publish(ui);
    crate::lists::favorites_changed(ui);
}

/// Adds `channel` (catalog) to the favourites or removes it.
pub fn toggle(ui: Ui, channel: usize) {
    edit(ui, |set| {
        if !set.remove(&channel) {
            set.insert(channel);
        }
    });
}

fn set_key(key: &str, on: bool) {
    let source = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.favorites.set(key, on).then_some(s.source).flatten()
    });
    if let Some(source) = source {
        send(Write::Favourite {
            source,
            key: key.to_owned(),
            on,
        });
    }
}

/// A live channel started playing (by key).
pub fn played(key: &str) {
    let source = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.recent.played(key);
        s.source
    });
    if let Some(source) = source {
        send(Write::Watched {
            source,
            key: key.to_owned(),
            at: crate::time::wall_unix(),
        });
    }
}

fn send(write: Write) {
    WRITER.with(|w| {
        let mut w = w.borrow_mut();
        let tx = w.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || write_all(rx));
            tx
        });
        if tx.send(write).is_err() {
            log::warn!("iptv favourites: the writer stopped");
        }
    });
}

/// The writer thread: one store connection, writes in order.
fn write_all(rx: Receiver<Write>) {
    let mut store = None;
    for write in rx {
        if store.is_none() {
            store = crate::data::open_store()
                .map_err(|e| log::warn!("iptv favourites: store unavailable ({e})"))
                .ok();
        }
        let Some(store) = store.as_mut() else {
            continue;
        };
        let result = match write {
            Write::MoveFavourite { source, key, index } => store
                .move_favourite(source, &ContentId::from_stored(key), index)
                .map(|_| ()),
            Write::Favourite { source, key, on } => {
                store.set_favourite(source, &ContentId::from_stored(key), on)
            }
            Write::Watched { source, key, at } => store.record_watch(
                source,
                &ContentId::from_stored(key),
                ContentKind::Live,
                at,
                None,
            ),
        };
        if let Err(e) = result {
            log::warn!("iptv favourites: could not save ({e})");
        }
    }
}

/// Ordered keys of the displayed source. Reorders use the same writer queue
/// as membership changes, so rapid add/move/remove cannot race persistence.
pub fn ordered_keys() -> Vec<String> {
    STATE.with(|s| s.borrow().favorites.keys().to_vec())
}
pub fn reorder(ui: Ui, key: &str, index: usize) {
    let source = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.favorites
            .move_key(key, index)
            .then_some(s.source)
            .flatten()
    });
    if let Some(source) = source {
        send(Write::MoveFavourite {
            source,
            key: key.into(),
            index,
        });
    }
    publish(ui);
    crate::lists::favorites_changed(ui);
}
