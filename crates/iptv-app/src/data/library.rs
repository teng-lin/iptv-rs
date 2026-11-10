//! The stored playlists (sources) as the playlists screen sees them, and
//! what it does to them: which one is shown (remembered in the store, so
//! the next start opens it), refresh, rename, the guide link, delete.
//!
//! One refresh per playlist runs at a time; each has a ticket, so a
//! cancelled one never overwrites what a newer one reports.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

use iptv_core::wizard::{ImportCounts, ImportFailure, ImportProgress};
use iptv_import::CancelToken;
use iptv_providers::SecretUrl;
use iptv_providers::model::SourceKind;
use iptv_store_sqlite::{ImportErrorKind, SourceId, StoreError, StoredSource};
use nsh_core::prelude::*;

use super::{Job, Purpose, Update, open_store, start_import_with, stored};
use crate::Ui;

/// What the data layer tells the screens about the stored playlists.
#[derive(Clone, Copy)]
pub struct Library {
    /// The stored playlist on screen; `None` before one is (first run).
    pub shown: Signal<Option<SourceId>>,
    /// Refreshes of stored playlists since the app started.
    pub activity: Signal<BTreeMap<SourceId, Activity>>,
    /// Bumped when what is stored about the playlists changed.
    pub version: Signal<u64>,
}

impl Library {
    pub fn new() -> Self {
        Self {
            shown: signal(None),
            activity: signal(BTreeMap::new()),
            version: signal(0),
        }
    }
}

/// A stored playlist's refresh this session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Activity {
    Running(Option<ImportProgress>),
    /// Finished; on failure the stored channels stay.
    Done(Result<ImportCounts, ImportFailure>),
}

/// A stored playlist, as listed. Carries no secrets but the guide link,
/// which redacts itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playlist {
    pub id: SourceId,
    pub name: String,
    pub kind: SourceKind,
    /// Live channels of the last successful import.
    pub channels: u32,
    pub updated_at: Option<i64>,
    /// The last refresh failed (and when); the channels before it remain.
    pub failed: Option<(ImportErrorKind, Option<i64>)>,
    /// The guide link the user gave it.
    pub guide_link: Option<SecretUrl>,
}

thread_local! {
    /// Running imports of stored playlists: their ticket and cancel token.
    static RUNNING: RefCell<HashMap<SourceId, (u64, CancelToken)>> = RefCell::default();
    static TICKETS: Cell<u64> = const { Cell::new(0) };
}

/// The stored playlists in list order.
pub fn playlists() -> Result<Vec<Playlist>, StoreError> {
    let store = open_store()?;
    store
        .sources()?
        .into_iter()
        .map(|s| {
            let guide = store.epg_guide(s.id)?;
            let settings = store.source_settings(s.id)?;
            Ok(Playlist {
                id: s.id,
                name: s.name,
                kind: s.kind,
                channels: s.import.counts.channels,
                updated_at: s.import.last_success_at,
                failed: s.import.last_error.map(|k| (k, s.import.last_error_at)),
                guide_link: stored::typed_guide(settings, guide.as_ref()),
            })
        })
        .collect()
}

/// The playlist to open at startup: the one shown last, else the newest.
pub(super) fn startup_source() -> Result<Option<StoredSource>, StoreError> {
    let store = open_store()?;
    let id = match store.active_source()? {
        Some(id) => Some(id),
        None => store.sources()?.last().map(|s| s.id),
    };
    match id {
        Some(id) => store.source(id),
        None => Ok(None),
    }
}

fn newest() -> Result<Option<StoredSource>, StoreError> {
    let store = open_store()?;
    match store.sources()?.last() {
        Some(summary) => store.source(summary.id),
        None => Ok(None),
    }
}

fn load(id: SourceId) -> Option<StoredSource> {
    match open_store().and_then(|s| s.source(id)) {
        Ok(found) => found,
        Err(e) => {
            log::warn!("iptv data: source {}: {e}", id.0);
            None
        }
    }
}

/// Remembers `id` as the playlist on screen (the next start opens it).
pub(super) fn remember_shown(id: SourceId) {
    if let Err(e) = open_store().and_then(|mut s| s.set_active_source(id)) {
        log::warn!("iptv data: could not remember the shown playlist: {e}");
    }
}

fn changed(ui: Ui) {
    ui.library.version.update(|v| *v += 1);
}

fn running(id: SourceId) -> bool {
    RUNNING.with(|r| r.borrow().contains_key(&id))
}

fn cancel_running(id: SourceId) {
    if let Some((_, token)) = RUNNING.with(|r| r.borrow_mut().remove(&id)) {
        log::info!(
            "iptv data: cancelling the running import of source {}",
            id.0
        );
        token.cancel();
    }
}

/// Imports stored playlist `stored` for `purpose`, reporting into the
/// library's activity.
pub(super) fn run(ui: Ui, stored: StoredSource, purpose: Purpose) {
    let id = stored.id;
    log::info!(
        "iptv data: {purpose:?} of source {} ({:?})",
        id.0,
        stored.source.kind()
    );
    let ticket = TICKETS.with(|t| {
        t.set(t.get() + 1);
        t.get()
    });
    let current =
        move || RUNNING.with(|r| r.borrow().get(&id).is_some_and(|(at, _)| *at == ticket));
    let job = Job {
        source: stored.source,
        epg_url: None,
        stored: Some(id),
        purpose,
    };
    let token = CancelToken::new();
    RUNNING.with(|r| r.borrow_mut().insert(id, (ticket, token.clone())));
    ui.library.activity.update(|a| {
        a.insert(id, Activity::Running(None));
    });
    start_import_with(ui, job, token, move |update| {
        if !current() {
            // Cancelled or replaced by a newer import of it, which reports.
            if matches!(update, Update::Done(_)) {
                changed(ui);
            }
            return;
        }
        let activity = match update {
            Update::Progress(p) => Activity::Running(Some(p)),
            Update::Done(result) => {
                match &result {
                    Ok(counts) => log::info!("iptv data: refreshed {counts:?}"),
                    Err(f) => {
                        log::warn!("iptv data: refresh failed ({f:?}); the stored data stays")
                    }
                }
                RUNNING.with(|r| r.borrow_mut().remove(&id));
                changed(ui);
                Activity::Done(result)
            }
        };
        ui.library.activity.update(|a| {
            a.insert(id, activity);
        });
    });
}

/// Shows stored playlist `id`: its stored channels and guide at once,
/// refreshed in the background as at startup.
pub fn switch_to(ui: Ui, id: SourceId) {
    if ui.library.shown.get_untracked() == Some(id) {
        return;
    }
    let Some(stored) = load(id) else {
        return;
    };
    // A refresh of it in the background gives way to the shown one.
    cancel_running(id);
    run(ui, stored, Purpose::Switch);
}

/// Downloads stored playlist `id` again (unless that already runs): the
/// shown one in place, another one into the store only.
pub fn refresh(ui: Ui, id: SourceId) {
    if running(id) {
        return;
    }
    let Some(stored) = load(id) else {
        return;
    };
    let purpose = if ui.library.shown.get_untracked() == Some(id) {
        Purpose::Refresh
    } else {
        Purpose::Background
    };
    run(ui, stored, purpose);
}

pub fn rename(ui: Ui, id: SourceId, name: &str) -> Result<(), StoreError> {
    open_store()?.rename_source(id, name)?;
    changed(ui);
    Ok(())
}

/// Sets (`None`: removes) the guide link of `id`, then refreshes it so
/// its guide comes from the new link.
pub fn set_guide_link(ui: Ui, id: SourceId, link: Option<SecretUrl>) -> Result<(), StoreError> {
    open_store()?.set_source_epg_url(id, link.as_ref())?;
    changed(ui);
    // A running refresh still has the old link.
    cancel_running(id);
    refresh(ui, id);
    Ok(())
}

/// Deletes stored playlist `id` with its channels, guide and favourites.
/// The shown one gives way to the newest other playlist, or to the
/// first-run state when none is left.
pub fn delete(ui: Ui, id: SourceId) -> Result<(), StoreError> {
    cancel_running(id);
    open_store()?.delete_source(id)?;
    log::info!("iptv data: deleted source {}", id.0);
    crate::player::forget_source(id);
    ui.library.activity.update(|a| {
        a.remove(&id);
    });
    changed(ui);
    if ui.library.shown.get_untracked() != Some(id) {
        return Ok(());
    }
    match newest()? {
        Some(next) => run(ui, next, Purpose::Switch),
        None => {
            super::show_nothing(ui);
            crate::playlists_view::close(ui);
            crate::wizard_view::open_first_run(ui);
        }
    }
    Ok(())
}
