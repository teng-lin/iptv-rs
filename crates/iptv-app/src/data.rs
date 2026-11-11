//! Real data: imports a source and its EPG off the UI thread, keeps it in
//! the SQLite store, and swaps the guide to the imported schedule.
//!
//! At startup the stored source shown last (the active playlist, kept in
//! the store) is refreshed, else the newest; with none, the add-playlist
//! wizard opens on its empty state (`wizard_view`). The wizard adds sources
//! through [`start_import`]. A new source is stored only once its import
//! succeeded, so a failed or cancelled attempt leaves nothing behind. The
//! playlists screen switches, refreshes, renames and deletes stored
//! sources through [`library`].
//!
//! A stored source's saved channels and guide are on screen before its
//! refresh reaches the network, and stay when the refresh fails. Its guide
//! is downloaded again only when the refresh policy says so (`stored`).
//! Only the source on screen has guide timers: each shown import starts a
//! new generation, and timers of an older one stop.

pub(crate) mod assign;
pub(crate) mod guides;
mod library;
mod opening;
pub(crate) mod organization;
pub(crate) mod pages;
pub(crate) mod stored;
mod wizard_map;

use std::cell::Cell;
use std::collections::BTreeSet;
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Sender, TryRecvError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iptv_core::wizard::{
    FailureKind, Field, ImportCounts, ImportFailure, ImportProgress, ImportStage,
};
use iptv_epg::GuideStore;
use iptv_guide::EpgSchedule;
use iptv_guide::playback::PlaybackContext;
use iptv_import::{
    CancelToken, Catalog, Decision, EpgImport, EpgSourceConfig, ImportError, ImportOptions,
    NoProgress, Progress, ProgressSink, Trigger, import_epg, import_source,
};
use iptv_net::NetClient;
use iptv_providers::SecretUrl;
use iptv_providers::model::{Channel, Group, Source};
use iptv_store_sqlite::{
    CatalogCounts, EpgStage, GuideSource, ImportErrorKind, SourceId, Store, StoreError,
};
use nsh_core::prelude::*;

pub use library::{
    Activity, Library, Playlist, delete, edit_guides, playlists, refresh, refresh_all, rename,
    set_policy, set_user_agent, source_details, switch_to, try_details,
};
pub use wizard_map::source_from_draft;

use crate::Ui;

/// A development playlist the wizard's M3U form starts with, set when the
/// app is built (`IPTV_RS_DEV_PLAYLIST=http://10.0.2.2:8089/iptv/playlist.m3u`
/// for the mock server, `tools/mock-server.py`, from an emulator). Without
/// it, as in every release build, the link field starts empty.
pub const DEV_PLAYLIST: Option<&str> = option_env!("IPTV_RS_DEV_PLAYLIST");

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
    pub purpose: Purpose,
    /// `source` holds new details (link, login, User-Agent) for the stored
    /// source: they replace the stored ones once they imported; a failure
    /// leaves the stored source and its data as they were.
    pub edit: bool,
}

/// Why a source is imported, which decides what reaches the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// A new source from the wizard: shown once it imported.
    Add,
    /// The stored source opening at startup: its stored channels and guide
    /// at once, then refreshed; its last channel may start.
    Startup,
    /// The user chose another stored source: as at startup, but nothing
    /// starts playing.
    Switch,
    /// "Refresh now" of the source on screen: the refreshed data replaces
    /// what is shown.
    Refresh,
    /// "Refresh now" of another stored source: kept in the store only; the
    /// screen and its guide timers stay as they are.
    Background,
}

impl Purpose {
    fn shows(self) -> bool {
        self != Self::Background
    }

    /// What the store has is shown before the network.
    fn shows_stored_first(self) -> bool {
        matches!(self, Self::Startup | Self::Switch)
    }

    /// The guide's refresh trigger: opening uses the stored guide while it
    /// is fresh; an added or refreshed playlist downloads it again.
    fn trigger(self) -> Trigger {
        match self {
            Self::Startup | Self::Switch => Trigger::Startup,
            Self::Add | Self::Refresh | Self::Background => Trigger::PlaylistChanged,
        }
    }
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
    source: SourceId,
    /// Whether the last channel may start (its stream can be resolved).
    resume: bool,
}

/// Where a new schedule gets its playback context.
enum Streams {
    /// Its own, with the streams the account allows at once (when it says).
    New(Option<PlaybackContext>, Option<u32>),
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
    /// What the store has, with only the first screen's programmes
    /// (`opening.rs`); a `Guide` follows.
    FirstScreen(Box<Imported>, opening::Held),
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

/// Where the app keeps its files: the Android app's files directory,
/// `~/Library/Application Support/iptv-rs` on macOS and in the iOS app's
/// container, or
/// `~/.local/share/iptv-rs` on the other desktops.
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
        if cfg!(any(target_os = "macos", target_os = "ios")) {
            home.join("Library/Application Support/iptv-rs")
        } else {
            home.join(".local/share/iptv-rs")
        }
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

/// Startup: refreshes the active (else the newest) stored source, or
/// opens the wizard.
pub fn start(ui: Ui) {
    APP_OWNER.with(|o| o.set(Some(Owner::current())));
    // Guide downloads a crash interrupted (none runs yet).
    EpgStage::remove_stale(data_dir());
    let started = std::time::Instant::now();
    let (store, source) = match open_store() {
        Ok(store) => {
            let source = library::startup_source(&store);
            (Some(store), source)
        }
        Err(e) => (None, Err(e)),
    };
    let ms = started.elapsed().as_millis();
    crate::startup::mark_with("store opened", format_args!("{ms} ms"));
    match source {
        Ok(Some(stored)) => {
            if let Some(store) = &store {
                opening::begin(ui, store, &stored);
            }
            drop(store);
            library::run(ui, stored, Purpose::Startup, false);
        }
        Ok(None) => {
            if playlists().is_ok_and(|p| !p.is_empty()) {
                show_nothing(ui);
                crate::playlists_view::open(ui);
            } else {
                crate::wizard_view::open_first_run(ui);
            }
        }
        Err(e) => {
            log::warn!("iptv data: store unavailable ({e}); opening the wizard");
            crate::wizard_view::open_first_run(ui);
        }
    }
    library::start_scheduler(ui);
}

/// A new generation owns the guide: older imports no longer show and
/// their guide timers stop.
fn next_generation() -> u64 {
    GENERATION.with(|g| {
        g.set(g.get() + 1);
        g.get()
    })
}

/// Imports `job` on its own thread. `on_update` hears progress (coalesced
/// per poll) and then one [`Update::Done`]; on success (unless the job is
/// a [`Purpose::Background`] refresh) the guide switches to the source at
/// once and again when its guide data arrives. Cancel with the returned
/// token.
pub fn start_import(ui: Ui, job: Job, on_update: impl Fn(Update) + 'static) -> CancelToken {
    let cancel = CancelToken::new();
    start_import_with(ui, job, cancel.clone(), on_update);
    cancel
}

/// [`start_import`] with the caller's cancel token.
fn start_import_with(ui: Ui, job: Job, cancel: CancelToken, on_update: impl Fn(Update) + 'static) {
    let purpose = job.purpose;
    let shows = purpose.shows();
    let generation = if shows {
        next_generation()
    } else {
        GENERATION.with(Cell::get)
    };
    // Only the startup refresh may start the last channel.
    let resume = purpose == Purpose::Startup;
    let (tx, rx) = mpsc::channel();
    {
        let cancel = cancel.clone();
        std::thread::spawn(move || run(job, &tx, &cancel));
    }
    let show = move |imported: Box<Imported>, held: Option<opening::Held>| {
        if shows {
            show_part(ui, imported, generation, resume, held);
        }
    };
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
                            show(imported, None);
                            counts
                        })));
                    }
                    Ok(Msg::Guide(imported)) => show(imported, None),
                    Ok(Msg::FirstScreen(imported, held)) => show(imported, Some(held)),
                    Ok(Msg::NextCheck { source, at }) if shows => {
                        schedule_check(ui, source, at, generation);
                    }
                    Ok(Msg::NextCheck { .. }) => {}
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
}

/// Puts `imported` on screen unless a newer import owns the guide.
fn show(ui: Ui, imported: Box<Imported>, generation: u64, resume: bool) {
    show_part(ui, imported, generation, resume, None);
}

/// [`show`]; with `held`, its guide holds only those programmes (a first
/// screen, `opening.rs`).
fn show_part(
    ui: Ui,
    imported: Box<Imported>,
    generation: u64,
    resume: bool,
    held: Option<opening::Held>,
) {
    if GENERATION.with(Cell::get) != generation {
        return;
    }
    if Some(imported.source) != ui.library.shown.get_untracked() {
        source_changed(ui, Some(imported.source));
    }
    stored::note_shown(Some(imported.source));
    let channels = imported.channels.len();
    let started = std::time::Instant::now();
    let schedule = EpgSchedule::from_catalog(
        &imported.groups,
        &imported.channels,
        imported.guide,
        imported.now,
    );
    organization::apply(&schedule, imported.source);
    let built = started.elapsed();
    let mut schedule = match imported.playback {
        Streams::New(context, allowed) => {
            crate::multiview::set_account_streams(allowed);
            schedule.with_playback(context)
        }
        Streams::Current => {
            let current = crate::schedule();
            let shared = current
                .as_any()
                .and_then(|s| s.downcast_ref::<EpgSchedule>())
                .and_then(EpgSchedule::shared_playback);
            schedule.with_shared_playback(shared)
        }
    };
    if let Some(lookup) = stored::description_lookup(imported.source) {
        schedule = schedule.with_description_lookup(lookup);
    }
    // Its guide is the stored one's recent part (`stored::load_guide`), or
    // only its first screen until that arrives.
    schedule = match held {
        Some(held) => schedule
            .held_only(held.from, held.to)
            .held_channels(held.keys),
        None => pages::paged(ui, schedule, imported.source, imported.now),
    };
    let wrapped = started.elapsed();
    log::info!("iptv data: guide switched to {channels} imported channels");
    crate::favorites::set_source(Some(imported.source));
    let favorites = started.elapsed();
    crate::set_schedule(&ui, Rc::new(schedule));
    pages::replaced();
    crate::startup::shown(&[
        ("schedule", built),
        ("pages", wrapped),
        ("favourites", favorites),
        ("swapped", started.elapsed()),
    ]);
    crate::player::source_ready(ui, resume && imported.resume);
}

/// Another stored source (or none) is about to be shown: what played and
/// the favourites belong to the old one, and the choice is remembered.
fn source_changed(ui: Ui, source: Option<SourceId>) {
    log::info!("iptv data: showing source {:?}", source.map(|id| id.0));
    crate::player::source_changed(ui);
    if ui.favorites.with_untracked(|f| !f.is_empty()) {
        ui.favorites.set(BTreeSet::new());
    }
    ui.library.shown.set(source);
    if let Some(id) = source {
        library::remember_shown(id);
    }
}

/// No stored source is left: the guide shows nothing real and no timers
/// run (the wizard's first-run state goes over it).
fn show_nothing(ui: Ui) {
    next_generation();
    source_changed(ui, None);
    crate::set_schedule(
        &ui,
        Rc::new(EpgSchedule::new(
            Vec::new(),
            GuideStore::default(),
            now_unix(),
        )),
    );
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
    let state = stored::guide_state(source);
    let guides = stored::current_configs(source, state.as_ref());
    let (groups, channels) = match stored::catalog(source) {
        Ok(catalog) => catalog,
        Err(e) => {
            log::warn!("iptv data: stored catalog: {e}");
            return;
        }
    };
    let overrides = stored::overrides(source);
    let matching = stored::Matching {
        channels: &channels,
        overrides: &overrides,
    };
    match stored::plan(Trigger::Timer, &guides, matching, state.as_ref(), now) {
        Decision::Refresh(reason) => {
            log::info!("iptv data: downloading the guide ({reason:?})");
            match NetClient::new(crate::settings::net_config()) {
                Ok(net) => {
                    let cancel = CancelToken::new();
                    let result = download_guide(
                        &net,
                        &guides,
                        matching,
                        now,
                        &mut NoProgress,
                        &cancel,
                        None,
                    )
                    .await;
                    match result {
                        Ok(import) => {
                            let guide = keep_guide(source, import, &guides, matching, now);
                            let _ = tx.send(Msg::Guide(Box::new(Imported {
                                groups,
                                channels: channels.clone(),
                                playback: Streams::Current,
                                guide,
                                now,
                                source,
                                resume: false,
                            })));
                        }
                        Err(e) => {
                            log::warn!("iptv data: guide: {e}");
                            stored::record_failure(source, &e, &guides);
                        }
                    }
                }
                Err(e) => log::warn!("iptv data: network client setup failed: {e}"),
            }
        }
        Decision::Discard => {
            stored::discard(source);
            // Its programmes went: the guide on screen shows none.
            let _ = tx.send(Msg::Guide(Box::new(Imported {
                groups,
                channels: channels.clone(),
                playback: Streams::Current,
                guide: GuideStore::default(),
                now,
                source,
                resume: false,
            })));
        }
        Decision::Keep { .. } | Decision::Backoff { .. } => {}
    }
    send_next_check(tx, source, &guides, matching);
}

/// Tells the UI when to look at the guide again, if ever.
fn send_next_check(
    tx: &Sender<Msg>,
    source: SourceId,
    guides: &[EpgSourceConfig],
    matching: stored::Matching<'_>,
) {
    if let Some(at) = stored::next_check(source, guides, matching, now_unix()) {
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
    // Opening a stored source: what the store has is on screen before any
    // network.
    if job.purpose.shows_stored_first()
        && let Some(id) = job.stored
    {
        opening::send_stored(&job, id, now, tx);
    }
    if let Some(id) = job
        .stored
        .filter(|id| !library::network_due(*id, job.purpose))
    {
        if let Ok((groups, channels)) = stored::catalog(id) {
            if !channels.is_empty() {
                let counts = ImportCounts {
                    channels: channels.len() as u32,
                    groups: groups.len() as u32,
                    movies: 0,
                    series: 0,
                };
                let guide = stored::load_guide(id, now).unwrap_or_default();
                let context = crate::player::stored_context(&job.source.config);
                let _ = tx.send(Msg::Playlist(Ok((
                    counts,
                    Box::new(Imported {
                        groups: groups.clone(),
                        channels: channels.clone(),
                        guide,
                        now,
                        source: id,
                        resume: context.is_some(),
                        playback: Streams::New(context, None),
                    }),
                ))));
                let overrides = stored::overrides(id);
                let matching = stored::Matching {
                    channels: &channels,
                    overrides: &overrides,
                };
                let guides = stored::current_configs(id, stored::guide_state(id).as_ref());
                let state = stored::guide_state(id);
                match stored::plan(
                    job.purpose.trigger(),
                    &guides,
                    matching,
                    state.as_ref(),
                    now,
                ) {
                    Decision::Refresh(_) => {
                        if let Ok(net) = NetClient::new(crate::settings::net_config()) {
                            match download_guide(
                                &net,
                                &guides,
                                matching,
                                now,
                                &mut NoProgress,
                                cancel,
                                None,
                            )
                            .await
                            {
                                Ok(import) => {
                                    let guide = keep_guide(id, import, &guides, matching, now);
                                    let _ = tx.send(Msg::Guide(Box::new(Imported {
                                        groups,
                                        channels: channels.clone(),
                                        guide,
                                        now,
                                        source: id,
                                        resume: false,
                                        playback: Streams::Current,
                                    })));
                                }
                                Err(ImportError::Cancelled) => {}
                                Err(e) => stored::record_failure(id, &e, &guides),
                            }
                        }
                    }
                    Decision::Discard => {
                        stored::discard(id);
                        let _ = tx.send(Msg::Guide(Box::new(Imported {
                            groups,
                            channels: channels.clone(),
                            guide: GuideStore::default(),
                            now,
                            source: id,
                            resume: false,
                            playback: Streams::Current,
                        })));
                    }
                    _ => {}
                }
                send_next_check(tx, id, &guides, matching);
                return;
            }
        }
    }
    let stored_guide = job.stored.and_then(stored::guide_state);
    let Ok(net) = NetClient::new(crate::settings::net_config()) else {
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
                // New details that failed say nothing about the stored ones.
                if !matches!(e, ImportError::Cancelled) && !job.edit {
                    record_failure(job.stored, stored::error_kind(&e));
                }
                // The stored guide stays on screen: keep it current.
                if let (Some(id), Some(state)) = (job.stored, stored_guide.as_ref())
                    && let Ok((_, channels)) = stored::catalog(id)
                {
                    let overrides = stored::overrides(id);
                    let matching = stored::Matching {
                        channels: &channels,
                        overrides: &overrides,
                    };
                    let guides = stored::current_configs(id, Some(state));
                    send_next_check(tx, id, &guides, matching);
                }
                return fail(wizard_map::failure_of(&e));
            }
        };
    if catalog.is_empty() {
        if !job.edit {
            record_failure(job.stored, ImportErrorKind::Other);
        }
        return fail(ImportFailure::new(FailureKind::EmptyPlaylist));
    }
    // The guide sources: the stored source's as the user arranged them (a
    // new source: the guide just typed), with the provider's guides of now.
    let planned = guides::sync_provider(&planned_guides(&job), &catalog.guide_urls());
    let overrides = job.stored.map(stored::overrides).unwrap_or_default();
    let matching = stored::Matching {
        channels: &catalog.channels,
        overrides: &overrides,
    };
    // A guide the user typed is part of adding the source: it must load
    // before the source is kept, and its failure points at its field.
    let typed_guide = match job.epg_url {
        Some(_) => {
            match download_guide(
                &net,
                &stored::configs(&planned),
                matching,
                now,
                &mut progress,
                cancel,
                Some(guides::FIRST_USER_FEED),
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
    let (id, counts, sources) = match save(&job, &catalog, &planned) {
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
            playback: Streams::New(
                crate::player::context_for(&job.source.config, &catalog.source),
                crate::multiview::account_streams(&catalog.source),
            ),
            guide,
            now,
            source: id,
            resume: true,
        })
    };
    let report = |guide| {
        let _ = tx.send(Msg::Playlist(Ok((counts, imported(guide)))));
    };
    let guides = stored::configs(&sources);
    if let Some(import) = typed_guide {
        report(keep_guide(id, import, &guides, matching, now));
        return send_next_check(tx, id, &guides, matching);
    }
    match stored::plan(
        job.purpose.trigger(),
        &guides,
        matching,
        stored_guide.as_ref(),
        now,
    ) {
        Decision::Refresh(reason) => {
            log::info!("iptv data: downloading the guide ({reason:?})");
            report(stored::load_guide(id, now).unwrap_or_default());
            // The guide loads after the channels are on screen.
            match download_guide(&net, &guides, matching, now, &mut NoProgress, cancel, None).await
            {
                Ok(import) => {
                    let guide = keep_guide(id, import, &guides, matching, now);
                    let _ = tx.send(Msg::Guide(imported(guide)));
                }
                Err(ImportError::Cancelled) => {}
                Err(e) => {
                    log::warn!("iptv data: guide: {e}");
                    stored::record_failure(id, &e, &guides);
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
    send_next_check(tx, id, &guides, matching);
}

/// The guide sources an import starts from: the stored source's, or the
/// guide typed for a new source.
fn planned_guides(job: &Job) -> Vec<GuideSource> {
    match (job.stored, &job.epg_url) {
        (Some(id), _) => stored::guide_sources(id),
        (None, Some(url)) => vec![guides::user_source(
            guides::FIRST_USER_FEED,
            url.clone(),
            None,
        )],
        (None, None) => Vec::new(),
    }
}

/// A downloaded guide not stored yet: how it matched, and its programmes
/// staged in a file next to the store (deleted when this is dropped, so a
/// guide that is never kept leaves the stored one as it was).
pub(crate) struct Downloaded {
    pub import: EpgImport,
    pub stage: EpgStage,
}

/// Stores a downloaded guide and reads it back without descriptions (the
/// stored one, if any, when the store fails).
fn keep_guide(
    id: SourceId,
    mut guide: Downloaded,
    guides: &[EpgSourceConfig],
    matching: stored::Matching<'_>,
    now: i64,
) -> GuideStore {
    if let Err(e) = stored::replace(id, &mut guide, guides, matching, now) {
        log::warn!("iptv data: could not store the guide: {e}");
    }
    drop(guide);
    stored::load_guide(id, now).unwrap_or_default()
}

/// Downloads `guides` for the catalog's channels with the user's guide
/// choices, streaming the programmes to a stage file (memory stays small
/// whatever the guide's size). The guide `must_load` (the one the user
/// just typed), when given, must load.
async fn download_guide<P: ProgressSink>(
    net: &NetClient,
    guides: &[EpgSourceConfig],
    matching: stored::Matching<'_>,
    now: i64,
    progress: &mut P,
    cancel: &CancelToken,
    must_load: Option<iptv_epg::SourceId>,
) -> Result<Downloaded, ImportError> {
    let mut stage =
        EpgStage::create_in(data_dir()).map_err(|e| ImportError::Storage(e.to_string()))?;
    let epg = import_epg(
        net,
        guides,
        matching.channels,
        matching.overrides,
        now,
        &mut stage,
        progress,
        cancel,
    )
    .await?;
    if let Some(Err(e)) = must_load
        .and_then(|id| epg.sources.iter().find(|o| o.id == id))
        .map(|o| &o.result)
    {
        return Err(e.clone());
    }
    Ok(Downloaded { import: epg, stage })
}

/// Stores the catalog: under a new source (removed again if the catalog
/// cannot be written) or the refreshed one (with its new details when they
/// were edited), and the guide sources with the provider's guides of now
/// (`planned` for a new source; a stored one's are read again, as the user
/// may have changed them meanwhile). Returns the guide sources saved.
fn save(
    job: &Job,
    catalog: &Catalog,
    planned: &[GuideSource],
) -> Result<(SourceId, CatalogCounts, Vec<GuideSource>), StoreError> {
    let mut store = open_store()?;
    let (id, added) = match job.stored {
        Some(id) => (id, false),
        None => (store.add_source(&job.source)?, true),
    };
    if let (Some(id), true) = (job.stored, job.edit) {
        store.update_source(id, &job.source)?;
        log::info!("iptv data: saved the new details of source {}", id.0);
    }
    let sources = if added {
        planned.to_vec()
    } else {
        guides::sync_provider(&store.guide_sources(id)?, &catalog.guide_urls())
    };
    let written = store.set_guide_sources(id, &sources).and_then(|()| {
        store.replace_catalog(
            id,
            &catalog.groups,
            &catalog.channels,
            &catalog.vod,
            &catalog.series,
        )
    });
    if written.is_err() && added {
        let _ = store.delete_source(id);
    }
    let counts = written?;
    log::info!(
        "iptv data: saved {} channels for source {}",
        counts.channels,
        id.0
    );
    Ok((id, counts, sources))
}

/// Notes a failed refresh on the stored source (kind only).
fn record_failure(stored: Option<SourceId>, kind: ImportErrorKind) {
    let Some(id) = stored else {
        return;
    };
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
