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
use std::collections::{BTreeSet, VecDeque};
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
    combined_sources: Vec<SourceId>,
    favorites: Favorites,
    recent: Recent,
    /// The catalog on screen, by key.
    index: KeyIndex,
    /// The source has movies or shows.
    has_vod: bool,
}

/// A store write, in the order made.
enum Write {
    Barrier(futures_channel::oneshot::Sender<bool>),
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
        s.combined_sources.clear();
        s.favorites = favorites;
        s.recent = recent;
        s.has_vod = has_vod;
    });
}

/// Load enabled-source favourites into qualified UI identities. Keep already
/// loaded in-memory keys so a guide refresh cannot overtake queued edits.
pub fn set_combined() {
    use iptv_guide::combined::{qualified_key, split_key};
    let Ok(store) = crate::data::open_store() else {
        return;
    };
    let Ok(sources) = store.sources() else { return };
    let ids: Vec<_> = sources
        .into_iter()
        .filter(|s| store.source_policy(s.id).is_ok_and(|p| p.enabled))
        .map(|s| s.id)
        .collect();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let previous = state.combined_sources.clone();
        let mut keys: Vec<_> = state
            .favorites
            .keys()
            .iter()
            .filter(|k| {
                split_key(k).is_some_and(|(s, _)| {
                    previous.contains(&SourceId(s)) && ids.contains(&SourceId(s))
                })
            })
            .cloned()
            .collect();
        for id in ids.iter().filter(|id| !previous.contains(id)) {
            if let Ok(rows) = store.load_channels(
                *id,
                &ChannelQuery {
                    favourites_only: true,
                    include_hidden: true,
                    ..Default::default()
                },
            ) {
                keys.extend(
                    rows.iter()
                        .map(|r| qualified_key(id.0, r.channel.id.as_str())),
                );
            }
        }
        state.source = None;
        state.combined_sources = ids.clone();
        state.favorites = Favorites::from_keys(keys);
        if previous != ids
            && let Ok(rows) = store.recent(None, Some(ContentKind::Live), RECENT_LIMIT as u32)
        {
            state.recent = Recent::from_keys(
                rows.into_iter()
                    .filter(|r| ids.contains(&r.source_id))
                    .map(|r| qualified_key(r.source_id.0, r.item_id.as_str())),
            );
        }
        state.has_vod = false;
    });
}
fn write_owner(state: &State, key: &str) -> Option<(SourceId, String)> {
    if let Some(source) = state.source {
        Some((source, key.to_owned()))
    } else {
        let (s, k) = iptv_guide::combined::split_key(key)?;
        state
            .combined_sources
            .contains(&SourceId(s))
            .then(|| (SourceId(s), k.to_owned()))
    }
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
        if s.favorites.set(key, on) {
            write_owner(&s, key)
        } else {
            None
        }
    });
    if let Some((source, key)) = source {
        send(Write::Favourite { source, key, on });
    }
}

/// A live channel started playing (by key).
pub fn played(key: &str) {
    let source = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.recent.played(key);
        write_owner(&s, key)
    });
    if let Some((source, key)) = source {
        send(Write::Watched {
            source,
            key,
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
pub(crate) async fn flush() -> bool {
    let (tx, rx) = futures_channel::oneshot::channel();
    send(Write::Barrier(tx));
    rx.await.unwrap_or(false)
}

fn write_all(rx: Receiver<Write>) {
    let mut store = None;
    write_all_with(rx, |write| {
        if store.is_none() {
            store = crate::data::open_store()
                .map_err(|e| log::warn!("iptv favourites: store unavailable ({e})"))
                .ok();
        }
        let Some(store) = store.as_mut() else {
            return false;
        };
        let result = match write {
            Write::Barrier(_) => unreachable!(),
            Write::MoveFavourite { source, key, index } => store
                .move_favourite(*source, &ContentId::from_stored(key.clone()), *index)
                .map(|_| ()),
            Write::Favourite { source, key, on } => {
                store.set_favourite(*source, &ContentId::from_stored(key.clone()), *on)
            }
            Write::Watched { source, key, at } => store.record_watch(
                *source,
                &ContentId::from_stored(key.clone()),
                ContentKind::Live,
                *at,
                None,
            ),
        };
        // Deleting a playlist also deletes its customization/history. A queued
        // change whose owner was explicitly removed must not block other sources.
        if matches!(
            &result,
            Err(iptv_store_sqlite::StoreError::SourceNotFound(_))
        ) {
            return true;
        }
        if let Err(e) = result {
            log::warn!("iptv favourites: could not save ({e})");
            return false;
        }
        true
    });
}

/// A failed operation stays at the head of the queue. Neither later writes nor
/// barriers may skip it; every barrier retries all pending changes in order.
fn write_all_with(rx: Receiver<Write>, mut persist: impl FnMut(&Write) -> bool) {
    let mut pending = VecDeque::new();
    for write in rx {
        let barrier = match write {
            Write::Barrier(done) => Some(done),
            write => {
                pending.push_back(write);
                None
            }
        };
        while let Some(write) = pending.front() {
            if !persist(write) {
                break;
            }
            pending.pop_front();
        }
        if let Some(done) = barrier {
            let _ = done.send(pending.is_empty());
        }
    }
    if !pending.is_empty() {
        log::warn!(
            "iptv favourites: writer stopped with {} unsaved changes",
            pending.len()
        );
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

#[cfg(test)]
mod writer_tests {
    use super::*;

    #[test]
    fn failed_changes_retry_in_order_before_a_barrier_can_succeed() {
        let (tx, rx) = mpsc::channel();
        tx.send(Write::Favourite {
            source: SourceId(1),
            key: "generated-a".into(),
            on: true,
        })
        .unwrap();
        tx.send(Write::MoveFavourite {
            source: SourceId(1),
            key: "generated-a".into(),
            index: 0,
        })
        .unwrap();
        let (first, mut first_result) = futures_channel::oneshot::channel();
        tx.send(Write::Barrier(first)).unwrap();
        let (retry, mut retry_result) = futures_channel::oneshot::channel();
        tx.send(Write::Barrier(retry)).unwrap();
        tx.send(Write::Watched {
            source: SourceId(2),
            key: "generated-b".into(),
            at: 123,
        })
        .unwrap();
        let (last, mut last_result) = futures_channel::oneshot::channel();
        tx.send(Write::Barrier(last)).unwrap();
        drop(tx);
        let mut attempts = 0;
        let mut persisted = Vec::new();
        write_all_with(rx, |write| {
            attempts += 1;
            if attempts <= 3 {
                return false;
            }
            persisted.push(match write {
                Write::Favourite { on, .. } => {
                    assert!(*on);
                    "favorite"
                }
                Write::MoveFavourite { index, .. } => {
                    assert_eq!(*index, 0);
                    "order"
                }
                Write::Watched { source, at, .. } => {
                    assert_eq!((*source, *at), (SourceId(2), 123));
                    "history"
                }
                _ => unreachable!(),
            });
            true
        });
        assert_eq!(first_result.try_recv().unwrap(), Some(false));
        assert_eq!(retry_result.try_recv().unwrap(), Some(true));
        assert_eq!(last_result.try_recv().unwrap(), Some(true));
        assert_eq!(persisted, ["favorite", "order", "history"]);
    }

    #[test]
    fn repeated_flushes_cannot_clear_a_persistent_failure() {
        let (tx, rx) = mpsc::channel();
        tx.send(Write::Favourite {
            source: SourceId(1),
            key: "generated".into(),
            on: false,
        })
        .unwrap();
        let mut answers = Vec::new();
        for _ in 0..3 {
            let (done, result) = futures_channel::oneshot::channel();
            tx.send(Write::Barrier(done)).unwrap();
            answers.push(result);
        }
        drop(tx);
        let mut attempts = 0;
        write_all_with(rx, |_| {
            attempts += 1;
            false
        });
        assert_eq!(attempts, 4);
        for mut result in answers {
            assert_eq!(result.try_recv().unwrap(), Some(false));
        }
    }
}
