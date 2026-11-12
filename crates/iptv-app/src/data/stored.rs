//! The store's side of an import: a stored source's catalog and guide,
//! shown before any network, and keeping the guide in the store under the
//! refresh policy (`iptv_import::decide`).
//!
//! A guide download streams into a stage file next to the store
//! (`super::download_guide`, `iptv_store_sqlite::EpgStage`) and replaces
//! the stored guide only once it loaded ([`replace`]); it is read back from
//! the store without descriptions (a few MiB for thousands of channels
//! instead of the parsed feed's hundreds); the guide's details read a
//! description on demand ([`description_lookup`]).
//!
//! The `epg.*` settings drive it (global values, `crate::settings`): the
//! refresh policy, how many days of finished programmes are kept (the
//! last of them held in memory, older ones paged in: `pages.rs`), and
//! whether descriptions are stored. The storage settings
//! travel with each guide's config, so changing them downloads the guide
//! again at the next check (the stored feeds no longer match).
//!
//! The guides to download are the playlist's guide sources (`guides.rs`,
//! kept in the store); the channels whose guide the user chose by hand
//! (`assign.rs`) are matched with those choices ([`overrides`]).
//!
//! The settings screen's guide commands act on the source on screen
//! ([`note_shown`]): [`update_now`] downloads its guide (every source
//! switched on), [`clear`] deletes the stored one, [`guide_status`] says
//! how the last update went and [`source_lines`] how each source did.

use std::cell::{Cell, RefCell};
use std::sync::mpsc::{self, Sender, TryRecvError};

use iptv_epg::{ChannelKey, DAY, GuideStore, Overrides, UnixTime};
use iptv_import::{
    CancelToken, Decision, EpgSourceConfig, ImportError, NoProgress, RefreshPolicy, Trigger,
    decide, matching_fingerprint, store_options,
};
use iptv_net::NetClient;
use iptv_providers::SecretUrl;
use iptv_providers::model::{Channel, ContentKind, Group};
use iptv_settings::{Settings, Target};
use iptv_store_sqlite::{
    ChannelOrder, ChannelQuery, EpgCounts, EpgFeedMeta, EpgWindow, FeedResult, GuideSource,
    ImportErrorKind, SourceId, Store, StoreError, StoredGuide,
};

use nsh_core::prelude::timer;

use super::{Downloaded, GENERATION, Imported, Msg, POLL, Streams, guides, open_store};
use crate::Ui;

thread_local! {
    /// The source whose guide is on screen.
    static SHOWN: Cell<Option<SourceId>> = const { Cell::new(None) };
    /// The sources whose update asked for by hand is running.
    static UPDATING: RefCell<Vec<SourceId>> = const { RefCell::new(Vec::new()) };
    /// Edits of guide sources since the app started: a check waits for
    /// the last one.
    static EDITS: Cell<u64> = const { Cell::new(0) };
}

/// How long a guide check waits after the guide sources changed (more
/// edits may follow).
const SETTLE: std::time::Duration = std::time::Duration::from_secs(2);

/// Days of finished programmes kept, for browsing back and catch-up
/// (`epg.keep_past`, whole days rounded up).
pub fn keep_past_days(settings: &Settings) -> u32 {
    store_options(settings, &Target::Global)
        .past_days_to_keep
        .unwrap_or(u32::MAX)
}

/// The guides' storage options from the settings (`epg.keep_past`,
/// `epg.store_descriptions`).
fn with_storage(mut configs: Vec<EpgSourceConfig>, settings: &Settings) -> Vec<EpgSourceConfig> {
    let store = store_options(settings, &Target::Global);
    for c in &mut configs {
        c.store = store;
    }
    configs
}

/// A stored source's live groups and channels, in provider order.
pub fn catalog(source: SourceId) -> Result<(Vec<Group>, Vec<Channel>), StoreError> {
    catalog_in(&open_store()?, source)
}

/// [`catalog`] from an open store.
pub fn catalog_in(
    store: &Store,
    source: SourceId,
) -> Result<(Vec<Group>, Vec<Channel>), StoreError> {
    let groups = store.groups(source, Some(ContentKind::Live))?;
    let query = ChannelQuery {
        include_hidden: true,
        order: ChannelOrder::Provider,
        ..ChannelQuery::default()
    };
    let channels = store
        .load_channels(source, &query)?
        .into_iter()
        .map(|row| row.channel)
        .collect();
    Ok((groups, channels))
}

/// What is stored about `source`'s guide.
pub fn guide_state(source: SourceId) -> Option<StoredGuide> {
    match open_store().and_then(|s| s.epg_guide(source)) {
        Ok(guide) => guide,
        Err(e) => {
            log::warn!("iptv data: stored guide state: {e}");
            None
        }
    }
}

/// The part of `source`'s stored guide a schedule shown at `now` holds
/// (`pages::window`: the last day on; older days are paged in), without
/// descriptions; `None` when nothing is stored.
pub fn load_guide(source: SourceId, now: UnixTime) -> Option<GuideStore> {
    let window = super::pages::window(now, keep_past_days(&crate::settings::get()));
    read_guide(source, &super::pages::store_window(window))
}

/// `window` of `source`'s stored guide; `None` when nothing is stored.
pub fn read_guide(source: SourceId, window: &EpgWindow<'_>) -> Option<GuideStore> {
    match open_store().and_then(|s| s.load_epg(source, window)) {
        Ok(guide) if guide.bound_count() > 0 => Some(guide),
        Ok(_) => None,
        Err(e) => {
            log::warn!("iptv data: stored guide: {e}");
            None
        }
    }
}

/// `source`'s guide sources in the user's order (empty when unreadable).
pub fn guide_sources(source: SourceId) -> Vec<GuideSource> {
    open_store()
        .and_then(|s| s.guide_sources(source))
        .unwrap_or_else(|e| {
            log::warn!("iptv data: guide sources: {e}");
            Vec::new()
        })
}

/// The download configs of `sources` (those switched on), with the storage
/// settings of now (a stored config has those it was fetched with).
pub fn configs(sources: &[GuideSource]) -> Vec<EpgSourceConfig> {
    let template = EpgSourceConfig::new(iptv_epg::SourceId(0), SecretUrl::new(""));
    let template = with_storage(vec![template], &crate::settings::get()).remove(0);
    guides::configs_of(sources, &template)
}

/// A stored source's guides without its playlist: its guide sources; for a
/// source whose guides were never arranged, the feeds stored with its
/// guide.
pub fn current_configs(source: SourceId, state: Option<&StoredGuide>) -> Vec<EpgSourceConfig> {
    let sources = guide_sources(source);
    if sources.is_empty() {
        let stored = state.map(StoredGuide::configs).unwrap_or_default();
        return with_storage(stored, &crate::settings::get());
    }
    configs(&sources)
}

/// The channels of `source` whose guide the user chose by hand.
pub fn overrides(source: SourceId) -> Overrides {
    match open_store().and_then(|s| s.epg_overrides(source)) {
        Ok(rows) => rows
            .into_iter()
            .map(|(id, epg)| (ChannelKey::new(id.as_str()), epg.to_override()))
            .collect(),
        Err(e) => {
            log::warn!("iptv data: guide choices: {e}");
            Overrides::default()
        }
    }
}

/// Whether to download the guide again (the `epg.update_*` settings).
pub fn plan(
    trigger: Trigger,
    configs: &[EpgSourceConfig],
    matching: Matching<'_>,
    stored: Option<&StoredGuide>,
    now: UnixTime,
) -> Decision {
    plan_with(
        &crate::settings::get(),
        trigger,
        configs,
        matching,
        stored,
        now,
    )
}

/// What EPG matching reads: the playlist's channels and the user's guide
/// choices.
#[derive(Clone, Copy)]
pub struct Matching<'a> {
    pub channels: &'a [Channel],
    pub overrides: &'a Overrides,
}

impl Matching<'_> {
    pub fn fingerprint(&self) -> u64 {
        matching_fingerprint(self.channels, self.overrides)
    }
}

fn plan_with(
    settings: &Settings,
    trigger: Trigger,
    configs: &[EpgSourceConfig],
    matching: Matching<'_>,
    stored: Option<&StoredGuide>,
    now: UnixTime,
) -> Decision {
    let state = stored.map(StoredGuide::refresh_state);
    let fingerprint = matching.fingerprint();
    decide(
        &RefreshPolicy::from_settings(settings, &Target::Global),
        trigger,
        configs,
        fingerprint,
        state.as_ref(),
        now,
    )
}

/// When to look at `source`'s guide again: when the policy will want it
/// (stale, running out, after a back-off), `None` when only a request or a
/// restart would.
pub fn next_check(
    source: SourceId,
    configs: &[EpgSourceConfig],
    matching: Matching<'_>,
    now: UnixTime,
) -> Option<UnixTime> {
    let stored = guide_state(source);
    check_at(
        plan(Trigger::Timer, configs, matching, stored.as_ref(), now),
        now,
    )
}

/// The time a guide check is due after `decision`.
fn check_at(decision: Decision, now: UnixTime) -> Option<UnixTime> {
    match decision {
        Decision::Keep { next_check } => next_check,
        Decision::Backoff { retry_at } => Some(retry_at),
        // Still due (the download was cancelled or not recorded): later.
        Decision::Refresh(_) => Some(now + RETRY_DUE),
        Decision::Discard => None,
    }
}

/// How long a check that is still due waits.
const RETRY_DUE: i64 = 3600;

/// Replaces `source`'s stored guide with `guide` (the feeds that loaded)
/// and drops programmes older than [`keep_past_days`]. A feed that failed
/// is recorded, so automatic refreshes back off; each feed's result is
/// kept for the guide sources screen.
pub fn replace(
    source: SourceId,
    guide: &mut Downloaded,
    configs: &[EpgSourceConfig],
    matching: Matching<'_>,
    now: UnixTime,
) -> Result<EpgCounts, StoreError> {
    let mut store = open_store()?;
    let import = &guide.import;
    let feeds = EpgFeedMeta::from_import(import, configs, now);
    let counts = store.replace_epg(
        source,
        &mut guide.stage,
        import,
        &feeds,
        matching.fingerprint(),
    )?;
    let results: Vec<_> = import
        .sources
        .iter()
        .map(|o| (o.id, o.result.as_ref().err().map(error_kind)))
        .collect();
    store.record_epg_feed_results(source, &results)?;
    let keep = keep_past_days(&crate::settings::get());
    let pruned = store.prune_epg(now.saturating_sub(i64::from(keep).saturating_mul(DAY)))?;
    if let Some(Err(e)) = import
        .sources
        .iter()
        .map(|o| &o.result)
        .find(|r| r.is_err())
    {
        store.record_epg_failure(source, error_kind(e))?;
    }
    log::info!(
        "iptv data: stored the guide of source {}: {} channels, {} programmes ({pruned} old ones pruned)",
        source.0,
        counts.bindings,
        counts.programmes
    );
    Ok(counts)
}

/// Notes a failed guide download (every source in `configs` failed); the
/// stored guide stays.
pub fn record_failure(source: SourceId, error: &ImportError, configs: &[EpgSourceConfig]) {
    let kind = error_kind(error);
    let results: Vec<_> = configs.iter().map(|c| (c.id, Some(kind))).collect();
    let recorded = open_store().and_then(|mut s| {
        s.record_epg_failure(source, kind)?;
        s.record_epg_feed_results(source, &results)
    });
    if let Err(e) = recorded {
        log::warn!("iptv data: could not record the guide failure: {e}");
    }
}

/// Deletes `source`'s stored guide (no guide is configured any more).
pub fn discard(source: SourceId) {
    match open_store().and_then(|mut s| s.delete_epg(source)) {
        Ok(true) => log::info!("iptv data: no guide configured; deleted the stored one"),
        Ok(false) => {}
        Err(e) => log::warn!("iptv data: could not delete the stored guide: {e}"),
    }
}

/// Reads descriptions of `source`'s stored guide on demand (on the UI
/// thread: one indexed row).
pub fn description_lookup(
    source: SourceId,
) -> Option<impl Fn(&ChannelKey, UnixTime) -> Option<String> + 'static> {
    let store = match open_store() {
        Ok(store) => store,
        Err(e) => {
            log::warn!("iptv data: descriptions unavailable: {e}");
            return None;
        }
    };
    Some(move |key: &ChannelKey, start| {
        store
            .epg_description(source, key, start)
            .unwrap_or_else(|e| {
                log::warn!("iptv data: description: {e}");
                None
            })
    })
}

/// `data.rs` put `source`'s schedule on screen.
pub fn note_shown(source: Option<SourceId>) {
    SHOWN.with(|s| s.set(source));
}

/// How the guide on screen was last updated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideStatus {
    /// No playlist on screen.
    NoSource,
    Updating,
    /// Nothing stored (not downloaded yet, or cleared).
    Empty,
    Updated {
        at: UnixTime,
        channels: u64,
    },
    Failed {
        at: UnixTime,
        kind: ImportErrorKind,
    },
}

/// The status of the guide on screen (one indexed read of the store).
pub fn guide_status() -> GuideStatus {
    let sources = shown_sources();
    if sources.is_empty() {
        return if open_store().is_ok_and(|s| s.all_playlists().unwrap_or(false)) {
            GuideStatus::Empty
        } else {
            GuideStatus::NoSource
        };
    }
    if sources.iter().any(|&s| updating(s)) {
        return GuideStatus::Updating;
    }
    let mut updated = None;
    let mut channels = 0;
    for source in sources {
        match status_of(guide_state(source).as_ref()) {
            failed @ GuideStatus::Failed { .. } => return failed,
            GuideStatus::Updated { at, channels: n } => {
                updated = Some(updated.map_or(at, |old: i64| old.min(at)));
                channels += n;
            }
            _ => {}
        }
    }
    updated.map_or(GuideStatus::Empty, |at| GuideStatus::Updated {
        at,
        channels,
    })
}

/// Individual mode has one guide owner; aggregate commands cover every
/// enabled source, with failures and feeds still managed per playlist.
pub fn shown_sources() -> Vec<SourceId> {
    if let Some(source) = SHOWN.with(Cell::get) {
        return vec![source];
    }
    let Ok(store) = open_store() else {
        return Vec::new();
    };
    if !store.all_playlists().unwrap_or(false) {
        return Vec::new();
    }
    store
        .sources()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| store.source_policy(s.id).is_ok_and(|p| p.enabled))
        .map(|s| s.id)
        .collect()
}

/// The source whose guide is on screen.
pub fn shown() -> Option<SourceId> {
    SHOWN.with(Cell::get)
}

/// An update of `source`'s guide asked for by hand is running.
pub fn updating(source: SourceId) -> bool {
    UPDATING.with(|u| u.borrow().contains(&source))
}

/// One guide source with how its last download went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLine {
    pub source: GuideSource,
    pub result: Option<FeedResult>,
}

/// `source`'s guide sources in order, each with its last result (one
/// short read of the store).
pub fn source_lines(source: SourceId) -> Vec<SourceLine> {
    let results = open_store()
        .and_then(|s| s.epg_feed_results(source))
        .unwrap_or_default();
    guide_sources(source)
        .into_iter()
        .map(|s| SourceLine {
            result: results.iter().find(|r| r.feed == s.feed).cloned(),
            source: s,
        })
        .collect()
}

pub fn status_of(stored: Option<&StoredGuide>) -> GuideStatus {
    let Some(g) = stored else {
        return GuideStatus::Empty;
    };
    match (g.last_error, g.attempted_at, g.replaced_at) {
        (Some(kind), Some(at), _) => GuideStatus::Failed { at, kind },
        (_, _, Some(at)) => GuideStatus::Updated {
            at,
            channels: g.bound_channels,
        },
        _ => GuideStatus::Empty,
    }
}

/// "Update the guide now" of the source on screen: see [`update_source`].
pub fn update_now(ui: Ui, done: impl FnOnce() + 'static) -> bool {
    let sources = shown_sources();
    if sources.is_empty() {
        return false;
    }
    let remaining = std::rc::Rc::new(Cell::new(sources.len()));
    let done = std::rc::Rc::new(RefCell::new(Some(done)));
    let mut started = false;
    for source in sources {
        let left = remaining.clone();
        let complete = done.clone();
        let finish = move || {
            left.set(left.get() - 1);
            if left.get() == 0 {
                if let Some(done) = complete.borrow_mut().take() {
                    done();
                }
            }
        };
        if updating(source) {
            remaining.set(remaining.get() - 1);
            started = true;
            continue;
        }
        if update_source(ui, source, finish) {
            started = true;
        } else {
            remaining.set(remaining.get() - 1);
        }
    }
    if started && remaining.get() == 0 {
        if let Some(done) = done.borrow_mut().take() {
            done();
        }
    }
    started
}

/// Downloads `source`'s guide now (every guide source switched on, with
/// the storage settings and guide choices of now); the guide on screen
/// changes when it is `source`'s. `done` runs on the UI thread when it
/// finished. False when there is no guide to download.
pub fn update_source(ui: Ui, source: SourceId, done: impl FnOnce() + 'static) -> bool {
    if updating(source) {
        return true;
    }
    let stored = guide_state(source);
    let guides = current_configs(source, stored.as_ref());
    if guides.is_empty() {
        return false;
    }
    // Without a stored guide the periodic checks stopped: start them again.
    let restart_checks = stored.is_none();
    UPDATING.with(|u| u.borrow_mut().push(source));
    let generation = GENERATION.with(Cell::get);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        super::with_runtime(|| download_now(source, guides, restart_checks, &tx));
    });
    super::spawn_in_app(async move {
        loop {
            match rx.try_recv() {
                Ok(Msg::Guide(imported))
                    if SHOWN.with(Cell::get) == Some(source)
                        || super::combined::includes(ui, source) =>
                {
                    super::show(ui, imported, generation, false)
                }
                Ok(Msg::NextCheck { source, at })
                    if SHOWN.with(Cell::get) == Some(source)
                        || super::combined::includes(ui, source) =>
                {
                    super::schedule_check(ui, source, at, generation);
                }
                Ok(_) => {}
                Err(TryRecvError::Empty) => timer(POLL).await,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        UPDATING.with(|u| u.borrow_mut().retain(|id| *id != source));
        ui.library.version.update(|v| *v += 1);
        done();
    });
    true
}

/// The download of [`update_source`], on its own thread.
async fn download_now(
    source: SourceId,
    guides: Vec<EpgSourceConfig>,
    restart_checks: bool,
    tx: &Sender<Msg>,
) {
    let now = super::now_unix();
    let (groups, channels) = match catalog(source) {
        Ok(catalog) => catalog,
        Err(e) => {
            log::warn!("iptv data: stored catalog: {e}");
            return;
        }
    };
    let overrides = overrides(source);
    let matching = Matching {
        channels: &channels,
        overrides: &overrides,
    };
    log::info!("iptv data: downloading the guide (asked for)");
    match NetClient::new(crate::settings::net_config()) {
        Ok(net) => {
            let cancel = CancelToken::new();
            let result =
                super::download_guide(&net, &guides, matching, now, &mut NoProgress, &cancel, None)
                    .await;
            match result {
                Ok(import) => {
                    let guide = super::keep_guide(source, import, &guides, matching, now);
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
                    record_failure(source, &e, &guides);
                }
            }
        }
        Err(e) => log::warn!("iptv data: network client setup failed: {e}"),
    }
    if restart_checks {
        super::send_next_check(tx, source, &guides, matching);
    }
}

/// `source`'s guide sources changed: its guide is checked once the edits
/// settle (the refresh policy downloads changed feeds). Only for the
/// source on screen; another one is checked when it is shown.
pub fn guides_changed(ui: Ui, source: SourceId) {
    if SHOWN.with(Cell::get) != Some(source) && !super::combined::includes(ui, source) {
        return;
    }
    let edit = EDITS.with(|e| {
        e.set(e.get() + 1);
        e.get()
    });
    let generation = GENERATION.with(Cell::get);
    super::spawn_in_app(async move {
        timer(SETTLE).await;
        let current = GENERATION.with(Cell::get) == generation && EDITS.with(Cell::get) == edit;
        if current && !updating(source) {
            log::info!("iptv data: guide sources changed; checking the guide");
            super::check_guide(ui, source, generation);
        }
    });
}

/// "Delete stored guide data": the source on screen keeps its channels
/// without programmes until its guide is downloaded again (at the next
/// start, refresh, or "Update the guide now"). The store works off the UI
/// thread; `done` hears whether the guide was deleted. False when no
/// playlist is on screen.
pub fn clear(ui: Ui, done: impl FnOnce(bool) + 'static) -> bool {
    let sources = shown_sources();
    if sources.is_empty() {
        return false;
    }
    let generation = GENERATION.with(Cell::get);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut deleted = true;
        let mut imports = Vec::new();
        for source in sources {
            if let Err(e) = open_store().and_then(|mut s| s.delete_epg(source)) {
                log::warn!("iptv data: could not delete stored guide: {e}");
                deleted = false;
                continue;
            }
            if let Ok((groups, channels)) = catalog(source) {
                imports.push(Box::new(Imported {
                    groups,
                    channels,
                    playback: Streams::Current,
                    guide: GuideStore::default(),
                    now: super::now_unix(),
                    source,
                    resume: false,
                }));
            }
        }
        let _ = tx.send((deleted, imports));
    });
    super::spawn_in_app(async move {
        let (deleted, imports) = loop {
            match rx.try_recv() {
                Ok(result) => break result,
                Err(TryRecvError::Empty) => timer(POLL).await,
                Err(TryRecvError::Disconnected) => return done(false),
            }
        };
        for imported in imports {
            super::show(ui, imported, generation, false);
        }
        done(deleted);
    });
    true
}

/// The stored kind of a failed import (kind only: no URL, no body).
pub fn error_kind(error: &ImportError) -> ImportErrorKind {
    match error {
        ImportError::InvalidConfig(_) => ImportErrorKind::InvalidConfig,
        ImportError::Cancelled => ImportErrorKind::Cancelled,
        ImportError::Network { .. } => ImportErrorKind::Network,
        ImportError::HttpStatus { .. } => ImportErrorKind::HttpStatus,
        ImportError::Auth { .. } => ImportErrorKind::AccountRejected,
        ImportError::Parse { .. } => ImportErrorKind::InvalidBody,
        ImportError::Portal { .. } | ImportError::Empty { .. } | ImportError::Storage(_) => {
            ImportErrorKind::Other
        }
    }
}

#[cfg(test)]
mod tests {
    use iptv_epg::StoreOptions;
    use iptv_import::RefreshReason;
    use iptv_settings::Value;
    use iptv_store_sqlite::StoredEpgFeed;

    use super::*;

    const NOW: UnixTime = 1_800_000_000;

    /// A playlist's guide sources: the provider's own `xmltv.php`, after a
    /// guide the user typed when there is one.
    fn sources(typed: bool) -> Vec<GuideSource> {
        let user = typed.then(|| {
            guides::user_source(
                guides::FIRST_USER_FEED,
                SecretUrl::new("http://typed.example.test/guide.xml".to_owned()),
                None,
            )
        });
        let provider = [SecretUrl::new(
            "http://guide.example.test/xmltv.php".to_owned(),
        )];
        guides::sync_provider(&user.into_iter().collect::<Vec<_>>(), &provider)
    }

    fn none() -> Overrides {
        Overrides::default()
    }

    fn nothing(overrides: &Overrides) -> Matching<'_> {
        Matching {
            channels: &[],
            overrides,
        }
    }

    /// What the store holds after a refresh of `feeds` an hour ago.
    fn stored(feeds: &[EpgSourceConfig]) -> StoredGuide {
        StoredGuide {
            replaced_at: Some(NOW - 3600),
            attempted_at: Some(NOW - 3600),
            last_error: None,
            match_fingerprint: Some(matching_fingerprint(&[], &Overrides::default())),
            bound_channels: 10,
            ends_at: Some(NOW + 3 * DAY),
            feeds: feeds
                .iter()
                .map(|config| StoredEpgFeed {
                    meta: EpgFeedMeta {
                        config: config.clone(),
                        fetched_at: NOW - 3600,
                        etag: None,
                        last_modified: None,
                        bytes: 1,
                    },
                    channel_count: 10,
                    programme_count: 100,
                })
                .collect(),
        }
    }

    #[test]
    fn a_fresh_stored_guide_is_kept_at_startup() {
        for typed in [false, true] {
            let guides = configs(&sources(typed));
            let stored = stored(&guides);
            let o = none();
            assert!(matches!(
                plan(
                    Trigger::Startup,
                    &configs(&sources(typed)),
                    nothing(&o),
                    Some(&stored),
                    NOW
                ),
                Decision::Keep { .. }
            ));
        }
    }

    #[test]
    fn a_v5_arrangement_keeps_its_stored_feeds() {
        // As a v5 build stored a typed guide (feed 1, priority 1000) and an
        // Xtream source's own (feed 2, priority 0).
        let c = configs(&sources(true));
        assert_eq!(
            c.iter().map(|c| (c.id.0, c.priority)).collect::<Vec<_>>(),
            [(1, 1000), (2, 0)]
        );
    }

    #[test]
    fn changed_guide_sources_download_the_guide_again() {
        let stored = stored(&configs(&sources(true)));
        let o = none();
        let edits = [
            guides::Edit::SetEnabled(guides::FIRST_USER_FEED, false),
            guides::Edit::Remove(guides::FIRST_USER_FEED),
            guides::Edit::Move(iptv_epg::SourceId(2), -1),
            guides::Edit::Shift(iptv_epg::SourceId(2), 60),
            guides::Edit::Add {
                url: SecretUrl::new("http://more.example.test/g.xml".to_owned()),
                name: None,
            },
        ];
        for edit in edits {
            let edited = guides::apply(&sources(true), &edit).unwrap();
            assert_eq!(
                plan(
                    Trigger::Timer,
                    &configs(&edited),
                    nothing(&o),
                    Some(&stored),
                    NOW
                ),
                Decision::Refresh(RefreshReason::FeedsChanged),
                "{edit:?}"
            );
        }
        // A new name changes no download.
        let renamed = guides::apply(
            &sources(true),
            &guides::Edit::Rename(guides::FIRST_USER_FEED, Some("Mine".into())),
        )
        .unwrap();
        assert!(matches!(
            plan(
                Trigger::Timer,
                &configs(&renamed),
                nothing(&o),
                Some(&stored),
                NOW
            ),
            Decision::Keep { .. }
        ));
    }

    #[test]
    fn a_guide_chosen_by_hand_is_matched_again() {
        let guides = configs(&sources(false));
        let stored = stored(&guides);
        let mut o = none();
        o.insert(ChannelKey::new("c1"), iptv_epg::Override::NoEpg);
        assert_eq!(
            plan(Trigger::Timer, &guides, nothing(&o), Some(&stored), NOW),
            Decision::Refresh(RefreshReason::ChannelsChanged)
        );
    }

    #[test]
    fn a_new_source_downloads_its_guide() {
        let o = none();
        assert_eq!(
            plan(
                Trigger::PlaylistChanged,
                &configs(&sources(false)),
                nothing(&o),
                None,
                NOW
            ),
            Decision::Refresh(RefreshReason::NeverFetched)
        );
    }

    #[test]
    fn the_next_check_is_when_the_guide_gets_stale_or_runs_short() {
        let guides = configs(&sources(false));
        let mut stored = stored(&guides);
        let o = none();
        let next = |s: &StoredGuide| {
            check_at(
                plan(Trigger::Timer, &guides, nothing(&o), Some(s), NOW),
                NOW,
            )
        };
        // Fetched an hour ago, programmes for 3 days: stale in 23 h.
        assert_eq!(next(&stored), Some(NOW - 3600 + DAY));
        // Programmes end in 20 h: 12 h before they run out.
        stored.ends_at = Some(NOW + 20 * 3600);
        assert_eq!(next(&stored), Some(NOW + 8 * 3600));
        // A failed attempt 10 min ago: retry an hour after it.
        stored.last_error = Some(ImportErrorKind::Network);
        stored.attempted_at = Some(NOW - 600);
        assert_eq!(next(&stored), Some(NOW + 3000));
        // No guide configured any more: no checks.
        assert_eq!(
            check_at(
                plan(Trigger::Timer, &[], nothing(&o), Some(&stored), NOW),
                NOW
            ),
            None
        );
    }

    #[test]
    fn the_refresh_follows_the_update_settings() {
        let guides = configs(&sources(false));
        let mut old = stored(&guides);
        old.replaced_at = Some(NOW - DAY);
        let fresh = stored(&guides);
        let o = none();
        let decide = |s: &Settings, trigger, stored: &StoredGuide| {
            plan_with(s, trigger, &guides, nothing(&o), Some(stored), NOW)
        };
        let mut s = Settings::new();
        assert!(matches!(
            decide(&s, Trigger::Startup, &old),
            Decision::Refresh(RefreshReason::Stale { .. })
        ));
        assert_eq!(
            decide(&s, Trigger::PlaylistChanged, &fresh),
            Decision::Refresh(RefreshReason::PlaylistChanged)
        );
        // Not at start: the next timer does it.
        s.set("epg.update_on_start", Value::Bool(false)).unwrap();
        assert!(matches!(
            decide(&s, Trigger::Startup, &old),
            Decision::Keep {
                next_check: Some(_)
            }
        ));
        assert!(matches!(
            decide(&s, Trigger::Timer, &old),
            Decision::Refresh(_)
        ));
        // Interval 0: only on request.
        s.set("epg.update_interval", Value::Duration(0)).unwrap();
        assert_eq!(
            decide(&s, Trigger::Timer, &old),
            Decision::Keep { next_check: None }
        );
        assert_eq!(
            decide(&s, Trigger::Manual, &old),
            Decision::Refresh(RefreshReason::Requested)
        );
        // A changed playlist no longer refreshes a fresh guide.
        s.set("epg.update_on_playlist_change", Value::Bool(false))
            .unwrap();
        assert_eq!(
            decide(&s, Trigger::PlaylistChanged, &fresh),
            Decision::Keep { next_check: None }
        );
    }

    #[test]
    fn storage_settings_travel_with_the_guides() {
        let mut s = Settings::new();
        assert_eq!(keep_past_days(&s), 7, "a week by default");
        s.set("epg.keep_past", Value::Duration(3 * DAY * 1000))
            .unwrap();
        s.set("epg.store_descriptions", Value::Bool(false)).unwrap();
        assert_eq!(keep_past_days(&s), 3);
        let guides = with_storage(configs(&sources(false)), &s);
        assert_eq!(
            guides[0].store,
            StoreOptions {
                store_descriptions: false,
                past_days_to_keep: Some(3)
            }
        );
        // A guide fetched with other storage settings is downloaded again.
        let fetched = stored(&configs(&sources(false)));
        let o = none();
        assert_eq!(
            plan_with(
                &s,
                Trigger::Startup,
                &guides,
                nothing(&o),
                Some(&fetched),
                NOW
            ),
            Decision::Refresh(RefreshReason::FeedsChanged)
        );
    }

    #[test]
    fn the_status_says_how_the_last_update_went() {
        let guides = configs(&sources(false));
        assert_eq!(status_of(None), GuideStatus::Empty);
        let mut g = stored(&guides);
        assert_eq!(
            status_of(Some(&g)),
            GuideStatus::Updated {
                at: NOW - 3600,
                channels: 10
            }
        );
        g.last_error = Some(ImportErrorKind::Network);
        g.attempted_at = Some(NOW - 60);
        assert_eq!(
            status_of(Some(&g)),
            GuideStatus::Failed {
                at: NOW - 60,
                kind: ImportErrorKind::Network
            }
        );
    }

    #[test]
    fn a_failed_imports_kind_is_stored_without_its_details() {
        use iptv_import::{AuthProblem, FetchError, FetchErrorKind, ParseProblem, Step};
        use iptv_providers::{StalkerError, StalkerErrorKind};
        let cases = [
            (
                ImportError::InvalidConfig("no link".into()),
                ImportErrorKind::InvalidConfig,
            ),
            (ImportError::Cancelled, ImportErrorKind::Cancelled),
            (
                ImportError::Network {
                    step: Step::Playlist,
                    error: FetchError::new(FetchErrorKind::Connect, "refused"),
                },
                ImportErrorKind::Network,
            ),
            (
                ImportError::HttpStatus {
                    step: Step::Playlist,
                    status: 500,
                },
                ImportErrorKind::HttpStatus,
            ),
            (
                ImportError::Auth {
                    step: Step::AccountInfo,
                    problem: AuthProblem::HttpStatus(401),
                },
                ImportErrorKind::AccountRejected,
            ),
            (
                ImportError::Parse {
                    step: Step::Playlist,
                    problem: ParseProblem::NotAPlaylist,
                },
                ImportErrorKind::InvalidBody,
            ),
            (
                ImportError::Portal {
                    step: Step::Authentication,
                    error: StalkerError::with_text(StalkerErrorKind::Portal, None),
                },
                ImportErrorKind::Other,
            ),
            (
                ImportError::Empty {
                    step: Step::Playlist,
                },
                ImportErrorKind::Other,
            ),
            (ImportError::Storage("disk".into()), ImportErrorKind::Other),
        ];
        for (error, kind) in cases {
            assert_eq!(error_kind(&error), kind, "{error:?}");
        }
    }

    #[test]
    fn a_day_old_guide_is_downloaded_again() {
        let guides = configs(&sources(false));
        let mut stored = stored(&guides);
        stored.replaced_at = Some(NOW - DAY);
        let o = none();
        assert!(matches!(
            plan(Trigger::Startup, &guides, nothing(&o), Some(&stored), NOW),
            Decision::Refresh(RefreshReason::Stale { .. })
        ));
    }
}
