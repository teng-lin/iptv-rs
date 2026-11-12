//! Aggregated mode owns one cached schedule per enabled playlist. A failed
//! refresh retains that playlist's last good schedule and cannot clear others.
use super::{Imported, Purpose, Streams, library, opening, organization, pages, stored};
use crate::Ui;
use iptv_guide::{
    EpgSchedule,
    combined::{CombinedSchedule, SourceSchedule, split_key},
};
use iptv_store_sqlite::SourceId;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};
thread_local! {
    static PARTS: RefCell<BTreeMap<SourceId,Rc<EpgSchedule>>> = RefCell::default();
    static LIMITS: RefCell<BTreeMap<SourceId,Option<u32>>> = RefCell::default();
    static RESUME: Cell<bool> = const { Cell::new(false) };
    static EPOCH: Cell<i64> = const { Cell::new(0) };
    static SINGLE: RefCell<Option<SingleSchedule>> = const { RefCell::new(None) };
}

/// The current single-source schedule can enter the combined catalogue without
/// dropping a portal session, account format, connection limit or loaded guide.
#[derive(Clone)]
pub(super) struct SingleSchedule {
    source: SourceId,
    schedule: Rc<EpgSchedule>,
    limit: Option<u32>,
    pending: Option<library::ImportCompletion>,
    revision: library::ImportRevision,
}
pub(super) fn reset_single() {
    SINGLE.with(|s| s.borrow_mut().take());
}
pub(super) fn single() -> Option<SingleSchedule> {
    SINGLE.with(|s| s.borrow().clone())
}
/// Snapshot before Add advances the generation. An unfinished account refresh
/// can still replace a cached source's provisional playback metadata, so that
/// source must go through the normal combined loader instead of being retained.
pub(super) fn ready_single() -> Option<SingleSchedule> {
    single()
        .filter(|s| !library::running(s.source) && s.schedule.playback().is_some())
        .map(|mut s| {
            s.pending = library::pending_import(s.source);
            s.revision = library::import_revision(s.source);
            s
        })
}
pub(super) fn remember_single(source: SourceId, schedule: Rc<EpgSchedule>) {
    SINGLE.with(|s| {
        *s.borrow_mut() = Some(SingleSchedule {
            source,
            schedule,
            limit: crate::multiview::account_limit(),
            pending: None,
            revision: library::import_revision(source),
        })
    });
}

/// Add already owns the current generation. Keep that generation so its guide
/// download and future guide checks continue after the browser joins sources.
pub(super) fn added(ui: Ui, source: SourceId, generation: u64, previous: Option<SingleSchedule>) {
    super::spawn_in_app(async move {
        if !crate::favorites::flush().await {
            crate::settings_view::notify("Recent changes could not be saved; try again");
            return;
        }
        if active(ui) || super::GENERATION.with(Cell::get) != generation || !has_multiple_enabled()
        {
            return;
        }
        // Guide messages may have arrived while writes flushed: take the latest
        // schedule, rather than the first channel-only Add result.
        let Some(added) = single().filter(|s| s.source == source) else {
            return;
        };
        if let Err(e) = super::open_store().and_then(|mut s| s.set_all_playlists(true)) {
            log::warn!("iptv combined: cannot save browsing mode: {e}");
            return;
        }
        let Ok(playlists) = library::playlists() else {
            return;
        };
        let enabled = |id| playlists.iter().any(|p| p.id == id && p.policy.enabled);
        super::source_changed(ui, None);
        stored::note_shown(None);
        ui.library.all.set(true);
        PARTS.with(|p| p.borrow_mut().clear());
        LIMITS.with(|p| p.borrow_mut().clear());
        let previous = previous.filter(|s| {
            enabled(s.source) && library::import_revision_is_current(s.source, &s.revision)
        });
        let previous_guide = previous.as_ref().map(|s| (s.source, s.pending.clone()));
        for kept in previous.into_iter().chain(std::iter::once(added)) {
            PARTS.with(|p| p.borrow_mut().insert(kept.source, kept.schedule));
            LIMITS.with(|p| p.borrow_mut().insert(kept.source, kept.limit));
        }
        reset_single();
        crate::multiview::set_account_streams(None);
        let now = super::now_unix();
        EPOCH.with(|e| e.set(now - now.rem_euclid(3600)));
        rebuild(ui);
        // The previous single source's timer belonged to its older generation.
        // Reattach guide work without fetching its playlist/account again.
        if let Some((previous, pending)) = previous_guide.filter(|(id, _)| *id != source) {
            reattach_guide(ui, previous, generation, pending);
        }
        for playlist in playlists.into_iter().filter(|p| p.policy.enabled) {
            if !PARTS.with(|p| p.borrow().contains_key(&playlist.id)) {
                if let Ok(Some(stored)) = super::open_store().and_then(|s| s.source(playlist.id)) {
                    library::run(ui, stored, Purpose::Combined, false);
                }
            }
        }
    });
}
/// A previous source can finish its original EPG download after Add has moved
/// the browser to a new generation. Keep the worker identity until its final
/// cache is restored; newer imports and explicit guide changes revoke it.
fn reattach_guide(
    ui: Ui,
    source: SourceId,
    generation: u64,
    pending: Option<library::ImportCompletion>,
) {
    let Some(pending) = pending else {
        super::restore_guide(ui, source, generation);
        return;
    };
    super::spawn_in_app(async move {
        let current = || {
            super::GENERATION.with(Cell::get) == generation
                && includes(ui, source)
                && library::import_is_current(source, &pending)
        };
        if !current() {
            return;
        }
        // Complete this initial restore before the final one so an older cache
        // read cannot arrive after the original download's successful result.
        super::read_guide(
            ui,
            source,
            generation,
            super::GuideRead::CacheOnly,
            Some(pending.clone()),
        )
        .await;
        while current() && !pending.finished() {
            nsh_core::prelude::timer(super::POLL).await;
        }
        if current() {
            super::read_guide(
                ui,
                source,
                generation,
                super::GuideRead::RestoreAndCheck,
                Some(pending),
            )
            .await;
        }
    });
}

pub fn active(ui: Ui) -> bool {
    ui.library.all.get_untracked()
}
pub fn includes(ui: Ui, id: SourceId) -> bool {
    active(ui) && library::policy(id).is_ok_and(|p| p.enabled)
}
pub fn owner(ui: Ui, key: &str) -> Option<(SourceId, String)> {
    if active(ui) {
        let (s, k) = split_key(key)?;
        Some((SourceId(s), k.to_owned()))
    } else {
        Some((ui.library.shown.get_untracked()?, key.to_owned()))
    }
}
/// The normal browser includes every enabled source once there is a choice.
pub fn has_multiple_enabled() -> bool {
    library::playlists()
        .is_ok_and(|sources| sources.iter().filter(|s| s.policy.enabled).take(2).count() == 2)
}

pub fn start(ui: Ui) {
    RESUME.with(|r| r.set(true));
    // Startup precedes user edits; install the empty real guide at once.
    switch_all_ready(ui);
}
pub fn switch_all(ui: Ui) {
    super::spawn_in_app(async move {
        if crate::favorites::flush().await {
            switch_all_ready(ui);
        } else {
            crate::settings_view::notify("Recent changes could not be saved; try again");
        }
    });
}
fn switch_all_ready(ui: Ui) {
    if active(ui) {
        return;
    }
    if let Err(e) = super::open_store().and_then(|mut s| s.set_all_playlists(true)) {
        log::warn!("iptv combined: cannot save browsing mode: {e}");
        return;
    }
    super::next_generation();
    reset_single();
    super::source_changed(ui, None);
    stored::note_shown(None);
    ui.library.all.set(true);
    PARTS.with(|p| p.borrow_mut().clear());
    LIMITS.with(|p| p.borrow_mut().clear());
    crate::multiview::set_account_streams(None);
    let now = super::now_unix();
    EPOCH.with(|e| e.set(now - now.rem_euclid(3600)));
    rebuild(ui);
    if let Ok(store) = super::open_store() {
        if let Ok(sources) = store.sources() {
            for source in sources {
                if store.source_policy(source.id).is_ok_and(|p| p.enabled) {
                    if let Ok(Some(source)) = store.source(source.id) {
                        library::run(ui, source, Purpose::Combined, false);
                    }
                }
            }
        }
    }
}
pub fn leave(ui: Ui) {
    reset_single();
    RESUME.with(|r| r.set(false));
    ui.library.all.set(false);
    PARTS.with(|p| p.borrow_mut().clear());
    LIMITS.with(|p| p.borrow_mut().clear());
    if let Err(e) = super::open_store().and_then(|mut s| s.set_all_playlists(false)) {
        log::warn!("iptv combined: cannot save browsing mode: {e}");
    }
}
pub(super) fn show(ui: Ui, imported: Box<Imported>, held: Option<opening::Held>) {
    if !includes(ui, imported.source) {
        return;
    }
    let source = imported.source;
    let can_resume = imported.resume;
    let mut schedule = EpgSchedule::from_catalog(
        &imported.groups,
        &imported.channels,
        imported.guide,
        imported.now,
    );
    organization::apply(&schedule, source);
    schedule = match imported.playback {
        Streams::New(context, allowed) => {
            LIMITS.with(|l| {
                let mut l = l.borrow_mut();
                if allowed.is_some() || !l.contains_key(&source) {
                    l.insert(source, allowed);
                }
            });
            schedule.with_playback(context)
        }
        Streams::Current => schedule.with_shared_playback(
            PARTS.with(|p| p.borrow().get(&source).and_then(|s| s.shared_playback())),
        ),
    };
    if let Some(lookup) = stored::description_lookup(source) {
        schedule = schedule.with_description_lookup(lookup);
    }
    schedule = match held {
        Some(h) => schedule.held_only(h.from, h.to).held_channels(h.keys),
        None => pages::paged(ui, schedule, source, imported.now),
    };
    PARTS.with(|p| p.borrow_mut().insert(source, Rc::new(schedule)));
    rebuild(ui);
    if can_resume && RESUME.with(Cell::get) {
        resume(ui);
    }
}
pub fn rebuild(ui: Ui) {
    if !active(ui) {
        return;
    }
    let Ok(playlists) = library::playlists() else {
        return;
    };
    let enabled: Vec<_> = playlists
        .iter()
        .filter(|p| p.policy.enabled)
        .map(|p| p.id)
        .collect();
    PARTS.with(|p| p.borrow_mut().retain(|id, _| enabled.contains(id)));
    let sources = PARTS.with(|p| {
        let parts = p.borrow();
        playlists
            .into_iter()
            .filter(|s| s.policy.enabled)
            .filter_map(|s| {
                Some(SourceSchedule {
                    id: s.id.0,
                    name: s.name,
                    schedule: parts.get(&s.id)?.clone(),
                })
            })
            .collect()
    });
    crate::favorites::set_combined();
    crate::set_schedule(
        &ui,
        Rc::new(
            CombinedSchedule::new(sources, EPOCH.with(Cell::get)).with_collapsed_sources(
                super::open_store()
                    .and_then(|s| s.collapsed_playlists())
                    .unwrap_or_default(),
            ),
        ),
    );
    pages::replaced();
}
pub fn connection_limit(source: SourceId) -> Option<u32> {
    LIMITS.with(|l| l.borrow().get(&source).copied().flatten())
}
pub fn organization_changed(ui: Ui, source: SourceId) {
    PARTS.with(|p| {
        if let Some(s) = p.borrow().get(&source) {
            organization::apply(s, source);
        }
    });
    rebuild(ui);
}
pub fn resume(ui: Ui) {
    if !crate::settings::flag(iptv_settings::keys::RESUME_LAST_CHANNEL)
        || ui.playing.get_untracked().is_some()
    {
        RESUME.with(|r| r.set(false));
        return;
    }
    let key = super::open_store().ok().and_then(|s| {
        s.combined_last_channel().ok().flatten().or_else(|| {
            // On first opening the combined browser, keep the previous
            // single-source startup preference, including legacy files.
            let source = s.active_source().ok().flatten()?;
            if !s.source_policy(source).ok()?.enabled {
                return None;
            }
            let key = crate::player::resume::last_key(source)?;
            Some(iptv_guide::combined::qualified_key(source.0, &key))
        })
    });
    let schedule = crate::schedule();
    if let Some(c) = key.and_then(|k| schedule.find_channel(&k)) {
        // Another playlist's import may call resume while the target portal is
        // still only cached rows. Wait for that owning source's authentication.
        if iptv_guide::combined::epg_channel(&*schedule, c)
            .is_none_or(|(child, _)| child.playback().is_none())
        {
            return;
        }
        RESUME.with(|r| r.set(false));
        ui.play(c, crate::Playback::Live);
        ui.mode.set(crate::ViewMode::Fullscreen);
        ui.show_playing();
    }
}

/// Toggle a parent row without changing the channel catalogue or player surface.
pub fn toggle_playlist(ui: Ui, group: usize) -> bool {
    let schedule = crate::schedule();
    let Some(all) = schedule
        .as_any()
        .and_then(|a| a.downcast_ref::<CombinedSchedule>())
    else {
        return false;
    };
    let Some(collapsed) = all.toggle_playlist(group) else {
        return false;
    };
    match super::open_store().and_then(|mut s| s.set_collapsed_playlists(&collapsed)) {
        Ok(()) => rebuild(ui),
        Err(e) => log::warn!("iptv browser: could not save expansion: {e}"),
    }
    true
}
