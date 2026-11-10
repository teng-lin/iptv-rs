//! The regions around the guide: the opaque backdrop under the video
//! layer, the groups column (shown with the rail while either has the
//! focus) and the geometry of the header (`header.rs`) and its preview.

use iptv_core::lists::{entry_count, entry_name};
use nsh_core::prelude::*;

use crate::guide::Pane;
use crate::theme::{FOCUS_ALPHA, RADIUS, RESTING_ALPHA, TEXT_SECONDARY_ALPHA, TEXT_SIZE};
use crate::{Ui, tint};

const WHITE: u32 = 0xFFFFFF;
pub const GROUPS_W: f32 = 220.0;

/// The groups column's width now (0 while the guide has the focus).
pub fn groups_width(ui: Ui) -> f32 {
    if crate::lists::columns_shown(ui) {
        GROUPS_W
    } else {
        0.0
    }
}
/// The preview at the header's top left (16:9) and the header's padding;
/// the header is as tall as the preview with its padding.
pub const PREVIEW_W: f32 = 256.0;
pub const PREVIEW_H: f32 = 144.0;
pub const HEADER_PAD_X: f32 = 16.0;
pub const HEADER_PAD_Y: f32 = 10.0;
pub const HEADER_H: f32 = PREVIEW_H + 2.0 * HEADER_PAD_Y;

/// The rectangle the video layer occupies in guide mode (x, y, width,
/// height): the preview, below the status bar.
pub fn preview_rect() -> (f32, f32, f32, f32) {
    let top = safe_area().top.0;
    (HEADER_PAD_X, top + HEADER_PAD_Y, PREVIEW_W, PREVIEW_H)
}

fn secondary() -> Rgba {
    tint(WHITE, TEXT_SECONDARY_ALPHA)
}

/// The opaque colours of the regions, drawn under the video layer with the
/// same geometry as the foreground (header, rail, groups column, guide).
/// The foreground stays clear in guide mode, so the video shows wherever it is
/// laid out: today the preview rectangle.
pub fn backdrop(ui: Ui) -> impl IntoElement {
    let panel = move || rgb(ui.theme.with(|t| t.background.panel));
    let window = move || rgb(ui.theme.with(|t| t.background.window));
    let alt = move || rgb(ui.theme.with(|t| t.background.alt));
    view()
        .absolute()
        .inset_0()
        .pointer_events(PointerEvents::None)
        .child(
            view()
                .pt_safe_area()
                .bg(panel)
                .child(view().h(px(HEADER_H))),
        )
        .child(
            view()
                .flex_1()
                .flex_row()
                .child(view().w(move || px(crate::rail::width(ui))).bg(alt))
                .child(view().w(move || px(groups_width(ui))).bg(panel))
                .child(view().flex_1().bg(window)),
        )
}

/// The groups column: Favourites, Recently watched, All channels, then the
/// playlist's groups. Moving over (or tapping) one shows its list.
pub fn group_column(ui: Ui, groups: ElementRef) -> impl IntoElement {
    // New data: the selected group stays in view.
    effect(move || {
        ui.guide_data();
        let group = ui.focus.with_untracked(|f| f.group);
        groups.scroll_to_index(group as u32, percentage(0.5), px(0.0), false);
    });
    virtual_list(
        move || {
            ui.guide_data();
            entry_count(&*crate::schedule())
        },
        move |at| {
            let current = move || ui.focus.with(|f| f.group) == at.index();
            let focused =
                move || current() && ui.focus.with(|f| f.pane) == Pane::Groups && ui.keys_shown();
            view()
                .h(px(44.0))
                .mx(px(8.0))
                .my(px(2.0))
                .px(px(12.0))
                .rounded(px(RADIUS))
                .justify_center()
                .bg(move || {
                    if focused() {
                        tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
                    } else if current() {
                        tint(WHITE, RESTING_ALPHA)
                    } else {
                        tint(WHITE, 0)
                    }
                })
                .on_press(move || {
                    ui.touched();
                    crate::lists::tap_entry(ui, at.index());
                })
                .child(
                    // Re-read on new data: the recycled cell keeps its index.
                    text(move || {
                        ui.guide_data();
                        entry_name(at.index(), &*crate::schedule())
                    })
                    .number_of_lines(1)
                    .text_size(px(TEXT_SIZE))
                    .text_color(move || {
                        if focused() {
                            rgb(WHITE)
                        } else if current() {
                            rgb(ui.theme.with(|t| t.accent.light))
                        } else {
                            secondary()
                        }
                    }),
                )
        },
    )
    .estimated_item_size(px(48.0))
    .element_ref(&groups)
    .w(move || px(groups_width(ui)))
    .py(px(6.0))
    .bg(move || ui.surface(ui.theme.with(|t| t.background.panel)))
}
