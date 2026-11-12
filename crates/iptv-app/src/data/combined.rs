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
        Rc::new(CombinedSchedule::new(sources, EPOCH.with(Cell::get))),
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
    let key = super::open_store()
        .ok()
        .and_then(|s| s.combined_last_channel().ok().flatten());
    if let Some(c) = key.and_then(|k| crate::schedule().find_channel(&k)) {
        RESUME.with(|r| r.set(false));
        ui.play(c, crate::Playback::Live);
        ui.mode.set(crate::ViewMode::Fullscreen);
        ui.show_playing();
    }
}
