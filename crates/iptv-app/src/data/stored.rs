//! The store's side of an import: a stored source's catalog and guide,
//! shown before any network, and keeping the guide in the store under the
//! refresh policy (`iptv_import::decide`).
//!
//! The guide is read back from the store without descriptions (a few MiB
//! for thousands of channels instead of the parsed feed's hundreds); the
//! guide's details read a description on demand ([`description_lookup`]).

use iptv_epg::{ChannelKey, DAY, GuideStore, Overrides, UnixTime};
use iptv_import::{
    Catalog, Decision, EpgImport, EpgSourceConfig, ImportError, RefreshPolicy, Trigger, decide,
    matching_fingerprint,
};
use iptv_providers::SecretUrl;
use iptv_providers::model::{Channel, ContentKind, Group};
use iptv_store_sqlite::{
    ChannelOrder, ChannelQuery, EpgCounts, EpgFeedMeta, EpgWindow, ImportErrorKind, SourceId,
    StoreError, StoredGuide,
};

use super::{CUSTOM_EPG, open_store};

/// `epg.keep_past`'s default: finished programmes kept and loaded for
/// browsing back and catch-up (the settings are not wired in yet).
pub const KEEP_PAST_DAYS: u32 = 2;

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

/// `source`'s stored guide from [`KEEP_PAST_DAYS`] before `now` on,
/// without descriptions; `None` when nothing is stored.
pub fn load_guide(source: SourceId, now: UnixTime) -> Option<GuideStore> {
    let window = EpgWindow::from(now, KEEP_PAST_DAYS);
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
    catalog.epg_sources(CUSTOM_EPG.0 + 1)
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
    configs
}

/// The config of a guide the user typed: before the source's own.
pub fn custom(url: SecretUrl) -> EpgSourceConfig {
    EpgSourceConfig::new(CUSTOM_EPG, url).with_priority(1000)
}

/// Whether to download the guide again (the settings' default policy).
pub fn plan(
    trigger: Trigger,
    configs: &[EpgSourceConfig],
    channels: &[Channel],
    stored: Option<&StoredGuide>,
    now: UnixTime,
) -> Decision {
    let state = stored.map(StoredGuide::refresh_state);
    let fingerprint = matching_fingerprint(channels, &Overrides::default());
    decide(
        &RefreshPolicy::default(),
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
/// and drops programmes older than [`KEEP_PAST_DAYS`]. A feed that failed
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
    let pruned = store.prune_epg(now - i64::from(KEEP_PAST_DAYS) * DAY)?;
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
    use iptv_import::RefreshReason;
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
