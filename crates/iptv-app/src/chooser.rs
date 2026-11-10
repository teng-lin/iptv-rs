//! Translucent choosers over full-screen video: picture size (with "apply
//! to all channels") and the sleep timer. Option sets follow the reference
//! UX's observed choosers (tivimate docs/09); wording is ours.

use std::time::Duration;

use iptv_core::player_options::{Chooser, PictureSize, SLEEP_MINUTES};
use nsh_core::prelude::*;

use crate::theme::{FOCUS_ALPHA, RADIUS, RADIUS_LARGE, TEXT_SECONDARY_ALPHA};
use crate::{Ui, tint};

const WHITE: u32 = 0xFFFFFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Picture,
    Sleep,
}

/// The open chooser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub kind: Kind,
    pub list: Chooser,
}

fn picture_label(size: PictureSize) -> &'static str {
    match size {
        PictureSize::Normal => "Normal",
        PictureSize::Full => "Stretch to fill",
        PictureSize::Zoom => "Zoom",
        PictureSize::CinemaZoom => "Zoom wide films",
        PictureSize::Reduced => "Smaller",
        PictureSize::Ratio4x3 => "4:3",
        PictureSize::Ratio16x9 => "16:9",
        PictureSize::Ratio235 => "2.35:1",
    }
}

fn labels(kind: Kind) -> Vec<String> {
    match kind {
        Kind::Picture => PictureSize::ALL
            .iter()
            .map(|p| picture_label(*p).to_owned())
            .chain(["Use for all channels".to_owned()])
            .collect(),
        Kind::Sleep => ["Off".to_owned()]
            .into_iter()
            .chain(SLEEP_MINUTES.iter().map(|m| format!("{m} minutes")))
            .collect(),
    }
}

pub fn open(ui: Ui, kind: Kind) {
    let selected = match kind {
        Kind::Picture => {
            let ch = ui.playing.get().unwrap_or(0);
            let size = ui.picture.with(|p| p.for_channel(ch));
            PictureSize::ALL
                .iter()
                .position(|p| *p == size)
                .unwrap_or(0)
        }
        Kind::Sleep => ui
            .sleep
            .with(|t| t.minutes())
            .and_then(|m| SLEEP_MINUTES.iter().position(|x| *x == m))
            .map_or(0, |i| i + 1),
    };
    ui.chooser.set(Some(Open {
        kind,
        list: Chooser::new(labels(kind).len(), selected),
    }));
}

pub fn close(ui: Ui) {
    ui.chooser.set(None);
}

pub fn move_focus(ui: Ui, step: isize) {
    ui.chooser
        .update(|c| c.iter_mut().for_each(|c| c.list.move_focus(step)));
}

/// OK (or a tap) on entry `index`.
pub fn pick(ui: Ui, index: usize) {
    let Some(kind) = ui.chooser.with(|c| c.as_ref().map(|c| c.kind)) else {
        return;
    };
    match kind {
        Kind::Picture => {
            let ch = ui.playing.get().unwrap_or(0);
            match PictureSize::ALL.get(index) {
                Some(size) => ui.picture.update(|p| p.set_channel(ch, *size)),
                None => ui.picture.update(|p| p.apply_to_all(ch)),
            }
        }
        Kind::Sleep => match index.checked_sub(1).and_then(|i| SLEEP_MINUTES.get(i)) {
            None => ui.sleep.update(|t| t.cancel()),
            Some(minutes) => start_sleep(ui, *minutes),
        },
    }
    close(ui);
}

/// Starts the sleep timer; when it fires, playback stops.
fn start_sleep(ui: Ui, minutes: u32) {
    let now = ui.now_ms();
    ui.sleep.update(|t| t.set(minutes, now));
    let _ = set_timeout(Duration::from_secs(u64::from(minutes) * 60), move || {
        let mut fired = false;
        let now = ui.now_ms();
        ui.sleep.update(|t| fired = t.fire_if_due(now));
        if fired {
            log::info!("iptv sleep timer: stopping playback");
            ui.playing.set(None);
            ui.mode.set(crate::ViewMode::Guide);
        }
    });
}

pub fn layer(ui: Ui) -> impl IntoElement {
    let open = move || ui.chooser.with(Option::is_some);
    let row = move |index: usize| {
        let entry = move || {
            ui.chooser.with(|c| {
                c.as_ref().and_then(|c| {
                    labels(c.kind)
                        .get(index)
                        .cloned()
                        .map(|l| (l, c.list.clone()))
                })
            })
        };
        text(move || {
            entry().map_or(String::new(), |(label, list)| {
                format!(
                    "{}{label}",
                    if list.selected == index {
                        "✓  "
                    } else {
                        "    "
                    }
                )
            })
        })
        .px(px(16.0))
        .py(px(10.0))
        .rounded(px(RADIUS))
        .text_size(px(15.0))
        .text_color(rgb(WHITE))
        .opacity(move || if entry().is_some() { 1.0_f32 } else { 0.0 })
        .h(move || px(if entry().is_some() { 42.0 } else { 0.0 }))
        .bg(move || {
            let focused = ui.keys_shown() && entry().is_some_and(|(_, l)| l.focus == index);
            if focused {
                tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
            } else {
                tint(WHITE, 0)
            }
        })
        .on_press(move || {
            ui.touched();
            pick(ui, index);
        })
    };
    let title = move || match ui.chooser.with(|c| c.as_ref().map(|c| c.kind)) {
        Some(Kind::Picture) => "Picture size",
        Some(Kind::Sleep) => "Sleep timer",
        None => "",
    };
    view()
        .absolute()
        .inset_0()
        .items_end()
        .justify_end()
        .pt_safe_area()
        .p(px(40.0))
        .opacity(move || if open() { 1.0_f32 } else { 0.0 })
        .pointer_events(move || {
            if open() {
                PointerEvents::BoxNone
            } else {
                PointerEvents::None
            }
        })
        .child(
            view()
                .w(px(300.0))
                .p(px(12.0))
                .gap(px(2.0))
                .rounded(px(RADIUS_LARGE))
                // Translucent over moving video, like the reference's choosers.
                .bg(move || tint(ui.theme.with(|t| t.background.panel), 0xD0))
                .pointer_events(PointerEvents::Auto)
                .child(
                    text(title)
                        .px(px(16.0))
                        .py(px(8.0))
                        .text_size(px(13.0))
                        .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA)),
                )
                .children((0..9).map(row)),
        )
}
