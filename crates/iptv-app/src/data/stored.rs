//! The store's side of an import: a stored source's catalog and guide,
//! shown before any network, and keeping the guide in the store under the
//! refresh policy (`iptv_import::decide`).
//!
//! The guide is read back from the store without descriptions (a few MiB
//! for thousands of channels instead of the parsed feed's hundreds); the
//! guide's details read a description on demand ([`description_lookup`]).
//!
//! The `epg.*` settings drive it (global values, `crate::settings`): the
//! refresh policy, how many days of finished programmes are kept and
//! loaded, and whether descriptions are stored. The storage settings
//! travel with each guide's config, so changing them downloads the guide
//! again at the next check (the stored feeds no longer match).
//!
//! The settings screen's guide commands act on the source on screen
//! ([`note_shown`]): [`update_now`] downloads its guide, [`clear`] deletes
//! the stored one, [`guide_status`] says how the last update went.

use std::cell::{Cell, RefCell};
use std::sync::mpsc::{self, Sender, TryRecvError};

use iptv_epg::{ChannelKey, DAY, GuideStore, Overrides, UnixTime};
use iptv_import::{
    CancelToken, Catalog, Decision, EpgImport, EpgSourceConfig, ImportError, NoProgress,
    RefreshPolicy, Trigger, decide, matching_fingerprint, store_options,
};
use iptv_net::NetClient;
use iptv_providers::SecretUrl;
use iptv_providers::model::{Channel, ContentKind, Group};
use iptv_settings::{Settings, Target};
use iptv_store_sqlite::{
    ChannelOrder, ChannelQuery, EpgCounts, EpgFeedMeta, EpgWindow, ImportErrorKind, SourceId,
    StoreError, StoredGuide,
};

use nsh_core::prelude::timer;

use super::{CUSTOM_EPG, GENERATION, Imported, Msg, POLL, Streams, open_store};
use crate::Ui;

thread_local! {
    /// The source whose guide is on screen.
    static SHOWN: Cell<Option<SourceId>> = const { Cell::new(None) };
    /// An update asked for by hand is running.
    static UPDATING: Cell<bool> = const { Cell::new(false) };
    /// The guides of sources whose stored guide was cleared, for an update
    /// asked for before the next refresh of the source.
    static CLEARED: RefCell<Vec<(SourceId, Vec<EpgSourceConfig>)>> = const { RefCell::new(Vec::new()) };
}

/// Days of finished programmes kept and loaded, for browsing back and
/// catch-up (`epg.keep_past`, whole days rounded up).
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
    let store = open_store()?;
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

/// `source`'s stored guide from [`keep_past_days`] before `now` on,
/// without descriptions; `None` when nothing is stored.
pub fn load_guide(source: SourceId, now: UnixTime) -> Option<GuideStore> {
    let window = EpgWindow::from(now, keep_past_days(&crate::settings::get()));
    match open_store().and_then(|s| s.load_epg(source, &window)) {
        Ok(guide) if guide.bound_count() > 0 => Some(guide),
        Ok(_) => None,
        Err(e) => {
            log::warn!("iptv data: stored guide: {e}");
            None
        }
    }
}

/// The source's own guides (from its catalog).
pub fn own_guides(catalog: &Catalog) -> Vec<EpgSourceConfig> {
    with_storage(
        catalog.epg_sources(CUSTOM_EPG.0 + 1),
        &crate::settings::get(),
    )
}

/// The guides to load: a typed one first (the one just typed, else the one
/// stored with the source's guide), then the source's `own`.
pub fn configs(
    mut configs: Vec<EpgSourceConfig>,
    typed: Option<SecretUrl>,
    stored: Option<&StoredGuide>,
) -> Vec<EpgSourceConfig> {
    let typed = match typed {
        Some(url) => Some(custom(url)),
        None => stored
            .into_iter()
            .flat_map(StoredGuide::configs)
            .find(|c| c.id == CUSTOM_EPG),
    };
    if let Some(typed) = typed {
        configs.insert(0, typed);
    }
    // With the storage settings of now (a stored config has those it was
    // fetched with).
    with_storage(configs, &crate::settings::get())
}

/// The config of a guide the user typed: before the source's own.
pub fn custom(url: SecretUrl) -> EpgSourceConfig {
    let config = EpgSourceConfig::new(CUSTOM_EPG, url).with_priority(1000);
    with_storage(vec![config], &crate::settings::get()).remove(0)
}

/// Whether to download the guide again (the `epg.update_*` settings).
pub fn plan(
    trigger: Trigger,
    configs: &[EpgSourceConfig],
    channels: &[Channel],
    stored: Option<&StoredGuide>,
    now: UnixTime,
) -> Decision {
    plan_with(
        &crate::settings::get(),
        trigger,
        configs,
        channels,
        stored,
        now,
    )
}

fn plan_with(
    settings: &Settings,
    trigger: Trigger,
    configs: &[EpgSourceConfig],
    channels: &[Channel],
    stored: Option<&StoredGuide>,
    now: UnixTime,
) -> Decision {
    let state = stored.map(StoredGuide::refresh_state);
    let fingerprint = matching_fingerprint(channels, &Overrides::default());
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
    channels: &[Channel],
    now: UnixTime,
) -> Option<UnixTime> {
    let stored = guide_state(source);
    check_at(
        plan(Trigger::Timer, configs, channels, stored.as_ref(), now),
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

/// Replaces `source`'s stored guide with `import` (the feeds that loaded)
/// and drops programmes older than [`keep_past_days`]. A feed that failed
/// is recorded, so automatic refreshes back off.
pub fn replace(
    source: SourceId,
    import: &EpgImport,
    configs: &[EpgSourceConfig],
    channels: &[Channel],
    now: UnixTime,
) -> Result<EpgCounts, StoreError> {
    let mut store = open_store()?;
    let feeds = EpgFeedMeta::from_import(import, configs, now);
    let fingerprint = matching_fingerprint(channels, &Overrides::default());
    let counts = store.replace_epg(source, &import.guide, &feeds, fingerprint)?;
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

/// Notes a failed guide download; the stored guide stays.
pub fn record_failure(source: SourceId, error: &ImportError) {
    if let Err(e) = open_store().and_then(|mut s| s.record_epg_failure(source, error_kind(error))) {
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
    if UPDATING.with(Cell::get) {
        return GuideStatus::Updating;
    }
    let Some(source) = SHOWN.with(Cell::get) else {
        return GuideStatus::NoSource;
    };
    status_of(guide_state(source).as_ref())
}

fn status_of(stored: Option<&StoredGuide>) -> GuideStatus {
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

/// "Update the guide now": downloads the guide of the source on screen
/// (its stored feeds, with the storage settings of now) and shows it;
/// `done` runs on the UI thread when it finished. False when there is no
/// guide to download.
pub fn update_now(ui: Ui, done: impl FnOnce() + 'static) -> bool {
    let Some(source) = SHOWN.with(Cell::get) else {
        return false;
    };
    if UPDATING.with(Cell::get) {
        return true;
    }
    let stored = guide_state(source);
    let configs = stored.as_ref().map(StoredGuide::configs).or_else(|| {
        CLEARED.with(|c| {
            c.borrow()
                .iter()
                .find(|(id, _)| *id == source)
                .map(|(_, configs)| configs.clone())
        })
    });
    let Some(configs) = configs.filter(|c| !c.is_empty()) else {
        return false;
    };
    let guides = with_storage(configs, &crate::settings::get());
    // Without a stored guide the periodic checks stopped: start them again.
    let restart_checks = stored.is_none();
    UPDATING.with(|u| u.set(true));
    let generation = GENERATION.with(Cell::get);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        super::with_runtime(|| download_now(source, guides, restart_checks, &tx));
    });
    super::spawn_in_app(async move {
        loop {
            match rx.try_recv() {
                Ok(Msg::Guide(imported)) => super::show(ui, imported, generation, false),
                Ok(Msg::NextCheck { source, at }) => {
                    super::schedule_check(ui, source, at, generation);
                }
                Ok(_) => {}
                Err(TryRecvError::Empty) => timer(POLL).await,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        UPDATING.with(|u| u.set(false));
        done();
    });
    true
}

/// The download of [`update_now`], on its own thread.
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
    log::info!("iptv data: downloading the guide (asked for)");
    match NetClient::new(crate::settings::net_config()) {
        Ok(net) => {
            let cancel = CancelToken::new();
            let result = super::download_guide(
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
                    let guide = super::keep_guide(source, import, &guides, &channels, now);
                    CLEARED.with(|c| c.borrow_mut().retain(|(id, _)| *id != source));
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
                    record_failure(source, &e);
                }
            }
        }
        Err(e) => log::warn!("iptv data: network client setup failed: {e}"),
    }
    if restart_checks {
        super::send_next_check(tx, source, &guides, &channels);
    }
}

/// "Delete stored guide data": the source on screen keeps its channels
/// without programmes until its guide is downloaded again (at the next
/// start, refresh, or "Update the guide now"). The store works off the UI
/// thread; `done` hears whether the guide was deleted. False when no
/// playlist is on screen.
pub fn clear(ui: Ui, done: impl FnOnce(bool) + 'static) -> bool {
    let Some(source) = SHOWN.with(Cell::get) else {
        return false;
    };
    let generation = GENERATION.with(Cell::get);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let configs = guide_state(source).map(|g| g.configs());
        let deleted = match open_store().and_then(|mut s| s.delete_epg(source)) {
            Ok(_) => true,
            Err(e) => {
                log::warn!("iptv data: could not delete the stored guide: {e}");
                false
            }
        };
        let catalog = catalog(source).inspect_err(|e| log::warn!("iptv data: stored catalog: {e}"));
        let _ = tx.send((configs, deleted, catalog.ok()));
    });
    super::spawn_in_app(async move {
        let (configs, deleted, catalog) = loop {
            match rx.try_recv() {
                Ok(result) => break result,
                Err(TryRecvError::Empty) => timer(POLL).await,
                Err(TryRecvError::Disconnected) => return done(false),
            }
        };
        if let Some(configs) = configs.filter(|_| deleted) {
            log::info!("iptv data: deleted the stored guide");
            CLEARED.with(|c| {
                let mut c = c.borrow_mut();
                c.retain(|(id, _)| *id != source);
                c.push((source, configs));
            });
        }
        if let (true, Some((groups, channels))) = (deleted, catalog) {
            let imported = Box::new(Imported {
                groups,
                channels,
                playback: Streams::Current,
                guide: GuideStore::default(),
                now: super::now_unix(),
                source: Some(source),
                resume: false,
            });
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
        ImportError::Portal { .. } | ImportError::Empty { .. } => ImportErrorKind::Other,
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

    fn own() -> Vec<EpgSourceConfig> {
        vec![EpgSourceConfig::new(
            iptv_epg::SourceId(CUSTOM_EPG.0 + 1),
            SecretUrl::new("http://guide.example.test/xmltv.php".to_owned()),
        )]
    }

    fn typed() -> SecretUrl {
        SecretUrl::new("http://typed.example.test/guide.xml".to_owned())
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
        let guides = configs(own(), None, None);
        let stored = stored(&guides);
        let again = configs(own(), None, Some(&stored));
        assert_eq!(again, guides);
        assert!(matches!(
            plan(Trigger::Startup, &again, &[], Some(&stored), NOW),
            Decision::Keep { .. }
        ));
    }

    #[test]
    fn a_typed_guide_stays_with_the_stored_guide() {
        let guides = configs(own(), Some(typed()), None);
        assert_eq!(guides[0], custom(typed()));
        assert_eq!(guides.len(), 2);
        let stored = stored(&guides);
        // The typed address is not kept with the source, only with its guide.
        let again = configs(own(), None, Some(&stored));
        assert_eq!(again, guides);
        assert!(matches!(
            plan(Trigger::Startup, &again, &[], Some(&stored), NOW),
            Decision::Keep { .. }
        ));
    }

    #[test]
    fn a_new_source_downloads_its_guide() {
        assert_eq!(
            plan(Trigger::PlaylistChanged, &own(), &[], None, NOW),
            Decision::Refresh(RefreshReason::NeverFetched)
        );
    }

    #[test]
    fn the_next_check_is_when_the_guide_gets_stale_or_runs_short() {
        let guides = configs(own(), None, None);
        let mut stored = stored(&guides);
        let next =
            |s: &StoredGuide| check_at(plan(Trigger::Timer, &guides, &[], Some(s), NOW), NOW);
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
            check_at(plan(Trigger::Timer, &[], &[], Some(&stored), NOW), NOW),
            None
        );
    }

    #[test]
    fn the_refresh_follows_the_update_settings() {
        let guides = configs(own(), None, None);
        let mut old = stored(&guides);
        old.replaced_at = Some(NOW - DAY);
        let fresh = stored(&guides);
        let decide = |s: &Settings, trigger, stored: &StoredGuide| {
            plan_with(s, trigger, &guides, &[], Some(stored), NOW)
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
        let guides = with_storage(own(), &s);
        assert_eq!(
            guides[0].store,
            StoreOptions {
                store_descriptions: false,
                past_days_to_keep: Some(3)
            }
        );
        // A guide fetched with other storage settings is downloaded again.
        let fetched = stored(&configs(own(), None, None));
        assert_eq!(
            plan_with(&s, Trigger::Startup, &guides, &[], Some(&fetched), NOW),
            Decision::Refresh(RefreshReason::FeedsChanged)
        );
    }

    #[test]
    fn the_status_says_how_the_last_update_went() {
        let guides = configs(own(), None, None);
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
    fn a_day_old_guide_is_downloaded_again() {
        let guides = configs(own(), None, None);
        let mut stored = stored(&guides);
        stored.replaced_at = Some(NOW - DAY);
        assert!(matches!(
            plan(Trigger::Startup, &guides, &[], Some(&stored), NOW),
            Decision::Refresh(RefreshReason::Stale { .. })
        ));
    }
}
