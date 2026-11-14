//! The stored playlists (sources) as the playlists screen sees them, and
//! what it does to them: which one is shown (remembered in the store, so
//! the next start opens it), refresh, rename, its guide sources, delete.
//!
//! One refresh per playlist runs at a time; each has a ticket, so a
//! cancelled one never overwrites what a newer one reports.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::{Rc, Weak};

use iptv_core::wizard::{ImportCounts, ImportFailure, ImportProgress};
use iptv_import::CancelToken;
use iptv_providers::model::{Source, SourceConfig};
use iptv_store_sqlite::{SourceId, Store, StoreError, StoredSource};
use nsh_core::prelude::*;

use super::{Job, Purpose, Update, guides, start_import_with, stored};
use crate::Ui;

/// What the data layer tells the screens about the stored playlists.
#[derive(Clone, Copy)]
pub struct Library {
    /// The stored playlist on screen; `None` before one is (first run).
    pub shown: Signal<Option<SourceId>>,
    pub all: Signal<bool>,
    /// Refreshes of stored playlists since the app started.
    pub activity: Signal<BTreeMap<SourceId, Activity>>,
    /// Bumped when what is stored about the playlists changed.
    pub version: Signal<u64>,
}

impl Library {
    pub fn new() -> Self {
        Self {
            shown: signal(None),
            all: signal(false),
            activity: signal(BTreeMap::new()),
            version: signal(0),
        }
    }
}

/// A stored playlist's refresh (or check of new details) this session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Activity {
    Running(Option<ImportProgress>),
    /// Finished; on failure the stored channels stay. New details that
    /// imported are saved.
    Done(Result<ImportCounts, ImportFailure>),
    /// New details are being tried.
    Checking(Option<ImportProgress>),
    /// New details did not import: the stored ones and their data stay.
    Rejected(ImportFailure),
}

pub use iptv_library::Playlist;

thread_local! {
    /// Running imports of stored playlists: their ticket and cancel token.
    static RUNNING: RefCell<HashMap<SourceId, (u64, CancelToken)>> = RefCell::default();
    static ATTEMPTS: RefCell<HashMap<SourceId,i64>> = RefCell::default();
    static TICKETS: Cell<u64> = const { Cell::new(0) };
    // Unlike RUNNING, this lasts through post-playlist guide work. Weak entries
    // identify the latest worker without retaining completed guides or tasks.
    static COMPLETIONS: RefCell<HashMap<SourceId, Weak<ImportWork>>> = RefCell::default();
}
pub(super) type ImportCompletion = Rc<ImportWork>;
pub(super) struct ImportWork {
    finished: Cell<bool>,
    catalog_done: Cell<bool>,
    cancel: CancelToken,
    /// Tasks waiting for [`Self::finish`].
    waiting: RefCell<Vec<futures_channel::oneshot::Sender<()>>>,
}
impl ImportWork {
    pub(super) fn finished(&self) -> bool {
        self.finished.get()
    }
    pub(super) fn finish(&self) {
        self.finished.set(true);
        for waiting in self.waiting.take() {
            let _ = waiting.send(());
        }
    }
    /// Resolves when the work has finished (at once if it has).
    pub(super) fn until_finished(&self) -> impl std::future::Future<Output = ()> + 'static {
        let wait = (!self.finished.get()).then(|| {
            let (sender, receiver) = futures_channel::oneshot::channel();
            self.waiting.borrow_mut().push(sender);
            receiver
        });
        async move {
            if let Some(wait) = wait {
                let _ = wait.await;
            }
        }
    }
    pub(super) fn playlist_done(&self) {
        self.catalog_done.set(true);
    }
    pub(super) fn cancel_guide(&self) {
        if self.catalog_done.get() {
            self.cancel.cancel();
        }
    }
}
pub(super) fn import_completion(cancel: CancelToken) -> ImportCompletion {
    Rc::new(ImportWork {
        finished: Cell::new(false),
        catalog_done: Cell::new(false),
        cancel,
        waiting: RefCell::default(),
    })
}
pub(super) fn track_import(source: SourceId, completion: &ImportCompletion) {
    COMPLETIONS.with(|jobs| jobs.borrow_mut().insert(source, Rc::downgrade(completion)));
}
/// An Add learns its source ID only after saving. A newer known-source job may
/// already own that ID by then, including a worker that has just completed.
pub(super) fn track_added_import(source: SourceId, completion: &ImportCompletion) {
    COMPLETIONS.with(|jobs| {
        jobs.borrow_mut()
            .entry(source)
            .or_insert_with(|| Rc::downgrade(completion));
    });
}
pub(super) type ImportRevision = Option<Weak<ImportWork>>;
pub(super) fn import_revision(source: SourceId) -> ImportRevision {
    COMPLETIONS.with(|jobs| jobs.borrow().get(&source).cloned())
}
pub(super) fn import_revision_is_current(source: SourceId, revision: &ImportRevision) -> bool {
    match (import_revision(source), revision) {
        (None, None) => true,
        (Some(current), Some(previous)) => Weak::ptr_eq(&current, previous),
        _ => false,
    }
}
pub(super) fn pending_import(source: SourceId) -> Option<ImportCompletion> {
    COMPLETIONS
        .with(|jobs| jobs.borrow().get(&source).and_then(Weak::upgrade))
        .filter(|job| !job.finished())
}
pub(super) fn import_is_current(source: SourceId, completion: &ImportCompletion) -> bool {
    COMPLETIONS.with(|jobs| {
        jobs.borrow()
            .get(&source)
            .and_then(Weak::upgrade)
            .is_some_and(|current| Rc::ptr_eq(&current, completion))
    })
}
pub(super) fn revoke_import_handoff(source: SourceId) {
    if let Some(job) = COMPLETIONS.with(|jobs| {
        jobs.borrow_mut()
            .remove(&source)
            .and_then(|job| job.upgrade())
    }) {
        // Guide edits may happen during a source's authentication/catalog
        // phase. Revoke handoff then, but let that phase establish playback.
        job.cancel_guide();
    }
}
pub(super) fn reset_imports() {
    COMPLETIONS.with(|jobs| {
        for job in jobs
            .borrow_mut()
            .drain()
            .filter_map(|(_, job)| job.upgrade())
        {
            job.cancel.cancel();
        }
    });
}

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct Snapshot {
    playlists: Vec<Playlist>,
    details: BTreeMap<SourceId, StoredSource>,
    imports: BTreeMap<SourceId, iptv_store_sqlite::ImportStatus>,
    active: Option<SourceId>,
}
thread_local! { static SNAPSHOT: RefCell<Option<Snapshot>> = const { RefCell::new(None) }; }

pub(super) fn snapshot_in(store: &Store) -> Result<Snapshot, StoreError> {
    let playlists = playlists_in(store)?;
    let mut details = BTreeMap::new();
    let mut imports = BTreeMap::new();
    for playlist in &playlists {
        imports.insert(playlist.id, store.import_status(playlist.id)?);
        if let Some(source) = store.source(playlist.id)? {
            details.insert(playlist.id, source);
        }
    }
    Ok(Snapshot {
        playlists,
        details,
        imports,
        active: store.active_source()?,
    })
}
pub(super) fn install(snapshot: Snapshot) -> bool {
    SNAPSHOT.with(|current| {
        let changed = current.borrow().as_ref() != Some(&snapshot);
        *current.borrow_mut() = Some(snapshot);
        changed
    })
}
pub(super) async fn refresh_cache(ui: Ui) -> Result<(), StoreError> {
    let snapshot = crate::store_service::call(|store| snapshot_in(store)).await?;
    install(snapshot);
    changed(ui);
    Ok(())
}

/// A committed mutation must retain its cleanup even if a subsequent read fails.
async fn refresh_after_commit(ui: Ui, patch: impl FnOnce(&mut Snapshot)) {
    SNAPSHOT.with(|snapshot| {
        if let Some(snapshot) = snapshot.borrow_mut().as_mut() {
            patch(snapshot);
        }
    });
    changed(ui);
    if let Err(error) = refresh_cache(ui).await {
        log::warn!("iptv library: change saved but metadata refresh failed: {error}");
        crate::settings_view::notify("Change saved; library details could not be refreshed.");
    }
}

/// The stored playlists in list order.
pub fn playlists() -> Result<Vec<Playlist>, StoreError> {
    SNAPSHOT.with(|snapshot| {
        snapshot
            .borrow()
            .as_ref()
            .map(|snapshot| snapshot.playlists.clone())
            .ok_or(StoreError::Unavailable("library is loading"))
    })
}
fn playlists_in(store: &Store) -> Result<Vec<Playlist>, StoreError> {
    store
        .sources()?
        .into_iter()
        .map(|s| {
            let guide = store.epg_guide(s.id)?;
            let guides = store.guide_sources(s.id)?;
            let config = store.source(s.id)?.map(|stored| stored.source.config);
            let (user_agent, from_file) = match &config {
                Some(SourceConfig::M3u(c)) => {
                    (c.user_agent.clone(), !c.location.expose().contains("://"))
                }
                Some(SourceConfig::Xtream(c)) => (c.user_agent.clone(), false),
                _ => (None, false),
            };
            Ok(Playlist {
                policy: store.source_policy(s.id)?,
                id: s.id,
                name: s.name,
                kind: s.kind,
                channels: s.import.counts.channels,
                updated_at: s.import.last_success_at,
                failed: s.import.last_error.map(|k| (k, s.import.last_error_at)),
                guide_failed: guide.as_ref().is_some_and(|g| g.last_error.is_some()),
                guides,
                user_agent,
                from_file,
            })
        })
        .collect()
}

/// The playlist to open at startup: the one shown last, else the newest.
pub(super) fn startup_source(store: &Store) -> Result<Option<StoredSource>, StoreError> {
    if let Some(id) = store.active_source()?
        && store.source_policy(id)?.enabled
    {
        return store.source(id);
    }
    for summary in store.sources()?.iter().rev() {
        if store.source_policy(summary.id)?.enabled {
            return store.source(summary.id);
        }
    }
    Ok(None)
}

fn newest() -> Result<Option<StoredSource>, StoreError> {
    SNAPSHOT.with(|snapshot| {
        let snapshot = snapshot.borrow();
        let snapshot = snapshot
            .as_ref()
            .ok_or(StoreError::Unavailable("library is loading"))?;
        let active = snapshot.active.filter(|id| {
            snapshot
                .playlists
                .iter()
                .any(|p| p.id == *id && p.policy.enabled)
        });
        let selected = active.or_else(|| {
            snapshot
                .playlists
                .iter()
                .rev()
                .find(|p| p.policy.enabled)
                .map(|p| p.id)
        });
        Ok(selected.and_then(|id| snapshot.details.get(&id).cloned()))
    })
}

pub fn policy(id: SourceId) -> Result<iptv_store_sqlite::SourcePolicy, StoreError> {
    playlists()?
        .into_iter()
        .find(|source| source.id == id)
        .map(|source| source.policy)
        .ok_or(StoreError::SourceNotFound(id))
}
pub async fn set_policy(
    ui: Ui,
    id: SourceId,
    policy: iptv_store_sqlite::SourcePolicy,
) -> Result<(), StoreError> {
    crate::store_service::call(move |store| store.set_source_policy(id, policy)).await?;
    refresh_after_commit(ui, |snapshot| {
        if let Some(playlist) = snapshot.playlists.iter_mut().find(|p| p.id == id) {
            playlist.policy = policy;
        }
    })
    .await;
    if super::combined::active(ui) {
        if !policy.enabled {
            cancel_running(id);
        }
        super::combined::rebuild(ui);
        if policy.enabled
            && let Some(source) = load(id)
        {
            run(ui, source, Purpose::Combined, false);
        }
        return Ok(());
    }
    if !policy.enabled {
        cancel_running(id);
        if ui.library.shown.get_untracked() == Some(id) {
            let next = newest()?;
            super::show_nothing(ui);
            match next {
                Some(next) => run(ui, next, Purpose::Switch, false),
                None => crate::playlists_view::open(ui),
            }
        }
    }
    Ok(())
}
/// Cached startup/switch can avoid a playlist download independently of EPG policy.
pub(super) fn network_due(id: SourceId, purpose: Purpose) -> bool {
    if !matches!(
        purpose,
        Purpose::Startup | Purpose::Switch | Purpose::Combined
    ) {
        return true;
    }
    crate::store_service::blocking(move |store| {
        Ok((store.source_policy(id)?, store.import_status(id)?))
    })
    .map(|(p, status)| {
        status.last_success_at.is_none()
            || p.due(
                super::now_unix(),
                status.last_attempt_at,
                matches!(purpose, Purpose::Startup | Purpose::Combined),
            )
    })
    .unwrap_or(true)
}
/// UI-owned scheduler; never wakes the operating system or schedules disabled sources.
pub fn start_scheduler(ui: Ui) {
    super::spawn_in_app(async move {
        let mut startup = true;
        loop {
            if lifecycle() == nsh_core::platform::Lifecycle::Background {
                timer(std::time::Duration::from_secs(60)).await;
                continue;
            }
            let sources = crate::store_service::call(|store| {
                store
                    .sources()?
                    .into_iter()
                    .map(|source| {
                        let policy = store.source_policy(source.id)?;
                        Ok((source, policy))
                    })
                    .collect::<Result<Vec<_>, StoreError>>()
            })
            .await;
            if let Ok(sources) = sources {
                for (source, policy) in sources {
                    let attempted = ATTEMPTS
                        .with(|a| a.borrow().get(&source.id).copied())
                        .or(source.import.last_attempt_at);
                    if policy.due(super::now_unix(), attempted, startup) && !running(source.id) {
                        refresh(ui, source.id);
                    }
                }
            }
            startup = false;
            timer(std::time::Duration::from_secs(60)).await;
        }
    });
}

pub(super) fn load(id: SourceId) -> Option<StoredSource> {
    SNAPSHOT.with(|snapshot| {
        snapshot
            .borrow()
            .as_ref()
            .and_then(|snapshot| snapshot.details.get(&id).cloned())
    })
}

/// Persists the shown identity without delaying navigation.
pub(super) fn remember_shown(id: SourceId) {
    let saved = crate::store_service::call(move |store| store.set_active_source(id));
    super::spawn_in_app(async move {
        if let Err(error) = saved.await {
            log::warn!("iptv data: could not remember the shown playlist: {error}");
        }
    });
}

fn changed(ui: Ui) {
    ui.library.version.update(|v| *v += 1);
}

pub(super) fn running(id: SourceId) -> bool {
    RUNNING.with(|r| r.borrow().contains_key(&id))
}

fn cancel_running(id: SourceId) {
    revoke_import_handoff(id);
    if let Some((_, token)) = RUNNING.with(|r| r.borrow_mut().remove(&id)) {
        log::info!(
            "iptv data: cancelling the running import of source {}",
            id.0
        );
        token.cancel();
    }
}

/// Imports stored playlist `stored` for `purpose`, reporting into the
/// library's activity. With `edit`, `stored.source` holds new details to
/// try (saved only if they import).
pub(super) fn run(ui: Ui, stored: StoredSource, purpose: Purpose, edit: bool) {
    let id = stored.id;
    // A mode switch can replace a still-running job for this source.
    cancel_running(id);
    let due = !matches!(
        purpose,
        Purpose::Startup | Purpose::Switch | Purpose::Combined
    ) || SNAPSHOT.with(|snapshot| {
        let snapshot = snapshot.borrow();
        let Some(snapshot) = snapshot.as_ref() else {
            return true;
        };
        let Some(playlist) = snapshot.playlists.iter().find(|playlist| playlist.id == id) else {
            return true;
        };
        let Some(status) = snapshot.imports.get(&id) else {
            return true;
        };
        status.last_success_at.is_none()
            || playlist.policy.due(
                super::now_unix(),
                status.last_attempt_at,
                matches!(purpose, Purpose::Startup | Purpose::Combined),
            )
    });
    if due {
        ATTEMPTS.with(|a| a.borrow_mut().insert(id, super::now_unix()));
    }
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
        edit,
    };
    let token = CancelToken::new();
    RUNNING.with(|r| r.borrow_mut().insert(id, (ticket, token.clone())));
    ui.library.activity.update(|a| {
        a.insert(
            id,
            if edit {
                Activity::Checking(None)
            } else {
                Activity::Running(None)
            },
        );
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
            Update::Progress(p) if edit => Activity::Checking(Some(p)),
            Update::Progress(p) => Activity::Running(Some(p)),
            Update::Done(Err(f)) if edit => {
                log::warn!("iptv data: the new details did not import ({f:?}); the old ones stay");
                RUNNING.with(|r| r.borrow_mut().remove(&id));
                changed(ui);
                Activity::Rejected(f)
            }
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
    super::spawn_in_app(async move {
        if crate::favorites::flush().await {
            switch_to_ready(ui, id);
        } else {
            crate::settings_view::notify("Recent changes could not be saved; try again");
        }
    });
}
fn switch_to_ready(ui: Ui, id: SourceId) {
    if policy(id).is_ok_and(|p| !p.enabled) {
        return;
    }
    if !super::combined::active(ui) && ui.library.shown.get_untracked() == Some(id) {
        return;
    }
    let Some(stored) = load(id) else {
        return;
    };
    super::combined::leave(ui);
    // A refresh of it in the background gives way to the shown one.
    cancel_running(id);
    run(ui, stored, Purpose::Switch, false);
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
    run(ui, stored, refresh_purpose(ui, id), false);
}

/// The shown playlist refreshes in place, another one into the store only.
fn refresh_purpose(ui: Ui, id: SourceId) -> Purpose {
    if super::combined::active(ui) {
        Purpose::Background
    } else if ui.library.shown.get_untracked() == Some(id) {
        Purpose::Refresh
    } else {
        Purpose::Background
    }
}

/// Refreshes every stored playlist (each one not refreshing already).
pub fn refresh_all(ui: Ui) {
    match playlists() {
        Ok(all) => all
            .iter()
            .filter(|p| p.policy.enabled)
            .for_each(|p| refresh(ui, p.id)),
        Err(e) => log::warn!("iptv data: the store is unavailable: {e}"),
    }
}

/// Stored playlist `id` with its details (secrets included: for the
/// details form only, never shown or logged).
pub fn source_details(id: SourceId) -> Option<Source> {
    load(id).map(|stored| stored.source)
}

/// Tries `source` (new details of stored playlist `id`): they replace the
/// stored ones once they imported; on failure the playlist stays as it was.
pub fn try_details(ui: Ui, id: SourceId, source: Source) {
    let Some(mut stored) = load(id) else {
        return;
    };
    // Only the details change; the name is the stored one.
    stored.source.config = source.config;
    cancel_running(id);
    run(ui, stored, refresh_purpose(ui, id), true);
}

/// Sets the User-Agent of playlist `id` (`None`: the app's own), tried
/// like other new details.
pub fn set_user_agent(ui: Ui, id: SourceId, user_agent: Option<String>) {
    let Some(mut source) = source_details(id) else {
        return;
    };
    match &mut source.config {
        SourceConfig::M3u(c) => c.user_agent = user_agent,
        SourceConfig::Xtream(c) => c.user_agent = user_agent,
        SourceConfig::Stalker(_) => return,
    }
    try_details(ui, id, source);
}

pub async fn rename(ui: Ui, id: SourceId, name: &str) -> Result<(), StoreError> {
    let name = name.to_owned();
    let saved_name = name.clone();
    crate::store_service::call(move |store| store.rename_source(id, &name)).await?;
    refresh_after_commit(ui, |snapshot| {
        if let Some(playlist) = snapshot.playlists.iter_mut().find(|p| p.id == id) {
            playlist.name = saved_name.clone();
        }
        if let Some(source) = snapshot.details.get_mut(&id) {
            source.source.name = saved_name;
        }
    })
    .await;
    if super::combined::active(ui) {
        super::combined::rebuild(ui);
    }
    Ok(())
}

/// Changes the guide sources of playlist `id`; false when `edit` changes
/// nothing. The guide on screen is checked again once the edits settle
/// (changed feeds are downloaded); another playlist's when it is shown.
pub async fn edit_guides(ui: Ui, id: SourceId, edit: &guides::Edit) -> Result<bool, StoreError> {
    let edit = edit.clone();
    let edited = crate::store_service::call(move |store| {
        let Some(sources) = guides::apply(&store.guide_sources(id)?, &edit) else {
            return Ok(false);
        };
        store.set_guide_sources(id, &sources)?;
        Ok(true)
    })
    .await?;
    if !edited {
        return Ok(false);
    }
    refresh_after_commit(ui, |_| {}).await;
    revoke_import_handoff(id);
    stored::guides_changed(ui, id);
    Ok(true)
}

/// Deletes stored playlist `id` with its channels, guide and favourites.
/// The shown one gives way to the newest other playlist, or to the
/// first-run state when none is left.
pub async fn delete(ui: Ui, id: SourceId) -> Result<(), StoreError> {
    cancel_running(id);
    crate::store_service::call(move |store| store.delete_source(id)).await?;
    refresh_after_commit(ui, |snapshot| {
        snapshot.playlists.retain(|p| p.id != id);
        snapshot.details.remove(&id);
        snapshot.imports.remove(&id);
        if snapshot.active == Some(id) {
            snapshot.active = None;
        }
    })
    .await;
    log::info!("iptv data: deleted source {}", id.0);
    crate::player::forget_source(id);
    ui.library.activity.update(|a| {
        a.remove(&id);
    });
    changed(ui);
    if super::combined::active(ui) {
        // Keep the explicitly selected aggregate usable when its last source
        // is deleted; the playlist screen still offers Add playlist.
        super::combined::rebuild(ui);
        return Ok(());
    }
    if playlists()?.is_empty() {
        super::show_nothing(ui);
        crate::playlists_view::close(ui);
        crate::wizard_view::open_first_run(ui);
        return Ok(());
    }
    if ui.library.shown.get_untracked() != Some(id) {
        return Ok(());
    }
    match newest()? {
        Some(next) => run(ui, next, Purpose::Switch, false),
        None => {
            super::show_nothing(ui);
            crate::playlists_view::close(ui);
            if playlists()?.is_empty() {
                crate::wizard_view::open_first_run(ui);
            } else {
                crate::playlists_view::open(ui);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    use iptv_providers::{SecretUrl, model::M3uSourceConfig};
    #[test]
    fn startup_ignores_disabled_active_source_and_recovers_when_reenabled() {
        let mut s = Store::open_in_memory().unwrap();
        let source = |name: &str| Source {
            name: name.into(),
            config: SourceConfig::M3u(M3uSourceConfig {
                location: SecretUrl::new("http://example.test/list.m3u"),
                user_agent: None,
                referrer: None,
            }),
        };
        let a = s.add_source(&source("A")).unwrap();
        let b = s.add_source(&source("B")).unwrap();
        s.set_active_source(a).unwrap();
        assert_eq!(startup_source(&s).unwrap().unwrap().id, a);
        let off = iptv_store_sqlite::SourcePolicy {
            enabled: false,
            ..Default::default()
        };
        s.set_source_policy(a, off).unwrap();
        assert_eq!(startup_source(&s).unwrap().unwrap().id, b);
        s.set_source_policy(b, off).unwrap();
        assert!(startup_source(&s).unwrap().is_none());
        s.set_source_policy(a, Default::default()).unwrap();
        assert_eq!(startup_source(&s).unwrap().unwrap().id, a);
    }
}
