//! The search screen: a query field, recent searches, and results in two
//! tabs (channels, programmes). State and rules live in
//! `iptv_search::SearchState`; this module builds the indexes, runs the
//! searches (debounced), maps remote keys and taps to search actions and
//! carries out what the state answers (play, show details, close).
//!
//! Choosing a result plays it full screen and keeps the screen suspended;
//! leaving full screen brings it back with its query and focus
//! (`SearchState::resume`). Wording is iptv-rs's own.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iptv_core::guide::{Airing, Schedule};
use iptv_core::menu::Origin;
use iptv_epg::UnixTime;
use iptv_guide::EpgSchedule;
use iptv_nav::Action;
use iptv_search::{
    ChannelIndex, ChannelSearchOptions, Focus, Phase, ProgrammeHit, ProgrammeIndex,
    ProgrammeSearchOptions, ResultId, SearchAction, SearchEffect, SearchHistory, SearchState, Tab,
};
use nsh_base::input;
use nsh_core::prelude::*;

use crate::theme::{FOCUS_ALPHA, RADIUS, RESTING_ALPHA, TEXT_SECONDARY_ALPHA};
use crate::{Playback, Ui, ViewMode, channel_name, clock, tint};

// Defaults of the `search.*` settings (iptv-settings), until settings reach the app.
const FAVOURITES_FIRST: bool = true;
const INCLUDE_PAST_WITHOUT_CATCHUP: bool = false;
/// Typing pause before a search runs.
const DEBOUNCE: Duration = Duration::from_millis(200);
const LIMIT: usize = 200;
const HISTORY_FILE: &str = "search-history.json";
const WHITE: u32 = 0xFFFFFF;
const ROW_H: f32 = 56.0;

/// Whether programme search can run on the current guide data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Programmes {
    /// No EPG behind the schedule (the synthetic guide).
    Unavailable,
    Building,
    Ready,
}

/// The open search screen (`Ui::search`).
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub state: SearchState,
    /// A result is playing full screen; the screen returns when it ends.
    pub suspended: bool,
    pub programmes: Programmes,
}

#[derive(Default)]
struct Indexes {
    /// The `data_version` they were built for.
    version: Option<u64>,
    channels: ChannelIndex,
    programmes: Option<ProgrammeIndex>,
}

thread_local! {
    static INDEXES: RefCell<Indexes> = RefCell::default();
    /// Bumped per search request: a debounced search runs only if no newer one came.
    static PENDING: Cell<u64> = const { Cell::new(0) };
}

// --- Glue (pure, unit-tested) ------------------------------------------------

/// What choosing a result does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    /// Channels and programmes on now: the channel, live.
    Live(usize),
    /// A past programme still in the archive: replay from its start.
    CatchUp {
        channel: usize,
        start: UnixTime,
    },
    /// Upcoming programmes (and past ones without replay): their details.
    Details {
        channel: usize,
        start: UnixTime,
    },
    Nothing,
}

fn outcome(id: ResultId, hit: Option<&ProgrammeHit>) -> Outcome {
    match (id, hit) {
        (ResultId::Channel(c), _) => Outcome::Live(c),
        (ResultId::Programme { .. }, None) => Outcome::Nothing,
        (ResultId::Programme { channel, start }, Some(h)) => match h.airing {
            Airing::Live => Outcome::Live(channel),
            Airing::Ended if h.catchup => Outcome::CatchUp { channel, start },
            Airing::Ended | Airing::Upcoming => Outcome::Details { channel, start },
        },
    }
}

/// The remote's action on the search screen.
fn key_action(state: &SearchState, action: Action) -> Option<SearchAction> {
    Some(match action {
        Action::MoveUp => SearchAction::Up,
        Action::MoveDown => SearchAction::Down,
        Action::MoveLeft => SearchAction::Left,
        Action::MoveRight => SearchAction::Right,
        // OK on the field (keyboard down) brings the keyboard back; with
        // the keyboard up it submits.
        Action::Select
            if *state.focus() == Focus::Input
                && (state.phase() == Phase::Idle || !state.keyboard_visible()) =>
        {
            SearchAction::ShowKeyboard
        }
        Action::Select => SearchAction::Ok,
        Action::Back => SearchAction::Back,
        // Number keys type into the query (channel numbers are searchable).
        Action::Digit(d) => SearchAction::Type(format!("{}{d}", state.query())),
        _ => return None,
    })
}

/// The guide minute (relative to `now`) inside a programme starting at
/// `start`: rounded up, so it never falls in the programme before.
fn guide_minute(start: UnixTime, now: UnixTime) -> i32 {
    (start - now + 59).div_euclid(60) as i32
}

/// The guide minute a time is displayed at (as the guide rounds).
fn display_minute(t: UnixTime, now: UnixTime) -> i32 {
    (t - now).div_euclid(60) as i32
}

/// The airing and replay marker of a programme result.
fn badge(hit: &ProgrammeHit) -> String {
    let when = match hit.airing {
        Airing::Live => "On now",
        Airing::Upcoming => "Later",
        Airing::Ended => "Earlier",
    };
    match (hit.airing, hit.catchup) {
        (_, true) => format!("{when} · ↺ replay"),
        (Airing::Ended, false) => format!("{when} · no replay"),
        _ => when.to_owned(),
    }
}

fn tab_label(tab: Tab, count: usize, programmes: Programmes) -> String {
    match (tab, programmes) {
        (Tab::Channels, _) => format!("Channels · {count}"),
        (Tab::Programs, Programmes::Unavailable) => "Programmes · no guide data".to_owned(),
        (Tab::Programs, Programmes::Building) => "Programmes · …".to_owned(),
        (Tab::Programs, Programmes::Ready) => format!("Programmes · {count}"),
    }
}

// --- Data ---------------------------------------------------------------------

fn epg(schedule: &dyn Schedule) -> Option<&EpgSchedule> {
    schedule.as_any()?.downcast_ref::<EpgSchedule>()
}

fn now_unix() -> UnixTime {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The guide's "now" (guide minute 0) for converting result times.
fn guide_now() -> UnixTime {
    epg(&*crate::schedule()).map_or_else(now_unix, EpgSchedule::now)
}

fn load_history() -> SearchHistory {
    let path = crate::data::data_dir().join(HISTORY_FILE);
    let list: Vec<String> = std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    SearchHistory::from_list(list, SearchHistory::DEFAULT_CAPACITY)
}

fn save_history(ui: Ui) {
    let Some(list) = ui
        .search
        .with_untracked(|s| s.as_ref().map(|s| s.state.history().to_list()))
    else {
        return;
    };
    let dir = crate::data::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(json) = serde_json::to_vec(&list)
        && std::fs::write(dir.join(HISTORY_FILE), json).is_err()
    {
        log::warn!("iptv search: could not save the search history");
    }
}

fn set_programmes(ui: Ui, status: Programmes) {
    ui.search.update(|s| {
        if let Some(s) = s {
            s.programmes = status;
        }
    });
}

/// Builds the indexes for the current guide data when they are stale:
/// channels at once, programmes a moment later (about 0.1 s for a large
/// EPG), so the screen shows first.
fn prepare(ui: Ui) {
    let version = ui.data_version.get_untracked();
    let schedule = crate::schedule();
    if INDEXES.with(|i| i.borrow().version != Some(version)) {
        let mut channels = ChannelIndex::from_schedule(&*schedule);
        for c in 0..schedule.channel_count() {
            channels.add_key(c, &(c + 1).to_string());
        }
        INDEXES.with(|i| {
            *i.borrow_mut() = Indexes {
                version: Some(version),
                channels,
                programmes: None,
            }
        });
    }
    if epg(&*schedule).is_none() {
        set_programmes(ui, Programmes::Unavailable);
        return;
    }
    if INDEXES.with(|i| i.borrow().programmes.is_some()) {
        set_programmes(ui, Programmes::Ready);
        return;
    }
    set_programmes(ui, Programmes::Building);
    // Scheduled again by every prepare until built (a timer dies with its
    // owner); a timer that finds the index built does nothing.
    let _ = set_timeout(Duration::from_millis(30), move || {
        let wanted = INDEXES.with(|i| {
            let i = i.borrow();
            i.version == Some(version) && i.programmes.is_none()
        });
        let schedule = crate::schedule();
        let Some(epg) = epg(&*schedule).filter(|_| wanted) else {
            return;
        };
        let t = Instant::now();
        let keys = (0..schedule.channel_count()).map_while(|c| epg.channel(c).map(|g| &g.key));
        let index = ProgrammeIndex::build(epg.store(), keys);
        log::info!(
            "iptv search: indexed {} programmes in {} ms",
            index.programme_count(),
            t.elapsed().as_millis()
        );
        INDEXES.with(|i| i.borrow_mut().programmes = Some(index));
        set_programmes(ui, Programmes::Ready);
        search_again(ui);
    });
}

/// Runs both searches for `query` now and hands the results to the state.
fn run_search(ui: Ui, query: &str) {
    let favourites = ui.favorites.get_untracked();
    let schedule = crate::schedule();
    let days = |c: usize| schedule.catchup_days(c);
    let (channels, programmes) = INDEXES.with(|i| {
        let i = i.borrow();
        let channels = i.channels.search_channels(
            query,
            &favourites,
            ChannelSearchOptions {
                favourites_first: FAVOURITES_FIRST,
                limit: LIMIT,
            },
        );
        let opts = ProgrammeSearchOptions {
            include_past_without_catchup: INCLUDE_PAST_WITHOUT_CATCHUP,
            catchup_days: Some(&days),
            favourites: Some(&favourites),
            favourites_first: FAVOURITES_FIRST,
            limit: LIMIT,
            ..Default::default()
        };
        let programmes = i
            .programmes
            .as_ref()
            .map_or_else(Vec::new, |p| p.search_programmes(query, now_unix(), opts));
        (channels, programmes)
    });
    ui.search.update(|s| {
        if let Some(s) = s {
            s.state.set_results(query, channels, programmes);
        }
    });
}

/// Searches the current query again (new data, programme index ready).
fn search_again(ui: Ui) {
    let query = ui.search.with_untracked(|s| {
        s.as_ref()
            .filter(|s| s.state.phase() != Phase::Idle)
            .map(|s| s.state.query().to_owned())
    });
    if let Some(q) = query {
        run_search(ui, &q);
    }
}

fn debounce(ui: Ui, query: String) {
    let ticket = PENDING.with(|p| {
        p.set(p.get() + 1);
        p.get()
    });
    let _ = set_timeout(DEBOUNCE, move || {
        if PENDING.with(Cell::get) == ticket {
            run_search(ui, &query);
        }
    });
}

// --- Actions -------------------------------------------------------------------

/// Opens the screen (guide menu, control strip). A screen suspended by
/// playback comes back as it was.
pub fn open(ui: Ui) {
    let suspended = ui
        .search
        .with_untracked(|s| s.as_ref().map(|s| s.suspended));
    match suspended {
        Some(true) => resume(ui),
        Some(false) => {}
        None => {
            ui.search.set(Some(Screen {
                state: SearchState::new(load_history()),
                suspended: false,
                programmes: Programmes::Building,
            }));
            prepare(ui);
        }
    }
    if ui.mode.get_untracked() == ViewMode::Fullscreen {
        ui.mode.set(ViewMode::Guide);
    }
}

pub fn close(ui: Ui) {
    save_history(ui);
    ui.search.set(None);
    dismiss_keyboard();
}

/// The screen is shown and takes the remote.
pub fn is_open(ui: Ui) -> bool {
    ui.search.with(|s| s.as_ref().is_some_and(|s| !s.suspended))
}

/// A remote action while the screen is shown.
pub fn key(ui: Ui, action: Action) {
    let mapped = ui
        .search
        .with_untracked(|s| s.as_ref().and_then(|s| key_action(&s.state, action)));
    if let Some(a) = mapped {
        apply(ui, a);
    }
}

pub fn apply(ui: Ui, action: SearchAction) {
    let mut effect = SearchEffect::None;
    ui.search.update(|s| {
        if let Some(s) = s {
            effect = s.state.apply(action);
        }
    });
    match effect {
        SearchEffect::None => {}
        SearchEffect::Search(q) => debounce(ui, q),
        SearchEffect::Play(id) => choose(ui, id),
        SearchEffect::Close => close(ui),
    }
}

fn resume(ui: Ui) {
    let mut effect = SearchEffect::None;
    ui.search.update(|s| {
        if let Some(s) = s {
            s.suspended = false;
            effect = s.state.resume();
        }
    });
    prepare(ui);
    if let SearchEffect::Search(q) = effect {
        run_search(ui, &q);
    }
}

fn choose(ui: Ui, id: ResultId) {
    let hit = ui.search.with_untracked(|s| {
        s.as_ref().and_then(|s| {
            s.state
                .programmes()
                .iter()
                .find(|h| {
                    ResultId::Programme {
                        channel: h.channel,
                        start: h.start,
                    } == id
                })
                .cloned()
        })
    });
    match outcome(id, hit.as_ref()) {
        Outcome::Live(channel) => watch(ui, channel, Playback::Live),
        Outcome::CatchUp { channel, start } => {
            let start = guide_minute(start, guide_now());
            watch(ui, channel, Playback::CatchUp { start });
        }
        Outcome::Details { channel, start } => ui.description.set(Some(Origin {
            channel,
            program_start: guide_minute(start, guide_now()),
        })),
        Outcome::Nothing => {}
    }
}

/// Plays full screen; the screen waits until playback leaves full screen.
fn watch(ui: Ui, channel: usize, playback: Playback) {
    save_history(ui);
    ui.focus.update(|f| f.tap(channel, 0, &*crate::schedule()));
    ui.play(channel, playback);
    // Full screen first: the resume effect ignores a suspended screen in full screen.
    ui.mode.set(ViewMode::Fullscreen);
    ui.update_panels(|p, now| p.show_info(now));
    ui.search.update(|s| {
        if let Some(s) = s {
            s.suspended = true;
        }
    });
}

// --- View ----------------------------------------------------------------------

fn state<R>(ui: Ui, f: impl FnOnce(&Screen) -> R) -> Option<R> {
    ui.search.with(|s| s.as_ref().map(f))
}

fn focus_bg(ui: Ui, focused: bool) -> Rgba {
    if focused && ui.keys_shown() {
        tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
    } else {
        tint(WHITE, 0)
    }
}

fn secondary() -> Rgba {
    tint(WHITE, TEXT_SECONDARY_ALPHA)
}

pub fn layer(ui: Ui) -> impl IntoElement {
    // Back from playback (full screen left): the suspended screen returns.
    effect(move || {
        let left = ui.mode.get() != ViewMode::Fullscreen;
        if left
            && ui
                .search
                .with_untracked(|s| s.as_ref().is_some_and(|s| s.suspended))
        {
            resume(ui);
        }
    });
    // New guide data while open: fresh indexes, the query searched again.
    effect(move || {
        ui.data_version.get();
        if ui.search.with_untracked(Option::is_some) {
            prepare(ui);
            search_again(ui);
        }
    });
    when(move || is_open(ui), move || screen(ui))
}

fn screen(ui: Ui) -> impl IntoElement {
    let idle = computed(move || state(ui, |s| s.state.phase() == Phase::Idle).unwrap_or(true));
    view()
        .absolute()
        .inset_0()
        .pt_safe_area()
        .px(px(48.0))
        .py(px(24.0))
        .gap(px(12.0))
        .bg(move || rgb(ui.theme.with(|t| t.background.window)))
        .child(header(ui))
        .child(
            text(move || hint(ui))
                .text_size(px(13.0))
                .text_color(secondary()),
        )
        .child(
            view().flex_1().child(
                when(move || idle.get(), move || history(ui)).otherwise(move || results(ui)),
            ),
        )
}

fn hint(ui: Ui) -> &'static str {
    let idle = state(ui, |s| s.state.phase() == Phase::Idle).unwrap_or(true);
    match (ui.keys_shown(), idle) {
        (true, true) => "OK type   ▼ recent searches   Back close",
        (true, false) => {
            "▲▼ move   ◀▶ channels / programmes   OK watch   Back keyboard, then close"
        }
        (false, true) => "Type a channel or programme, or tap a recent search",
        (false, false) => "Tap a result to watch it",
    }
}

fn button(ui: Ui, label: &'static str, act: fn(Ui)) -> impl IntoElement {
    text(label)
        .px(px(16.0))
        .py(px(8.0))
        .rounded(px(RADIUS))
        .text_size(px(14.0))
        .text_color(rgb(WHITE))
        .bg(tint(WHITE, RESTING_ALPHA * 2))
        .on_press(move || {
            ui.touched();
            act(ui);
        })
}

fn header(ui: Ui) -> impl IntoElement {
    let field = element_ref();
    let want_keyboard = computed(move || {
        state(ui, |s| {
            s.state.keyboard_visible() && *s.state.focus() == Focus::Input
        })
        .unwrap_or(false)
    });
    effect(move || {
        if want_keyboard.get() {
            field.focus();
        } else {
            field.blur();
        }
    });
    // The platform closed the keyboard (Back on Android hides it first).
    let was_up = Rc::new(Cell::new(false));
    effect(move || {
        let up = keyboard_height() > px(0.0);
        if was_up.replace(up) && !up {
            apply(ui, SearchAction::KeyboardHidden);
        }
    });
    let field_focused = move || state(ui, |s| *s.state.focus() == Focus::Input).unwrap_or(false);
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(text("Search").text_size(px(20.0)).text_color(rgb(WHITE)))
        .child(
            input()
                .element_ref(&field)
                .flex_1()
                .value(move || state(ui, |s| s.state.query().to_owned()).unwrap_or_default())
                .placeholder("Channel, number or programme")
                .auto_focus(want_keyboard.get_untracked())
                .auto_capitalize(AutoCapitalize::None)
                .auto_correct(false)
                .return_key_type(ReturnKeyType::Search)
                .text_size(px(17.0))
                .text_color(rgb(WHITE))
                .placeholder_color(secondary())
                .px(px(14.0))
                .py(px(10.0))
                .rounded(px(RADIUS))
                .bg(move || {
                    if field_focused() && ui.keys_shown() {
                        tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
                    } else {
                        tint(WHITE, RESTING_ALPHA)
                    }
                })
                .on_change(move |text: &str| {
                    let same = state(ui, |s| s.state.query() == text).unwrap_or(true);
                    if !same {
                        apply(ui, SearchAction::Type(text.to_owned()));
                    }
                })
                .on_submit(move |_: &str| apply(ui, SearchAction::Ok))
                .on_focus(move || {
                    if !state(ui, |s| s.state.keyboard_visible()).unwrap_or(true) {
                        apply(ui, SearchAction::ShowKeyboard);
                    }
                })
                .on_blur(move || {
                    if state(ui, |s| s.state.keyboard_visible()).unwrap_or(false) {
                        apply(ui, SearchAction::KeyboardHidden);
                    }
                }),
        )
        .child(button(ui, "Clear", |ui| apply(ui, SearchAction::Clear)))
        .child(button(ui, "Close", close))
}

fn history(ui: Ui) -> impl IntoElement {
    let count = move || state(ui, |s| s.state.history().len()).unwrap_or(0);
    let entry =
        move |i: usize| state(ui, |s| s.state.history().entries().get(i).cloned()).flatten();
    let row = move |at: Position| {
        let focused = move || {
            let e = entry(at.index());
            e.is_some() && state(ui, |s| s.state.focus().clone()) == e.map(Focus::History)
        };
        text(move || entry(at.index()).unwrap_or_default())
            .h(px(48.0))
            .px(px(16.0))
            .pt(px(13.0))
            .rounded(px(RADIUS))
            .number_of_lines(1)
            .text_size(px(16.0))
            .text_color(rgb(WHITE))
            .bg(move || focus_bg(ui, focused()))
            .on_press(move || {
                ui.touched();
                if let Some(e) = entry(at.index()) {
                    apply(ui, SearchAction::Pick(Focus::History(e)));
                }
            })
    };
    view()
        .flex_1()
        .gap(px(8.0))
        .child(
            text(move || {
                if count() == 0 {
                    "No recent searches yet"
                } else {
                    "Recent searches"
                }
            })
            .text_size(px(13.0))
            .text_color(secondary()),
        )
        .child(
            virtual_list(count, row)
                .estimated_item_size(px(48.0))
                .flex_1(),
        )
}

fn tab_button(ui: Ui, tab: Tab) -> impl IntoElement {
    let selected = move || state(ui, |s| s.state.tab() == tab).unwrap_or(false);
    let focused =
        move || selected() && state(ui, |s| *s.state.focus() == Focus::Tabs) == Some(true);
    text(move || {
        state(ui, |s| tab_label(tab, s.state.count(tab), s.programmes)).unwrap_or_default()
    })
    .px(px(16.0))
    .py(px(8.0))
    .rounded(px(RADIUS))
    .text_size(px(15.0))
    .text_color(move || {
        if selected() {
            rgb(ui.theme.with(|t| t.accent.light))
        } else {
            secondary()
        }
    })
    .bg(move || {
        if focused() && ui.keys_shown() {
            tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
        } else if selected() {
            tint(WHITE, RESTING_ALPHA)
        } else {
            tint(WHITE, 0)
        }
    })
    .on_press(move || {
        ui.touched();
        apply(ui, SearchAction::ShowTab(tab));
    })
}

fn status(ui: Ui) -> String {
    state(ui, |s| match s.state.phase() {
        Phase::Typing => "Searching…".to_owned(),
        Phase::NoResults => format!("Nothing found for “{}”", s.state.query().trim()),
        Phase::Idle | Phase::Results => String::new(),
    })
    .unwrap_or_default()
}

/// The result at `i` of the shown tab, with its identity.
fn result_at(ui: Ui, i: usize) -> Option<(ResultId, Option<ProgrammeHit>)> {
    state(ui, |s| match s.state.tab() {
        Tab::Channels => s
            .state
            .channels()
            .get(i)
            .map(|h| (ResultId::Channel(h.channel), None)),
        Tab::Programs => s.state.programmes().get(i).map(|h| {
            let id = ResultId::Programme {
                channel: h.channel,
                start: h.start,
            };
            (id, Some(h.clone()))
        }),
    })
    .flatten()
}

fn results(ui: Ui) -> impl IntoElement {
    let list = element_ref();
    let count = move || state(ui, |s| s.state.count(s.state.tab())).unwrap_or(0);
    let focused_index = computed(move || {
        let focus = state(ui, |s| s.state.focus().clone())?;
        (0..count())
            .find(|&i| result_at(ui, i).map(|r| Focus::Result(r.0)).as_ref() == Some(&focus))
    });
    effect(move || {
        if let Some(i) = focused_index.get()
            && ui.keys_shown()
        {
            list.scroll_to_index(i as u32, percentage(0.5), px(0.0), false);
        }
    });
    view()
        .flex_1()
        .gap(px(8.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(tab_button(ui, Tab::Channels))
                .child(tab_button(ui, Tab::Programs))
                .child(
                    text(move || status(ui))
                        .flex_1()
                        .px(px(12.0))
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .text_color(secondary()),
                ),
        )
        .child(
            virtual_list(count, move |at| result_row(ui, at, focused_index))
                .estimated_item_size(px(ROW_H))
                .element_ref(&list)
                .flex_1(),
        )
}

fn result_row(ui: Ui, at: Position, focused_index: Memo<Option<usize>>) -> impl IntoElement {
    let item = move || result_at(ui, at.index());
    let line1 = move || match item() {
        Some((ResultId::Channel(c), _)) => {
            let star = ui.favorites.with(|f| f.contains(&c));
            format!(
                "{}   {}{}",
                c + 1,
                if star { "★ " } else { "" },
                channel_name(c)
            )
        }
        Some((_, Some(h))) => h.title,
        _ => String::new(),
    };
    let line2 = move || match item() {
        Some((ResultId::Channel(c), _)) if ui.playing.get() == Some(c) => "Playing".to_owned(),
        Some((_, Some(h))) => {
            let now = guide_now();
            format!(
                "{} · {}–{}",
                channel_name(h.channel),
                clock(display_minute(h.start, now)),
                clock(display_minute(h.end, now))
            )
        }
        _ => String::new(),
    };
    let marker = move || match item() {
        Some((_, Some(h))) => badge(&h),
        _ => String::new(),
    };
    view()
        .h(px(ROW_H))
        .px(px(16.0))
        .flex_row()
        .items_center()
        .gap(px(16.0))
        .rounded(px(RADIUS))
        .bg(move || focus_bg(ui, focused_index.get() == Some(at.index())))
        .on_press(move || {
            ui.touched();
            if let Some((id, _)) = item() {
                apply(ui, SearchAction::Pick(Focus::Result(id)));
            }
        })
        .child(
            view()
                .flex_1()
                .gap(px(2.0))
                .child(
                    text(line1)
                        .number_of_lines(1)
                        .text_size(px(16.0))
                        .text_color(rgb(WHITE)),
                )
                .child(
                    text(line2)
                        .number_of_lines(1)
                        .text_size(px(13.0))
                        .text_color(secondary()),
                ),
        )
        .child(
            text(marker)
                .text_size(px(13.0))
                .text_color(move || match item() {
                    Some((_, Some(h))) if h.airing == Airing::Live => {
                        rgb(ui.theme.with(|t| t.accent.light))
                    }
                    _ => secondary(),
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use iptv_search::MatchQuality;

    fn hit(airing: Airing, catchup: bool) -> ProgrammeHit {
        ProgrammeHit {
            channel: 3,
            start: 1_000,
            end: 2_800,
            title: "Synthetic Show".into(),
            airing,
            catchup,
            score: MatchQuality::Word,
        }
    }

    const P: ResultId = ResultId::Programme {
        channel: 3,
        start: 1_000,
    };

    #[test]
    fn choosing_a_result() {
        assert_eq!(outcome(ResultId::Channel(7), None), Outcome::Live(7));
        assert_eq!(outcome(P, Some(&hit(Airing::Live, true))), Outcome::Live(3));
        assert_eq!(
            outcome(P, Some(&hit(Airing::Ended, true))),
            Outcome::CatchUp {
                channel: 3,
                start: 1_000
            }
        );
        let details = Outcome::Details {
            channel: 3,
            start: 1_000,
        };
        assert_eq!(outcome(P, Some(&hit(Airing::Upcoming, false))), details);
        assert_eq!(outcome(P, Some(&hit(Airing::Ended, false))), details);
        assert_eq!(outcome(P, None), Outcome::Nothing);
    }

    #[test]
    fn remote_keys() {
        let mut s = SearchState::new(SearchHistory::default());
        let k = |s: &SearchState, a| key_action(s, a);
        // Idle field: OK (re)opens the keyboard; menu keys do nothing here.
        assert_eq!(k(&s, Action::Select), Some(SearchAction::ShowKeyboard));
        assert_eq!(k(&s, Action::ShowMenu), None);
        assert_eq!(k(&s, Action::MoveLeft), Some(SearchAction::Left));
        assert_eq!(k(&s, Action::Back), Some(SearchAction::Back));
        assert_eq!(
            k(&s, Action::Digit(4)),
            Some(SearchAction::Type("4".into()))
        );
        s.apply(SearchAction::Type("10".into()));
        assert_eq!(
            k(&s, Action::Digit(5)),
            Some(SearchAction::Type("105".into()))
        );
        // Keyboard up with a query: OK submits; keyboard down: OK brings it back.
        assert_eq!(k(&s, Action::Select), Some(SearchAction::Ok));
        s.apply(SearchAction::KeyboardHidden);
        assert_eq!(k(&s, Action::Select), Some(SearchAction::ShowKeyboard));
        // On a result OK is OK.
        s.set_results(
            "10",
            vec![iptv_search::ChannelHit {
                channel: 9,
                score: MatchQuality::Exact,
            }],
            vec![],
        );
        s.apply(SearchAction::Down);
        s.apply(SearchAction::Down);
        assert_eq!(s.focus(), &Focus::Result(ResultId::Channel(9)));
        assert_eq!(k(&s, Action::Select), Some(SearchAction::Ok));
    }

    #[test]
    fn times_convert_to_guide_minutes() {
        let now = 1_800_000_000;
        assert_eq!(guide_minute(now, now), 0);
        assert_eq!(guide_minute(now + 90, now), 2, "inside the programme");
        assert_eq!(guide_minute(now - 3_600, now), -60);
        assert_eq!(guide_minute(now - 3_590, now), -59);
        assert_eq!(display_minute(now + 90, now), 1);
        assert_eq!(display_minute(now - 30, now), -1);
    }

    #[test]
    fn labels() {
        assert_eq!(badge(&hit(Airing::Live, false)), "On now");
        assert_eq!(badge(&hit(Airing::Live, true)), "On now · ↺ replay");
        assert_eq!(badge(&hit(Airing::Ended, true)), "Earlier · ↺ replay");
        assert_eq!(badge(&hit(Airing::Ended, false)), "Earlier · no replay");
        assert_eq!(badge(&hit(Airing::Upcoming, false)), "Later");
        assert_eq!(
            tab_label(Tab::Channels, 3, Programmes::Unavailable),
            "Channels · 3"
        );
        assert_eq!(
            tab_label(Tab::Programs, 0, Programmes::Unavailable),
            "Programmes · no guide data"
        );
        assert_eq!(
            tab_label(Tab::Programs, 0, Programmes::Building),
            "Programmes · …"
        );
        assert_eq!(
            tab_label(Tab::Programs, 12, Programmes::Ready),
            "Programmes · 12"
        );
    }
}
