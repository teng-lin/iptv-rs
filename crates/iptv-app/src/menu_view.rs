//! The guide's context menu (Long OK / Menu) as a scrolling side panel, and
//! the programme description popover. State lives in `iptv_core::menu`;
//! this opens, draws and performs it. Labels are iptv-rs's own wording.

use iptv_core::menu::{Heading, Menu, MenuItem, Origin, Row, program_menu};
use nsh_core::prelude::*;

use crate::theme::{FOCUS_ALPHA, RADIUS, RADIUS_LARGE, TEXT_SECONDARY_ALPHA};
use crate::{Ui, ViewMode, channel_name, clock, tint};

const WHITE: u32 = 0xFFFFFF;
const PANEL_W: f32 = 360.0;
const ROW_H: f32 = 48.0;

/// The actions this build can perform; the rest show disabled.
fn implemented(item: MenuItem) -> bool {
    matches!(
        item,
        MenuItem::Search
            | MenuItem::Settings
            | MenuItem::Description
            | MenuItem::ToggleFavorite
            | MenuItem::StartFromBeginning
            | MenuItem::ManageFavorites
            | MenuItem::AssignEpg
    )
}

fn label(item: MenuItem, favorite: bool) -> &'static str {
    use MenuItem::*;
    match item {
        Search => "Search",
        Settings => "Settings",
        StartFromBeginning => "Play from the beginning",
        ExternalPlayer => "Open in another player",
        Reminder => "Remind me",
        Record => "Record",
        CustomRecording => "Schedule a recording…",
        SaveToList => "Save to my list",
        Description => "Details",
        ToggleFavorite if favorite => "Remove from favourites",
        ToggleFavorite => "Add to favourites",
        BlockChannel => "Block channel",
        HideChannel => "Hide channel",
        AssignEpg => "Choose guide data…",
        ChannelOptions => "Channel options…",
        ManageFavorites => "Manage favourites…",
        ManageBlocked => "Manage blocked channels…",
        ManageHidden => "Manage hidden channels…",
        ReorderChannels => "Reorder channels",
        CopyChannels => "Copy channels to a group…",
        CreateGroup => "New group…",
        GroupOptions => "Group options…",
    }
}

fn title(channel: usize, p: crate::guide::Program) -> String {
    crate::schedule().title(channel, p)
}

/// Long OK / Menu: the menu for the focused programme (in full screen, the
/// playing channel's current programme).
pub fn open(ui: Ui) {
    let now = ui.focus.with(|f| f.now);
    let (channel, minute) = match (ui.mode.get(), ui.playing.get()) {
        (ViewMode::Fullscreen, Some(ch)) => (ch, now),
        _ => {
            let Some(ch) = crate::lists::focused_channel(ui) else {
                return; // an empty list
            };
            (ch, ui.focus.with(|f| f.cursor))
        }
    };
    let program = crate::schedule().program_at(channel, minute);
    let rows = program_menu(
        program,
        crate::schedule().catchup_days(channel),
        now,
        implemented,
    );
    let origin = Origin {
        channel,
        program_start: program.start,
    };
    ui.menu.set(Some(Menu::new(rows, origin)));
}

/// Closes the menu or popover; guide focus stays on the programme it was
/// opened on (it does not jump to now).
pub fn close(ui: Ui) {
    let origin = ui
        .menu
        .with(|m| m.as_ref().map(|m| m.origin))
        .or_else(|| ui.description.get());
    ui.menu.set(None);
    ui.description.set(None);
    if let Some(origin) = origin
        && ui.mode.get() != ViewMode::Fullscreen
    {
        crate::lists::tap(ui, origin.channel, origin.program_start);
    }
}

pub fn perform(ui: Ui, item: MenuItem) {
    let Some(origin) = ui.menu.with(|m| m.as_ref().map(|m| m.origin)) else {
        return;
    };
    ui.menu.set(None);
    match item {
        // Details replaces the menu; Back from it returns to the guide.
        MenuItem::Description => ui.description.set(Some(origin)),
        MenuItem::StartFromBeginning => {
            close(ui);
            ui.play(
                origin.channel,
                crate::Playback::CatchUp {
                    start: origin.program_start,
                    offset: 0,
                },
            );
            ui.mode.set(ViewMode::Fullscreen);
            ui.update_panels(|p, now| p.show_info(now));
        }
        MenuItem::ManageFavorites => {
            close(ui);
            crate::fav_editor::open(ui);
        }
        MenuItem::Search => {
            close(ui);
            crate::search_view::open(ui);
        }
        MenuItem::Settings => {
            close(ui);
            crate::settings_view::open(ui);
        }
        MenuItem::ToggleFavorite => {
            close(ui);
            crate::favorites::toggle(ui, origin.channel);
        }
        // The chooser opens over the guide; the guide's focus stays.
        MenuItem::AssignEpg => {
            close(ui);
            crate::guide_assign_view::open(ui, origin.channel);
        }
        other => {
            log::info!("iptv menu {other:?} not implemented yet");
            close(ui);
        }
    }
}

fn heading_text(ui: Ui, heading: Heading) -> String {
    let Some(origin) = ui.menu.with(|m| m.as_ref().map(|m| m.origin)) else {
        return String::new();
    };
    match heading {
        Heading::Program => {
            let p = crate::schedule().program_at(origin.channel, origin.program_start);
            format!(
                "{} · {}–{}",
                title(origin.channel, p),
                clock(p.start),
                clock(p.end)
            )
        }
        Heading::Channel => channel_name(origin.channel),
        Heading::Group => crate::lists::list_name(ui),
    }
}

pub fn layer(ui: Ui) -> impl IntoElement {
    let open = move || ui.menu.with(Option::is_some);
    let count = move || ui.menu.with(|m| m.as_ref().map_or(0, |m| m.rows.len()));
    let list = element_ref();
    let focus = computed(move || ui.menu.with(|m| m.as_ref().map(|m| m.focus)));
    effect(move || {
        if let Some(i) = focus.get() {
            list.scroll_to_index(i as u32, percentage(0.5), px(0.0), false);
        }
    });
    let row = move |at: Position| {
        let row = move || {
            ui.menu
                .with(|m| m.as_ref().and_then(|m| m.rows.get(at.index()).copied()))
        };
        let favorite = move || {
            let ch = ui.menu.with(|m| m.as_ref().map(|m| m.origin.channel));
            ch.is_some_and(|ch| ui.favorites.with(|f| f.contains(&ch)))
        };
        let focused = move || ui.keys_shown() && focus.get() == Some(at.index());
        text(move || match row() {
            Some(Row::Heading(h)) => heading_text(ui, h),
            Some(Row::Action { item, .. }) => label(item, favorite()).to_owned(),
            None => String::new(),
        })
        .h(px(ROW_H))
        .px(px(16.0))
        .pt(move || {
            px(if matches!(row(), Some(Row::Heading(_))) {
                18.0
            } else {
                13.0
            })
        })
        .rounded(px(RADIUS))
        .number_of_lines(1)
        .text_size(move || {
            px(if matches!(row(), Some(Row::Heading(_))) {
                13.0
            } else {
                16.0
            })
        })
        .text_color(move || match row() {
            Some(Row::Action { enabled: true, .. }) => rgb(WHITE),
            Some(Row::Heading(_)) => tint(WHITE, TEXT_SECONDARY_ALPHA),
            _ => tint(WHITE, 0x50),
        })
        .bg(move || {
            if focused() {
                tint(ui.theme.with(|t| t.accent.main), FOCUS_ALPHA)
            } else {
                tint(WHITE, 0)
            }
        })
        .on_press(move || {
            ui.touched();
            let picked = ui
                .menu
                .with(|m| m.clone())
                .and_then(|mut m| m.tap(at.index()));
            if let Some(item) = picked {
                perform(ui, item);
            }
        })
    };
    let menu_opacity = move || if open() { 1.0_f32 } else { 0.0 };
    view()
        .absolute()
        .inset_0()
        .flex_row()
        // The description popover is in it and shows without the menu.
        .opacity(move || {
            if open() || ui.description.with(Option::is_some) {
                1.0_f32
            } else {
                0.0
            }
        })
        .pointer_events(move || {
            if open() {
                PointerEvents::Auto
            } else {
                PointerEvents::None
            }
        })
        .child(
            view()
                .flex_1()
                .bg(tint(0x000000, 0x80))
                .opacity(menu_opacity)
                .on_press(move || {
                    ui.touched();
                    close(ui);
                }),
        )
        .child(
            view()
                .w(px(PANEL_W))
                .opacity(menu_opacity)
                .pt_safe_area()
                .px(px(12.0))
                .py(px(16.0))
                .bg(move || rgb(ui.theme.with(|t| t.background.panel)))
                .child(
                    virtual_list(count, row)
                        .estimated_item_size(px(ROW_H))
                        .element_ref(&list)
                        .flex_1(),
                ),
        )
        .child(description(ui))
}

/// The description popover: title, time range and text; no actions (those
/// live in the menu). Back or a tap closes it.
fn description(ui: Ui) -> impl IntoElement {
    let shown = move || ui.description.get().is_some();
    let program = move || {
        ui.description.get().map(|o| {
            (
                o.channel,
                crate::schedule().program_at(o.channel, o.program_start),
            )
        })
    };
    view()
        .absolute()
        .inset_0()
        .items_end()
        .pt_safe_area()
        .px(px(40.0))
        .py(px(40.0))
        .bg(move || tint(0x000000, if shown() { 0x80 } else { 0 }))
        .opacity(move || if shown() { 1.0_f32 } else { 0.0 })
        .pointer_events(move || if shown() { PointerEvents::Auto } else { PointerEvents::None })
        .on_press(move || {
            ui.touched();
            close(ui);
        })
        .child(
            view()
                .w(px(480.0))
                .gap(px(10.0))
                .p(px(24.0))
                .rounded(px(RADIUS_LARGE))
                .bg(move || rgb(ui.theme.with(|t| t.background.panel)))
                .child(
                    text(move || program().map_or(String::new(), |(ch, p)| title(ch, p)))
                        .text_size(px(22.0))
                        .text_color(rgb(WHITE)),
                )
                .child(
                    text(move || {
                        program().map_or(String::new(), |(ch, p)| {
                            format!("{} · {}–{}", channel_name(ch), clock(p.start), clock(p.end))
                        })
                    })
                    .text_size(px(14.0))
                    .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA)),
                )
                .child(
                    text("A synthetic programme description for testing the guide. Real descriptions come from the EPG.")
                        .text_size(px(15.0))
                        .text_color(rgb(WHITE)),
                ),
        )
}
