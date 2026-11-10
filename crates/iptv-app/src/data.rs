//! Real data: imports a source and its EPG off the UI thread, keeps it in
//! the SQLite store, and swaps the guide to the imported schedule.
//!
//! At startup the newest stored source is refreshed; with none, the
//! add-playlist wizard opens on its empty state (`wizard_view`). The wizard
//! adds sources through [`start_import`]. A new source is stored only once
//! its import succeeded, so a failed or cancelled attempt leaves nothing
//! behind.
//!
//! A stored source's saved channels and guide are on screen before its
//! refresh reaches the network, and stay when the refresh fails. Its guide
//! is downloaded again only when the refresh policy says so (`stored`).

mod stored;
mod wizard_map;

use std::cell::Cell;
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Sender, TryRecvError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iptv_core::wizard::{
    FailureKind, Field, ImportCounts, ImportFailure, ImportProgress, ImportStage,
};
use iptv_epg::{GuideStore, Overrides};
use iptv_guide::EpgSchedule;
use iptv_guide::playback::PlaybackContext;
use iptv_import::{
    CancelToken, Catalog, Decision, EpgImport, EpgSourceConfig, ImportError, ImportOptions,
    NoProgress, Progress, ProgressSink, Trigger, import_epg, import_source,
};
use iptv_net::{NetClient, NetConfig};
use iptv_providers::SecretUrl;
use iptv_providers::model::{Channel, Group, Source};
use iptv_store_sqlite::{CatalogCounts, SourceId, Store, StoreError, StoredSource};
use nsh_core::prelude::*;

pub use wizard_map::source_from_draft;

use crate::Ui;

/// The URL the wizard's M3U form starts with: the synthetic development
/// playlist on the local mock server (`tools/mock-server.py` with
/// `tools/make-iptv-fixtures.py`); the emulator reaches the host as
/// 10.0.2.2. Development only: remove before release.
#[cfg(target_os = "android")]
pub const EXAMPLE_PLAYLIST: &str = "http://10.0.2.2:8089/iptv/playlist.m3u";
#[cfg(not(target_os = "android"))]
pub const EXAMPLE_PLAYLIST: &str = "http://127.0.0.1:8089/iptv/playlist.m3u";

/// Id of a guide the user typed; the source's own guides follow it.
const CUSTOM_EPG: iptv_epg::SourceId = iptv_epg::SourceId(1);
/// How often the UI picks up the import thread's reports.
const POLL: Duration = Duration::from_millis(100);
/// Guide checks wait at most this long at a time (a sleeping device can
/// delay timers; an early check finds nothing due and waits again)...
const MAX_WAIT: Duration = Duration::from_secs(6 * 3600);
/// ...and at least this long.
const MIN_WAIT: Duration = Duration::from_secs(60);

/// An import to run.
pub struct Job {
    pub source: Source,
    /// A guide the user named, tried before the source's own guides. When
    /// set, it must load for the import to succeed.
    pub epg_url: Option<SecretUrl>,
    /// `None`: add the source to the store once it imported. `Some`:
    /// refresh that stored source.
    pub stored: Option<SourceId>,
}

/// What an import reports, on the UI thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    Progress(ImportProgress),
    /// The catalog is saved and on screen; its guide may still be loading.
    Done(Result<ImportCounts, ImportFailure>),
}

/// The lists the guide is built from.
struct Imported {
    groups: Vec<Group>,
    channels: Vec<Channel>,
    /// What the source's streams need (headers, account, session).
    playback: Streams,
    guide: GuideStore,
    now: i64,
    /// The stored source, whose descriptions are read on demand.
    source: Option<SourceId>,
    /// Whether the last channel may start (its stream can be resolved).
    resume: bool,
}

/// Where a new schedule gets its playback context.
enum Streams {
    New(Option<PlaybackContext>),
    /// The schedule on screen's (a refreshed guide of the same source).
    Current,
}

enum Msg {
    Progress(ImportProgress),
    /// When to look at `source`'s guide again (unix time).
    NextCheck {
        source: SourceId,
        at: i64,
    },
    Playlist(Result<(ImportCounts, Box<Imported>), ImportFailure>),
    /// What the store has (before the playlist), or the source's guide
    /// loaded after the playlist was reported.
    Guide(Box<Imported>),
}

thread_local! {
    /// The app's root owner: import tasks outlive the view that started them.
    static APP_OWNER: Cell<Option<Owner>> = const { Cell::new(None) };
    /// The latest import; an older one no longer changes the guide.
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Where the app keeps its files: the Android app's files directory, or
/// `~/.local/share/iptv-rs` on the desktop.
pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        let cmdline = std::fs::read("/proc/self/cmdline").unwrap_or_default();
        let package = cmdline
            .split(|b| *b == 0)
            .next()
            .map(|p| String::from_utf8_lossy(p).into_owned());
        let package = package.unwrap_or_else(|| "com.hantekllc.iptvrs".to_owned());
        PathBuf::from(format!(
            "/data/data/{}/files",
            package.split(':').next().unwrap_or_default()
        ))
    }
    #[cfg(not(target_os = "android"))]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        home.join(".local/share/iptv-rs")
    }
}

pub(crate) fn open_store() -> Result<Store, StoreError> {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    Store::open(dir.join("iptv.sqlite"))
}

/// Runs `future` on the UI thread under the app's root owner.
pub fn spawn_in_app(future: impl Future<Output = ()> + 'static) {
    let owner = APP_OWNER.with(Cell::get).unwrap_or_else(Owner::current);
    owner.run(move || {
        // Dropping the handle detaches the task; it lives with the owner.
        drop(spawn(future));
    });
}

/// Startup: refreshes the newest stored source, or opens the wizard.
pub fn start(ui: Ui) {
    APP_OWNER.with(|o| o.set(Some(Owner::current())));
    match newest_source() {
        Ok(Some(stored)) => {
            log::info!(
                "iptv data: refreshing source {} ({:?})",
                stored.id.0,
                stored.source.kind()
            );
            let job = Job {
                source: stored.source,
                epg_url: None,
                stored: Some(stored.id),
            };
            let _ = start_import(ui, job, |update| match update {
                Update::Done(Ok(counts)) => log::info!("iptv data: refreshed {counts:?}"),
                Update::Done(Err(f)) => {
                    log::warn!("iptv data: refresh failed ({f:?}); keeping what is on screen")
                }
                Update::Progress(_) => {}
            });
        }
        Ok(None) => crate::wizard_view::open_first_run(ui),
        Err(e) => {
            log::warn!("iptv data: store unavailable ({e}); opening the wizard");
            crate::wizard_view::open_first_run(ui);
        }
    }
}

fn newest_source() -> Result<Option<StoredSource>, StoreError> {
    let store = open_store()?;
    match store.sources()?.last() {
        Some(summary) => store.source(summary.id),
        None => Ok(None),
    }
}

/// Imports `job` on its own thread. `on_update` hears progress (coalesced
/// per poll) and then one [`Update::Done`]; on success the guide switches
/// to the source at once and again when its guide data arrives. Cancel
/// with the returned token.
pub fn start_import(ui: Ui, job: Job, on_update: impl Fn(Update) + 'static) -> CancelToken {
    let cancel = CancelToken::new();
    let generation = GENERATION.with(|g| {
        g.set(g.get() + 1);
        g.get()
    });
    // A stored source's refresh may start the last channel; a new one not.
    let refresh = job.stored.is_some();
    let (tx, rx) = mpsc::channel();
    {
        let cancel = cancel.clone();
        std::thread::spawn(move || run(job, &tx, &cancel));
    }
    let show = move |imported: Box<Imported>| show(ui, imported, generation, refresh);
    spawn_in_app(async move {
        let mut finished = false;
        loop {
            let mut progress = None;
            let mut open = true;
            loop {
                match rx.try_recv() {
                    Ok(Msg::Progress(p)) => progress = Some(p),
                    Ok(Msg::Playlist(result)) => {
                        if let Some(p) = progress.take() {
                            on_update(Update::Progress(p));
                        }
                        finished = true;
                        on_update(Update::Done(result.map(|(counts, imported)| {
                            show(imported);
                            counts
                        })));
                    }
                    Ok(Msg::Guide(imported)) => show(imported),
                    Ok(Msg::NextCheck { source, at }) => {
                        schedule_check(ui, source, at, generation);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        open = false;
                        break;
                    }
                }
            }
            if let Some(p) = progress.filter(|_| !finished) {
                on_update(Update::Progress(p));
            }
            if !open {
                if !finished {
                    log::warn!("iptv data: import thread ended without a result");
                    on_update(Update::Done(Err(ImportFailure::new(FailureKind::Network))));
                }
                break;
            }
            timer(POLL).await;
        }
    });
    cancel
}

/// Puts `imported` on screen unless a newer import owns the guide.
fn show(ui: Ui, imported: Box<Imported>, generation: u64, refresh: bool) {
    if GENERATION.with(Cell::get) != generation {
        return;
    }
    let channels = imported.channels.len();
    let schedule = EpgSchedule::from_catalog(
        &imported.groups,
        &imported.channels,
        imported.guide,
        imported.now,
    );
    let mut schedule = match imported.playback {
        Streams::New(context) => schedule.with_playback(context),
        Streams::Current => {
            let current = crate::schedule();
            let shared = current
                .as_any()
                .and_then(|s| s.downcast_ref::<EpgSchedule>())
                .and_then(EpgSchedule::shared_playback);
            schedule.with_shared_playback(shared)
        }
    };
    if let Some(lookup) = imported.source.and_then(stored::description_lookup) {
        schedule = schedule.with_description_lookup(lookup);
    }
    log::info!("iptv data: guide switched to {channels} imported channels");
    crate::favorites::set_source(imported.source);
    crate::set_schedule(&ui, Rc::new(schedule));
    crate::player::source_ready(ui, refresh && imported.resume);
}

/// Looks at `source`'s guide again at `at` (unix time), unless a newer
/// import started meanwhile.
fn schedule_check(ui: Ui, source: SourceId, at: i64, generation: u64) {
    let wait =
        Duration::from_secs(u64::try_from(at - now_unix()).unwrap_or(0)).clamp(MIN_WAIT, MAX_WAIT);
    log::info!("iptv data: next guide check in {} min", wait.as_secs() / 60);
    spawn_in_app(async move {
        timer(wait).await;
        if GENERATION.with(Cell::get) == generation {
            check_guide(ui, source, generation);
        }
    });
}

/// A guide check on its own thread: downloads the guide again when the
/// refresh policy says so, then schedules the next check.
fn check_guide(ui: Ui, source: SourceId, generation: u64) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        with_runtime(|| guide_pipeline(source, &tx));
    });
    spawn_in_app(async move {
        loop {
            match rx.try_recv() {
                Ok(Msg::Guide(imported)) => show(ui, imported, generation, false),
                Ok(Msg::NextCheck { source, at }) => schedule_check(ui, source, at, generation),
                Ok(_) => {}
                Err(TryRecvError::Empty) => timer(POLL).await,
                Err(TryRecvError::Disconnected) => break,
            }
        }
    });
}

/// Runs `future` on a small tokio runtime on this thread; false when the
/// runtime cannot start.
fn with_runtime<F: Future<Output = ()>>(future: impl FnOnce() -> F) -> bool {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => {
            runtime.block_on(future());
            true
        }
        Err(e) => {
            log::warn!("iptv data: runtime: {e}");
            false
        }
    }
}

/// The import itself, on its own thread with a small tokio runtime.
fn run(job: Job, tx: &Sender<Msg>, cancel: &CancelToken) {
    if !with_runtime(|| pipeline(job, tx, cancel)) {
        let _ = tx.send(Msg::Playlist(Err(ImportFailure::new(FailureKind::Network))));
    }
}

/// The timer's guide check: the stored feeds for the stored channels.
async fn guide_pipeline(source: SourceId, tx: &Sender<Msg>) {
    let now = now_unix();
    let Some(state) = stored::guide_state(source) else {
        return;
    };
    let guides = state.configs();
    let (groups, channels) = match stored::catalog(source) {
        Ok(catalog) => catalog,
        Err(e) => {
            log::warn!("iptv data: stored catalog: {e}");
            return;
        }
    };
    match stored::plan(Trigger::Timer, &guides, &channels, Some(&state), now) {
        Decision::Refresh(reason) => {
            log::info!("iptv data: downloading the guide ({reason:?})");
            match NetClient::new(NetConfig::default()) {
                Ok(net) => {
                    let cancel = CancelToken::new();
                    let result = download_guide(
                        &net,
                        &guides,
                        &channels,
                        now,
                        &mut NoProgress,
                        &cancel,
                        false,
                    )
                    .await;
                    match result {
                        Ok(import) => {
                            let guide = keep_guide(source, import, &guides, &channels, now);
                            let _ = tx.send(Msg::Guide(Box::new(Imported {
                                groups,
                                channels: channels.clone(),
                                playback: Streams::Current,
                                guide,
                                now,
                                source: Some(source),
                                resume: false,
                            })));
                        }
                        Err(e) => {
                            log::warn!("iptv data: guide: {e}");
                            stored::record_failure(source, &e);
                        }
                    }
                }
                Err(e) => log::warn!("iptv data: network client setup failed: {e}"),
            }
        }
        Decision::Discard => stored::discard(source),
        Decision::Keep { .. } | Decision::Backoff { .. } => {}
    }
    send_next_check(tx, source, &guides, &channels);
}

/// Tells the UI when to look at the guide again, if ever.
fn send_next_check(
    tx: &Sender<Msg>,
    source: SourceId,
    guides: &[EpgSourceConfig],
    channels: &[Channel],
) {
    if let Some(at) = stored::next_check(source, guides, channels, now_unix()) {
        let _ = tx.send(Msg::NextCheck { source, at });
    }
}

/// Reports to the UI as the wizard's progress.
struct Forward<'a>(&'a Sender<Msg>);

impl ProgressSink for Forward<'_> {
    fn report(&mut self, progress: &Progress) {
        let _ = self
            .0
            .send(Msg::Progress(wizard_map::progress_of(progress)));
    }
}

async fn pipeline(job: Job, tx: &Sender<Msg>, cancel: &CancelToken) {
    let fail = |failure: ImportFailure| {
        let _ = tx.send(Msg::Playlist(Err(failure)));
    };
    let now = now_unix();
    let stored_guide = job.stored.and_then(stored::guide_state);
    // A stored source: what the store has is on screen before any network.
    if let Some(view) = job.stored.and_then(|id| stored_view(&job, id, now)) {
        let _ = tx.send(Msg::Guide(view));
    }
    let Ok(net) = NetClient::new(NetConfig::default()) else {
        log::warn!("iptv data: network client setup failed");
        return fail(ImportFailure::new(FailureKind::Network));
    };
    let mut progress = Forward(tx);
    let options = ImportOptions::new(now);
    let catalog =
        match import_source(&net, &job.source.config, &options, &mut progress, cancel).await {
            Ok(catalog) => catalog,
            Err(e) => {
                log::warn!("iptv data: {:?} import failed: {e}", job.source.kind());
                record_failure(job.stored, &e);
                // The stored guide stays on screen: keep it current.
                if let (Some(id), Some(state)) = (job.stored, stored_guide.as_ref())
                    && let Ok((_, channels)) = stored::catalog(id)
                {
                    send_next_check(tx, id, &state.configs(), &channels);
                }
                return fail(wizard_map::failure_of(&e));
            }
        };
    if catalog.is_empty() {
        return fail(ImportFailure::new(FailureKind::EmptyPlaylist));
    }
    let guides = stored::configs(
        stored::own_guides(&catalog),
        job.epg_url.clone(),
        stored_guide.as_ref(),
    );
    // A guide the user typed is part of adding the source: it must load
    // before the source is kept, and its failure points at its field.
    let typed_guide = match job.epg_url {
        Some(_) => {
            match download_guide(
                &net,
                &guides,
                &catalog.channels,
                now,
                &mut progress,
                cancel,
                true,
            )
            .await
            {
                Ok(import) => Some(import),
                Err(e) => {
                    log::warn!("iptv data: the typed guide failed: {e}");
                    let mut failure = wizard_map::failure_of(&e);
                    if failure.kind != FailureKind::Cancelled {
                        failure.field = Some(Field::EpgUrl);
                    }
                    return fail(failure);
                }
            }
        }
        None => None,
    };
    if cancel.is_cancelled() {
        return fail(ImportFailure::new(FailureKind::Cancelled));
    }
    let _ = tx.send(Msg::Progress(ImportProgress {
        stage: ImportStage::Saving,
        done: 0,
        total: None,
    }));
    let (id, counts) = match save(&job, &catalog) {
        Ok(saved) => saved,
        Err(e) => {
            log::warn!("iptv data: store: {e}");
            return fail(ImportFailure::new(FailureKind::Network));
        }
    };
    let counts = ImportCounts {
        channels: counts.channels,
        groups: counts.groups,
        movies: counts.vod,
        series: counts.series,
    };
    let imported = |guide| {
        Box::new(Imported {
            groups: catalog.groups.clone(),
            channels: catalog.channels.clone(),
            playback: Streams::New(crate::player::context_for(
                &job.source.config,
                &catalog.source,
            )),
            guide,
            now,
            source: Some(id),
            resume: true,
        })
    };
    let report = |guide| {
        let _ = tx.send(Msg::Playlist(Ok((counts, imported(guide)))));
    };
    if let Some(import) = typed_guide {
        report(keep_guide(id, import, &guides, &catalog.channels, now));
        return send_next_check(tx, id, &guides, &catalog.channels);
    }
    let trigger = if job.stored.is_some() {
        Trigger::Startup
    } else {
        Trigger::PlaylistChanged
    };
    match stored::plan(
        trigger,
        &guides,
        &catalog.channels,
        stored_guide.as_ref(),
        now,
    ) {
        Decision::Refresh(reason) => {
            log::info!("iptv data: downloading the guide ({reason:?})");
            report(stored::load_guide(id, now).unwrap_or_default());
            // The guide loads after the channels are on screen.
            match download_guide(
                &net,
                &guides,
                &catalog.channels,
                now,
                &mut NoProgress,
                cancel,
                false,
            )
            .await
            {
                Ok(import) => {
                    let guide = keep_guide(id, import, &guides, &catalog.channels, now);
                    let _ = tx.send(Msg::Guide(imported(guide)));
                }
                Err(ImportError::Cancelled) => {}
                Err(e) => {
                    log::warn!("iptv data: guide: {e}");
                    stored::record_failure(id, &e);
                }
            }
        }
        Decision::Discard => {
            stored::discard(id);
            report(GuideStore::default());
        }
        decision => {
            log::info!("iptv data: using the stored guide ({decision:?})");
            report(stored::load_guide(id, now).unwrap_or_default());
        }
    }
    send_next_check(tx, id, &guides, &catalog.channels);
}

/// A stored source's saved channels and guide, as they were last imported.
fn stored_view(job: &Job, id: SourceId, now: i64) -> Option<Box<Imported>> {
    let (groups, channels) = match stored::catalog(id) {
        Ok(catalog) => catalog,
        Err(e) => {
            log::warn!("iptv data: stored catalog: {e}");
            return None;
        }
    };
    if channels.is_empty() {
        return None;
    }
    let context = crate::player::stored_context(&job.source.config);
    log::info!(
        "iptv data: showing {} stored channels before the refresh",
        channels.len()
    );
    Some(Box::new(Imported {
        groups,
        channels,
        resume: context.is_some(),
        playback: Streams::New(context),
        guide: stored::load_guide(id, now).unwrap_or_default(),
        now,
        source: Some(id),
    }))
}

/// Stores a downloaded guide and reads it back without descriptions; the
/// downloaded one when the store fails.
fn keep_guide(
    id: SourceId,
    import: EpgImport,
    guides: &[EpgSourceConfig],
    channels: &[Channel],
    now: i64,
) -> GuideStore {
    match stored::replace(id, &import, guides, channels, now) {
        Ok(_) => stored::load_guide(id, now).unwrap_or(import.guide),
        Err(e) => {
            log::warn!("iptv data: could not store the guide: {e}");
            import.guide
        }
    }
}

/// Downloads `guides` for the catalog's channels. With `typed_first`, the
/// first guide (the one the user typed) must load.
async fn download_guide<P: ProgressSink>(
    net: &NetClient,
    guides: &[EpgSourceConfig],
    channels: &[Channel],
    now: i64,
    progress: &mut P,
    cancel: &CancelToken,
    typed_first: bool,
) -> Result<EpgImport, ImportError> {
    let epg = import_epg(
        net,
        guides,
        channels,
        &Overrides::default(),
        now,
        progress,
        cancel,
    )
    .await?;
    if typed_first
        && let Some(Err(e)) = epg
            .sources
            .iter()
            .find(|o| o.id == CUSTOM_EPG)
            .map(|o| &o.result)
    {
        return Err(e.clone());
    }
    Ok(epg)
}

/// Stores the catalog: under a new source (removed again if the catalog
/// cannot be written) or the refreshed one.
fn save(job: &Job, catalog: &Catalog) -> Result<(SourceId, CatalogCounts), StoreError> {
    let mut store = open_store()?;
    let (id, added) = match job.stored {
        Some(id) => (id, false),
        None => (store.add_source(&job.source)?, true),
    };
    let written = store.replace_catalog(
        id,
        &catalog.groups,
        &catalog.channels,
        &catalog.vod,
        &catalog.series,
    );
    if written.is_err() && added {
        let _ = store.delete_source(id);
    }
    let counts = written?;
    log::info!(
        "iptv data: saved {} channels for source {}",
        counts.channels,
        id.0
    );
    Ok((id, counts))
}

/// Notes a failed refresh on the stored source (kind only).
fn record_failure(stored: Option<SourceId>, error: &ImportError) {
    let Some(id) = stored else {
        return;
    };
    let kind = stored::error_kind(error);
    if let Err(e) = open_store().and_then(|mut s| s.record_import_failure(id, kind)) {
        log::warn!("iptv data: could not record the failure: {e}");
    }
}

/// Keeps a picked playlist file in the app's files (a picked document is
/// only readable now; a refresh reads the copy). Returns its path.
pub fn keep_picked_file(name: &str, bytes: &[u8]) -> std::io::Result<String> {
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let name = name.trim_start_matches('.');
    let name = if name.is_empty() {
        "playlist.m3u"
    } else {
        name
    };
    let dir = data_dir().join("playlists").join(now_unix().to_string());
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    std::fs::write(&path, bytes)?;
    Ok(path.to_string_lossy().into_owned())
}
