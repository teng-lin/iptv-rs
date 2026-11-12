//! The stored guide's older days, read in on demand.
//!
//! A shown schedule holds the stored guide from [`LOADED_PAST_DAYS`]
//! before its start on ([`window`]: the guide's first screen, a day of
//! "earlier programmes" and the play bar's archive of the last day need
//! nothing more), not every day `epg.keep_past` keeps: on Android a week
//! of a large guide was most of the app's memory. Older days are pages
//! (`iptv_guide::pages`): the first guide cell, programme detail or
//! catch-up that needs one asks [`loader`] for it, which reads it on its
//! own thread (`Store::load_epg` of that day) and puts it in the schedule
//! on screen; `Ui::guide_pages` then redraws what shows programmes.
//!
//! Pages go again when the guide's window is back at now ([`bind`]); the
//! day of the programme playing from the archive is pinned meanwhile, and
//! catch-up waits for its day before it resolves the archive ([`ready`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use iptv_epg::{DAY, GuideStore, UnixTime};
use iptv_guide::EpgSchedule;
use iptv_guide::pages::{PageRequest, PagedWindow};
use iptv_store_sqlite::{EpgWindow, SourceId};
use nsh_core::prelude::*;

use crate::guide::{Schedule, home_window};
use crate::{Playback, Ui};

/// Days of finished programmes held in memory with the guide (at most
/// what `epg.keep_past` keeps). Older ones are paged in.
pub const LOADED_PAST_DAYS: u32 = 1;

/// A page usually takes a few hundred milliseconds: look often.
const POLL: Duration = Duration::from_millis(30);

thread_local! {
    /// What waits for a page (catch-up of an older programme): run after
    /// every page that arrives, each checks again.
    static WAITERS: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
}

/// What a schedule shown at `now` holds of the stored guide: from
/// [`LOADED_PAST_DAYS`] (at most `keep_past_days`) before the hour of
/// `now` on; the store keeps nothing that ended before `keep_past_days`.
pub fn window(now: UnixTime, keep_past_days: u32) -> PagedWindow {
    let days = i64::from(LOADED_PAST_DAYS.min(keep_past_days));
    PagedWindow {
        loaded_from: now - now.rem_euclid(3600) - days * DAY,
        oldest: now.saturating_sub(i64::from(keep_past_days).saturating_mul(DAY)),
    }
}

/// The store read of [`window`]'s held part.
pub fn store_window(window: PagedWindow) -> EpgWindow<'static> {
    EpgWindow {
        from: window.loaded_from,
        ..EpgWindow::from(0, 0)
    }
}

/// [`window`] with its oldest time at the first programme stored (`None`:
/// none is), when that is later than what the store would keep: the guide
/// can be browsed back to it and no further (`EpgSchedule::browsable`).
pub fn kept_from(window: PagedWindow, first: Option<UnixTime>, now: UnixTime) -> PagedWindow {
    PagedWindow {
        oldest: window.oldest.max(first.unwrap_or(now)),
        ..window
    }
}

/// `schedule` (source `source`'s stored guide, as [`window`] at `now`
/// read it) with its older days paged in on demand, back to the first
/// programme stored.
pub fn paged(ui: Ui, schedule: EpgSchedule, source: SourceId, now: UnixTime) -> EpgSchedule {
    let keep = super::stored::keep_past_days(&crate::settings::get());
    let first = super::open_store()
        .and_then(|s| s.epg_first_start(source))
        .unwrap_or_else(|e| {
            log::warn!("iptv data: the stored guide's first day: {e}");
            None
        });
    let window = kept_from(window(now, keep), first, now);
    schedule.with_pages(window, loader(ui, source))
}

/// Reads a page on its own thread and puts it in the schedule on screen.
/// Called from guide queries (maybe inside a view's computation): it only
/// starts work.
fn loader(ui: Ui, source: SourceId) -> impl Fn(PageRequest) + 'static {
    move |request: PageRequest| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let window = EpgWindow {
                from: request.from,
                to: request.to,
                ..EpgWindow::from(0, 0)
            };
            let started = std::time::Instant::now();
            let read = super::open_store().and_then(|s| s.load_epg(source, &window));
            let _ = tx.send((read, started.elapsed()));
        });
        super::spawn_in_app(async move {
            let (read, took) = loop {
                match rx.try_recv() {
                    Ok(result) => break result,
                    Err(TryRecvError::Empty) => timer(POLL).await,
                    Err(TryRecvError::Disconnected) => return,
                }
            };
            let store = read.unwrap_or_else(|e| {
                log::warn!("iptv data: older guide day: {e}");
                GuideStore::default()
            });
            arrived(ui, source, request, store, took);
        });
    }
}

fn arrived(ui: Ui, source: SourceId, request: PageRequest, store: GuideStore, took: Duration) {
    let schedule = crate::schedule();
    let child = combined(&*schedule)
        .and_then(|s| s.source(source.0))
        .or_else(|| {
            (ui.library.shown.get_untracked() == Some(source))
                .then(|| epg(&*schedule))
                .flatten()
        });
    let inserted = child.is_some_and(|s| {
        let kb = store.heap_bytes() / 1024;
        let inserted = s.insert_page(request, store);
        if inserted {
            log::info!(
                "iptv data: guide day -{} paged in ({kb} KiB, read in {} ms); {} held",
                request.page + 1,
                took.as_millis(),
                s.paged_days()
            );
        }
        inserted
    });
    if inserted {
        ui.guide_pages.update(|v| *v += 1);
    }
    for waiter in WAITERS.with(|w| std::mem::take(&mut *w.borrow_mut())) {
        waiter();
    }
}

fn epg(schedule: &dyn Schedule) -> Option<&EpgSchedule> {
    schedule.as_any()?.downcast_ref::<EpgSchedule>()
}

fn combined(schedule: &dyn Schedule) -> Option<&iptv_guide::combined::CombinedSchedule> {
    schedule.as_any()?.downcast_ref()
}
fn each(schedule: &dyn Schedule, mut f: impl FnMut(&EpgSchedule)) {
    if let Some(s) = epg(schedule) {
        f(s);
    }
    if let Some(s) = combined(schedule) {
        for p in s.sources() {
            f(&p.schedule);
        }
    }
}
/// A schedule replaced the one on screen (with the whole window after a
/// first screen, `opening.rs`): what waited for its programmes looks again.
pub fn replaced() {
    for waiter in WAITERS.with(|w| std::mem::take(&mut *w.borrow_mut())) {
        waiter();
    }
}

/// Asks for the day holding guide `minute` before the grid shows it (where
/// a fling of the timeline lands).
pub fn want(schedule: &dyn Schedule, minute: i32) {
    let unix = iptv_core::guide::unix_of(schedule, minute);
    each(schedule, |s| {
        s.ensure_loaded(unix);
    });
}

/// Whether the programmes around `t` are in memory on `schedule`, so a
/// catch-up there resolves with its programme's real times; when not, its
/// day is pinned and asked for, and `retry` runs once a page arrived.
pub fn ready(
    schedule: &Rc<dyn Schedule>,
    channel: usize,
    t: UnixTime,
    retry: impl FnOnce() + 'static,
) -> bool {
    let Some((epg, _)) = iptv_guide::combined::epg_channel(&**schedule, channel) else {
        return true;
    };
    epg.pin_page(Some(t));
    if epg.ensure_loaded(t) {
        return true;
    }
    log::info!("iptv data: catch-up waits for its guide day");
    WAITERS.with(|w| w.borrow_mut().push(Box::new(retry)));
    false
}

/// Keeps the archive playing's day held, and drops the other pages once
/// the guide's window is back at now.
pub fn bind(ui: Ui) {
    effect(move || {
        ui.data_version.get();
        let archive = match (ui.playing.get(), ui.playback.get()) {
            (Some(channel), Playback::CatchUp { start, .. }) => Some((channel, start)),
            _ => None,
        };
        untracked(|| {
            let schedule = crate::schedule();
            each(&*schedule, |s| s.pin_page(None));
            if let Some((channel, minute)) = archive
                && let Some((epg, _)) = iptv_guide::combined::epg_channel(&*schedule, channel)
            {
                epg.pin_page(Some(iptv_core::guide::unix_of(&*schedule, minute)));
            }
        });
    });
    let away = Cell::new(false);
    effect(move || {
        ui.data_version.get();
        let home = ui.focus.with(|f| f.window == home_window(f.now));
        if home && away.replace(false) {
            untracked(|| {
                let schedule = crate::schedule();
                each(&*schedule, |s| {
                    let dropped = s.evict_pages();
                    if dropped > 0 {
                        log::info!("iptv data: back at now; dropped {dropped} older guide days");
                    }
                });
            });
        }
        away.set(!home);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: UnixTime = 1_800_000_000 + 1234;

    #[test]
    fn catchup_waits_only_for_its_own_source_page() {
        use iptv_guide::combined::{CombinedSchedule, SourceSchedule};
        let parsed = iptv_providers::m3u::parse_m3u(
            "#EXTM3U\n#EXTINF:-1,Generated\nhttp://stream.example.test/generated\n",
        );
        let requests_a = Rc::new(RefCell::new(Vec::new()));
        let requests_b = Rc::new(RefCell::new(Vec::new()));
        let make = |requests: Rc<RefCell<Vec<PageRequest>>>| {
            Rc::new(
                EpgSchedule::from_catalog(
                    &parsed.groups,
                    &parsed.channels.items,
                    GuideStore::default(),
                    NOW,
                )
                .with_pages(window(NOW, 7), move |request| {
                    requests.borrow_mut().push(request)
                }),
            )
        };
        let a = make(requests_a.clone());
        let b = make(requests_b.clone());
        let schedule: Rc<dyn Schedule> = Rc::new(CombinedSchedule::new(
            vec![
                SourceSchedule {
                    id: 1,
                    name: "Generated A".into(),
                    schedule: a.clone(),
                },
                SourceSchedule {
                    id: 2,
                    name: "Generated B".into(),
                    schedule: b.clone(),
                },
            ],
            NOW,
        ));
        let time = NOW - 2 * DAY;
        assert!(!ready(&schedule, 0, time, || {}));
        assert_eq!(requests_a.borrow().len(), 1);
        assert!(requests_b.borrow().is_empty());
        // B remains pending indefinitely. Completing A must still unblock A.
        assert!(!b.ensure_loaded(time));
        let request = requests_a.borrow()[0];
        assert!(a.insert_page(request, GuideStore::default()));
        assert!(ready(&schedule, 0, time, || panic!("A is already loaded")));
        assert!(!b.ensure_loaded(time));
        WAITERS.with(|waiters| waiters.borrow_mut().clear());
    }

    #[test]
    fn the_window_holds_a_day_before_the_hour() {
        let w = window(NOW, 7);
        assert_eq!(w.loaded_from, 1_800_000_000 - DAY);
        assert_eq!(w.oldest, NOW - 7 * DAY);
        assert_eq!(store_window(w).from, w.loaded_from);
        assert_eq!(store_window(w).to, UnixTime::MAX);
        // Keeping less than a day holds no more than is kept.
        assert_eq!(window(NOW, 0).loaded_from, 1_800_000_000);
        // Keeping everything (`u32::MAX`): no oldest day.
        assert!(window(NOW, u32::MAX).oldest < NOW - 1000 * DAY);
    }

    #[test]
    fn the_guide_goes_back_to_its_first_programme_stored() {
        let w = window(NOW, 7);
        // Two days stored of the seven kept: back to the first programme.
        let first = NOW - 2 * DAY + 600;
        assert_eq!(kept_from(w, Some(first), NOW).oldest, first);
        // Older programmes than the store keeps (not pruned yet): no
        // further back than it keeps.
        assert_eq!(kept_from(w, Some(NOW - 9 * DAY), NOW).oldest, w.oldest);
        // Keeping everything: the first programme bounds it.
        let all = window(NOW, u32::MAX);
        assert_eq!(kept_from(all, Some(first), NOW).oldest, first);
        // Nothing stored: no past at all.
        let none = kept_from(w, None, NOW);
        assert_eq!((none.oldest, none.loaded_from), (NOW, w.loaded_from));
    }
}
