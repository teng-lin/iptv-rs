//! iptv-rs NSH shell (TV + tablet): remote (D-pad) and touch drive one
//! app-owned focus model (`guide.rs`); the layout follows the reference
//! UX's regions (context bar, groups, guide grid, preview + info panel) over
//! one persistent video layer (`video.rs`).

pub use iptv_core::guide;
pub mod theme;

mod chooser;
mod grid;
mod input;
mod menu_view;
mod panels;
mod player_panels;
mod video;

use std::time::{Duration, Instant};

use guide::{Airing, Focus, Pane, Program, Schedule, SyntheticSchedule};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use iptv_core::menu::{Menu, Origin};
use iptv_core::panels::Panels;
use iptv_core::player_options::{PictureSettings, SleepTimer};
use nsh_core::prelude::*;
use theme::Theme;

#[native_module("iptv-rs/native")]
fn native_module(registry: &mut Registry) {
    registry.register::<App>();
}

/// The last kind of input: the focus fill only shows for keys, as in
/// Android's touch mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Keys,
    Touch,
}

/// How the UI sits over the video layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// Opaque guide; video in the preview rectangle.
    Guide,
    /// Full-screen video under a translucent guide.
    Overlay,
    /// Full-screen video, no UI.
    Fullscreen,
}

/// What the player is showing for the playing channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Playback {
    Live,
    /// An archived programme from its start (guide minutes).
    CatchUp {
        start: i32,
    },
}

/// Unix time of guide minute 0 on the synthetic clock (catch-up URLs carry
/// absolute times).
pub const GUIDE_EPOCH_UNIX: i64 = 1_794_333_600;

/// Keys handled and handler time, for the status line and logcat.
#[derive(Clone, Copy, PartialEq, Default)]
pub struct Stats {
    pub keys: u64,
    pub last_us: u128,
    pub max_us: u128,
}

/// The app's state, shared by the regions (signals are `Copy`).
#[derive(Clone, Copy)]
pub struct Ui {
    pub focus: Signal<Focus>,
    pub playing: Signal<Option<usize>>,
    pub input: Signal<Input>,
    pub mode: Signal<ViewMode>,
    pub theme: Signal<Theme>,
    pub stats: Signal<Stats>,
    pub player_status: Signal<String>,
    /// The guide rows on screen, as the list reports them.
    pub visible_rows: Signal<Option<(usize, usize)>>,
    /// Player panels over full-screen video.
    pub panels: Signal<Panels>,
    pub paused: Signal<bool>,
    /// The playing video's size, from the player.
    pub video_size: Signal<Option<(u32, u32)>>,
    /// The app's monotonic clock origin (core state machines take ms).
    pub epoch: Instant,
    /// The open context menu, if any.
    pub menu: Signal<Option<Menu>>,
    /// The programme description popover, if open.
    pub description: Signal<Option<Origin>>,
    pub favorites: Signal<BTreeSet<usize>>,
    /// Live or catch-up for the playing channel.
    pub playback: Signal<Playback>,
    /// Bumped when the guide data changes (views re-read `schedule()`).
    pub data_version: Signal<u64>,
    pub picture: Signal<PictureSettings>,
    pub sleep: Signal<SleepTimer>,
    /// The open chooser over the video (picture size, sleep timer).
    pub chooser: Signal<Option<chooser::Open>>,
}

impl Ui {
    /// OK or a tap on the focused programme: a live one plays (OK on the
    /// channel already playing goes full screen); ended and upcoming ones
    /// only show their details (no catch-up or reminders yet).
    pub fn watch(&self) {
        let (channel, program) = self
            .focus
            .with(|f| (f.channel, f.program(&*crate::schedule())));
        match program.airing() {
            Airing::Live => {
                let live = self.playback.get() == Playback::Live;
                if self.playing.get() == Some(channel) && live {
                    self.mode.set(ViewMode::Fullscreen);
                    self.update_panels(|p, now| p.show_info(now));
                } else {
                    self.play(channel, Playback::Live);
                }
            }
            // A past programme plays from the archive when the channel has
            // one that reaches it; otherwise OK leaves the guide as it is.
            Airing::Ended if in_archive(channel, program) => {
                self.play(
                    channel,
                    Playback::CatchUp {
                        start: program.start,
                    },
                );
                self.mode.set(ViewMode::Fullscreen);
                self.update_panels(|p, now| p.show_info(now));
            }
            Airing::Ended | Airing::Upcoming => {}
        }
    }

    /// Starts `channel` live or from the archive.
    pub fn play(&self, channel: usize, playback: Playback) {
        self.playback.set(playback);
        self.playing.set(Some(channel));
        self.paused.set(false);
    }

    /// A touch on any control.
    pub fn touched(&self) {
        self.input.set(Input::Touch);
    }

    pub fn keys_shown(&self) -> bool {
        self.input.get() == Input::Keys
    }

    /// A foreground UI surface colour. In guide mode the opaque colours are
    /// drawn by the backdrop *under* the video layer (`panels::backdrop`), so
    /// the foreground is clear and the preview rectangle shows the video; in
    /// overlay mode it tints the full-screen video.
    pub fn surface(&self, rgb_hex: u32) -> Rgba {
        let alpha = match self.mode.get() {
            ViewMode::Overlay => 0xC8,
            ViewMode::Guide | ViewMode::Fullscreen => 0x00,
        };
        tint(rgb_hex, alpha)
    }

    pub fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    /// Changes the panels and (re)schedules their auto-hide.
    pub fn update_panels(&self, f: impl FnOnce(&mut Panels, u64)) {
        let now = self.now_ms();
        self.panels.update(|p| f(p, now));
        if let Some(at) = self.panels.with(|p| p.hide_at_ms()) {
            let ui = *self;
            let _ = set_timeout(Duration::from_millis(at.saturating_sub(now)), move || {
                let now = ui.now_ms();
                // Later input moved the deadline: this tick is a no-op then.
                if ui
                    .panels
                    .with(|p| p.hide_at_ms().is_some_and(|at| now >= at))
                {
                    ui.panels.update(|p| {
                        p.tick(now);
                    });
                }
            });
        }
    }

    pub(crate) fn toggle(&self, mode: ViewMode) {
        self.mode
            .update(|m| *m = if *m == mode { ViewMode::Guide } else { mode });
    }
}

thread_local! {
    static SCHEDULE: RefCell<Rc<dyn Schedule>> = RefCell::new(Rc::new(SyntheticSchedule));
}

/// The guide data in use (synthetic until a playlist is imported).
pub fn schedule() -> Rc<dyn Schedule> {
    SCHEDULE.with(|s| s.borrow().clone())
}

/// Swaps the guide data (after an import) and tells the views to re-read.
pub fn set_schedule(ui: &Ui, schedule: Rc<dyn Schedule>) {
    SCHEDULE.with(|s| *s.borrow_mut() = schedule);
    ui.data_version.update(|v| *v += 1);
    ui.focus.update(|f| *f = Focus::default());
}

/// Whether `program` on `channel` is inside the channel's catch-up window.
pub fn in_archive(channel: usize, program: Program) -> bool {
    let days = i32::from(schedule().catchup_days(channel));
    days > 0 && program.end <= 0 && program.start >= -days * 24 * 60
}

/// `0xRRGGBB` with alpha, as an NSH colour.
pub fn tint(rgb_hex: u32, alpha: u8) -> Rgba {
    rgba(theme::with_alpha(rgb_hex, alpha))
}

/// Guide minute 0 is 18:00 on Monday 9 November on the synthetic clock;
/// other days show their date, as the guide does when browsing the past.
pub fn clock(minute: i32) -> String {
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let total = 18 * 60 + minute;
    let (day, m) = (total.div_euclid(24 * 60), total.rem_euclid(24 * 60));
    let time = format!("{:02}:{:02}", m / 60, m % 60);
    if day == 0 {
        time
    } else {
        format!("{} {} · {time}", DAYS[day.rem_euclid(7) as usize], 9 + day)
    }
}

pub fn channel_name(ch: usize) -> String {
    schedule().channel_name(ch)
}

#[component]
pub fn App() -> impl IntoElement {
    let ui = Ui {
        focus: signal(Focus::default()),
        playing: signal(None),
        input: signal(Input::Keys),
        mode: signal(ViewMode::Guide),
        theme: signal(Theme::default()),
        stats: signal(Stats::default()),
        player_status: signal("idle".to_owned()),
        visible_rows: signal(None),
        panels: signal(Panels::default()),
        paused: signal(false),
        video_size: signal(None),
        epoch: Instant::now(),
        menu: signal(None),
        description: signal(None),
        favorites: signal(BTreeSet::new()),
        playback: signal(Playback::Live),
        data_version: signal(0),
        picture: signal(PictureSettings::default()),
        sleep: signal(SleepTimer::default()),
        chooser: signal(None),
    };
    // Leaving full screen closes the panels.
    effect(move || {
        if ui.mode.get() != ViewMode::Fullscreen {
            ui.panels.update(|p| p.hide());
        }
    });
    input::bind(ui);
    bind_back(ui);

    let rows = element_ref();
    let groups = element_ref();
    let channel = computed(move || ui.focus.with(|f| f.channel));
    let group = computed(move || ui.focus.with(|f| f.group));
    // Keys scroll only when the focus comes within EDGE rows of the
    // viewport's edge (touch scrolls on its own).
    const EDGE: usize = 2;
    effect(move || {
        let ch = channel.get();
        if !ui.keys_shown() {
            return;
        }
        let Some((first, last)) = ui.visible_rows.get_untracked() else {
            return;
        };
        if ch < first + EDGE {
            rows.scroll_to_index(
                ch.saturating_sub(EDGE) as u32,
                percentage(0.0),
                px(0.0),
                false,
            );
        } else if ch + EDGE > last {
            let target = (ch + EDGE).min(schedule().channel_count().saturating_sub(1));
            rows.scroll_to_index(target as u32, percentage(1.0), px(0.0), false);
        }
    });
    effect(move || groups.scroll_to_index(group.get() as u32, percentage(0.5), px(0.0), false));

    let fullscreen = move || ui.mode.get() == ViewMode::Fullscreen;
    view()
        .flex_1()
        .bg(move || rgb(ui.theme.with(|t| t.background.window)))
        // Layers, bottom to top: opaque backdrop, video, UI, full-screen tap catcher.
        .child(panels::backdrop(ui))
        .child(video::layer(ui))
        .child(
            view()
                .flex_1()
                .opacity(move || if fullscreen() { 0.0_f32 } else { 1.0 })
                .pointer_events(move || {
                    if fullscreen() {
                        PointerEvents::None
                    } else {
                        PointerEvents::Auto
                    }
                })
                .child(panels::top_bar(ui))
                .child(
                    view()
                        .flex_1()
                        .flex_row()
                        .child(panels::group_column(ui, groups))
                        .child(grid::guide(ui, rows)),
                )
                .child(panels::info_panel(ui)),
        )
        .child(
            // Full screen: a tap toggles the player panels.
            view()
                .absolute()
                .inset_0()
                .pointer_events(move || {
                    if fullscreen() {
                        PointerEvents::Auto
                    } else {
                        PointerEvents::None
                    }
                })
                .on_press(move || {
                    ui.touched();
                    if ui.panels.with(|p| p.shown) == iptv_core::panels::Shown::Hidden {
                        ui.update_panels(|p, now| p.show_controls(now));
                    } else {
                        ui.panels.update(|p| p.hide());
                    }
                }),
        )
        .child(player_panels::layer(ui))
        .child(menu_view::layer(ui))
        .child(chooser::layer(ui))
}

/// Back: player panels → hidden; full screen/overlay → guide; guide →
/// groups; groups → leave the app.
fn bind_back(ui: Ui) {
    let back_target = computed(move || {
        let panels = ui.panels.with(|p| p.shown) != iptv_core::panels::Shown::Hidden;
        let menu = ui.menu.with(Option::is_some)
            || ui.description.with(Option::is_some)
            || ui.chooser.with(Option::is_some);
        (ui.mode.get(), ui.focus.with(|f| f.pane), panels || menu)
    });
    effect(move || match back_target.get() {
        _ if ui.chooser.with(Option::is_some) => on_back_press(move || {
            chooser::close(ui);
            true
        }),
        _ if ui.menu.with(Option::is_some) || ui.description.with(Option::is_some) => {
            on_back_press(move || {
                menu_view::close(ui);
                true
            })
        }
        (ViewMode::Fullscreen, _, true) => on_back_press(move || {
            ui.panels.update(|p| p.hide());
            true
        }),
        (ViewMode::Guide, Pane::Groups, _) => {}
        (ViewMode::Guide, Pane::Guide, _) => on_back_press(move || {
            // Back on a past/future programme returns to now; on now, to the groups.
            let mut moved = false;
            ui.focus
                .update(|f| moved = f.return_to_now(&*crate::schedule()));
            if !moved {
                ui.focus.update(|f| f.pane = Pane::Groups);
            }
            true
        }),
        _ => on_back_press(move || {
            ui.mode.set(ViewMode::Guide);
            true
        }),
    });
}
