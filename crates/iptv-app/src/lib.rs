//! iptv-rs NSH shell (TV + tablet): remote (D-pad) and touch drive one
//! app-owned focus model (`guide.rs`); the layout follows the reference
//! UX's regions (context bar, groups, guide grid, preview + info panel) over
//! one persistent video layer (`video.rs`).

pub use iptv_core::guide;
pub mod theme;

mod grid;
mod panels;
mod video;

use std::time::Instant;

use guide::{Airing, Focus, Move, Pane};
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
}

impl Ui {
    /// OK or a tap on the focused programme: a live one plays (OK on the
    /// channel already playing goes full screen); ended and upcoming ones
    /// only show their details (no catch-up or reminders yet).
    pub fn watch(&self) {
        let (channel, airing) = self.focus.with(|f| (f.channel, f.program().airing()));
        if airing != Airing::Live {
            return;
        }
        if self.playing.get() == Some(channel) {
            self.mode.set(ViewMode::Fullscreen);
        } else {
            self.playing.set(Some(channel));
        }
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

    fn toggle(&self, mode: ViewMode) {
        self.mode.update(|m| *m = if *m == mode { ViewMode::Guide } else { mode });
    }
}

/// `0xRRGGBB` with alpha, as an NSH colour.
pub fn tint(rgb_hex: u32, alpha: u8) -> Rgba {
    rgba(theme::with_alpha(rgb_hex, alpha))
}

/// Guide minute 0 is 18:00 on the synthetic clock.
pub fn clock(minute: i32) -> String {
    let m = (18 * 60 + minute).rem_euclid(24 * 60);
    format!("{:02}:{:02}", m / 60, m % 60)
}

pub fn channel_name(ch: usize) -> String {
    format!("Channel {}", ch + 1)
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
    };
    bind_keys(ui);

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
        let Some((first, last)) = ui.visible_rows.get_untracked() else { return };
        if ch < first + EDGE {
            rows.scroll_to_index(ch.saturating_sub(EDGE) as u32, percentage(0.0), px(0.0), false);
        } else if ch + EDGE > last {
            let target = (ch + EDGE).min(guide::CHANNELS - 1);
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
                .pointer_events(move || if fullscreen() { PointerEvents::None } else { PointerEvents::Auto })
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
            // Full screen: a tap anywhere returns to the guide.
            view()
                .absolute()
                .inset_0()
                .pointer_events(move || if fullscreen() { PointerEvents::Auto } else { PointerEvents::None })
                .on_press(move || {
                    ui.touched();
                    ui.mode.set(ViewMode::Guide);
                }),
        )
}

fn bind_keys(ui: Ui) {
    let timed = move |name: &'static str, f: &dyn Fn()| {
        let t = Instant::now();
        f();
        let us = t.elapsed().as_micros();
        ui.stats.update(|s| {
            s.keys += 1;
            s.last_us = us;
            s.max_us = s.max_us.max(us);
        });
        log::info!("iptv key={name} n={} handler_us={us}", ui.stats.with(|s| s.keys));
    };
    // The first key after touch starts from a row on screen.
    let from_touch = move || {
        if ui.input.get() == Input::Touch {
            if let Some((first, last)) = ui.visible_rows.get() {
                ui.focus.update(|f| f.adopt_rows(first, last));
            }
            ui.input.set(Input::Keys);
        }
    };
    let repeat = KeyOptions { repeat: true, ..Default::default() };
    for (name, m) in [("up", Move::Up), ("down", Move::Down), ("left", Move::Left), ("right", Move::Right)] {
        on_key_with(name, repeat, move |_| {
            timed(name, &|| {
                from_touch();
                // In full screen the directions first bring the guide back.
                if ui.mode.get() == ViewMode::Fullscreen {
                    ui.mode.set(ViewMode::Overlay);
                    return;
                }
                ui.focus.update(|f| f.apply(m));
            })
        });
    }
    // OK: Enter, or the D-pad centre with the NSH keymap patch.
    on_key("enter", move |_| {
        timed("enter", &|| {
            from_touch();
            match ui.focus.with(|f| f.pane) {
                Pane::Groups => ui.focus.update(Focus::enter_group),
                Pane::Guide => ui.watch(),
            }
        })
    });
    // Keyboard shortcuts for the spike (remote keys come with Phase 0).
    on_key("o", move |_| ui.toggle(ViewMode::Overlay));
    on_key("f", move |_| ui.toggle(ViewMode::Fullscreen));
    // Back: full screen/overlay → guide; guide → groups; groups → leave.
    let back_target = computed(move || (ui.mode.get(), ui.focus.with(|f| f.pane)));
    effect(move || match back_target.get() {
        (ViewMode::Guide, Pane::Groups) => {}
        (ViewMode::Guide, Pane::Guide) => on_back_press(move || {
            ui.focus.update(|f| f.pane = Pane::Groups);
            true
        }),
        _ => on_back_press(move || {
            ui.mode.set(ViewMode::Guide);
            true
        }),
    });
}
