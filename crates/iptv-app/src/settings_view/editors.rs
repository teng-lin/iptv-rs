//! The settings screen's editors and dialogs, over the screen: a list for
//! a choice (colours with their swatch), a stepper for numbers and
//! durations, the text sheet the wizard uses, the confirmation of a
//! command, and the read-only key list. Each says what it is about; the
//! remote drives them through `iptv_settings::screen`, taps directly.

use std::rc::Rc;

use iptv_settings::screen::{Editor, Effect, grid};
use iptv_settings::{Setting, Value, ValueType};
use nsh_core::prelude::*;

use super::{WHITE, accent_fill, remote_keys, secondary, state, swatch_colour, update, words};
use crate::theme::{RADIUS, RADIUS_LARGE, RESTING_ALPHA};
use crate::wizard_view::entry::{self, Sheet};
use crate::{Ui, tint};

const ENTRY_H: f32 = 42.0;

/// Which editor is open (rebuilt only when this changes).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Choice,
    Number,
    Text,
    Confirm,
    List,
}

fn editor<R>(ui: Ui, f: impl FnOnce(&Editor) -> R) -> Option<R> {
    state(ui, |s| s.editor().map(f)).flatten()
}

fn setting(ui: Ui) -> Option<&'static Setting> {
    editor(ui, Editor::setting).flatten()
}

/// What the open editor is about: its setting's or command's name.
fn subject(ui: Ui) -> &'static str {
    editor(ui, |e| words::label(e.key())).unwrap_or("")
}

pub fn layer(ui: Ui) -> impl IntoElement {
    let kind = computed(move || {
        editor(ui, |e| match e {
            Editor::Choice { .. } => Kind::Choice,
            Editor::Number { .. } => Kind::Number,
            Editor::Text { .. } => Kind::Text,
            Editor::Confirm { .. } => Kind::Confirm,
            Editor::List { .. } => Kind::List,
        })
    });
    view().absolute().inset_0().child(dynamic(move || {
        let shown = kind.get();
        untracked(move || match shown {
            Some(Kind::Choice) => choice(ui).into_element(),
            Some(Kind::Number) => stepper(ui).into_element(),
            Some(Kind::Text) => text_sheet(ui).into_element(),
            Some(Kind::Confirm) => confirm(ui).into_element(),
            Some(Kind::List) => key_list(ui).into_element(),
            None => view().into_element(),
        })
    }))
}

fn cancel(ui: Ui) {
    update(ui, |screen, _| {
        screen.cancel();
        Effect::None
    });
}

fn keep(ui: Ui) {
    update(ui, |screen, _| screen.commit());
}

/// A dimmed backdrop: a tap outside the editor cancels it.
fn backdrop(ui: Ui) -> View {
    view()
        .absolute()
        .inset_0()
        .bg(tint(0x000000, 0x60))
        .on_press(move || {
            ui.touched();
            cancel(ui);
        })
}

fn title(ui: Ui) -> impl IntoElement {
    text(move || subject(ui))
        .px(px(12.0))
        .py(px(6.0))
        .text_size(px(14.0))
        .text_color(secondary())
}

fn button(ui: Ui, label: &'static str, act: fn(Ui)) -> impl IntoElement {
    text(label)
        .px(px(18.0))
        .py(px(9.0))
        .rounded(px(RADIUS))
        .text_size(px(15.0))
        .text_color(rgb(WHITE))
        .bg(tint(WHITE, RESTING_ALPHA * 2))
        .on_press(move || {
            ui.touched();
            act(ui);
        })
}

/// A choice: the variants, the current one marked; on the right like the
/// settings panel itself.
fn choice(ui: Ui) -> impl IntoElement {
    let variants = move || {
        setting(ui)
            .and_then(|s| s.ty.choices())
            .map_or(&[][..], |c| c.variants)
    };
    let list = element_ref();
    let focus = computed(move || {
        editor(ui, |e| match e {
            Editor::Choice { focus, .. } => Some(*focus),
            _ => None,
        })
        .flatten()
    });
    effect(move || {
        if let Some(i) = focus.get() {
            list.scroll_to_index(i as u32, percentage(0.5), px(0.0), false);
        }
    });
    let entry = move |at: Position| {
        let variant = move || variants().get(at.index()).copied();
        let selected = move || {
            editor(ui, |e| match e {
                Editor::Choice { selected, .. } => *selected == at.index(),
                _ => false,
            }) == Some(true)
        };
        let swatch = move || {
            let s = setting(ui)?;
            swatch_colour(&s.ty, variant()?)
        };
        let label = move || {
            let (Some(s), Some(v)) = (setting(ui), variant()) else {
                return String::new();
            };
            let family = s.ty.choices().map_or("", |c| c.family);
            format!(
                "{}{}",
                if selected() { "✓  " } else { "    " },
                words::choice(family, v)
            )
        };
        view()
            .h(px(ENTRY_H))
            .px(px(12.0))
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .rounded(px(RADIUS))
            .bg(move || {
                if ui.keys_shown() && focus.get() == Some(at.index()) {
                    accent_fill(ui)
                } else {
                    tint(WHITE, 0)
                }
            })
            .on_press(move || {
                ui.touched();
                let i = at.index();
                update(ui, move |screen, _| screen.pick(i));
            })
            .child(
                text(label)
                    .flex_1()
                    .number_of_lines(1)
                    .text_size(px(15.0))
                    .text_color(rgb(WHITE)),
            )
            .child(
                view()
                    .w(px(18.0))
                    .h(px(18.0))
                    .rounded(px(4.0))
                    .bg(move || swatch().map_or(tint(WHITE, 0), rgb))
                    .opacity(move || if swatch().is_some() { 1.0_f32 } else { 0.0 }),
            )
    };
    view()
        .absolute()
        .inset_0()
        .flex_row()
        .justify_end()
        .child(backdrop(ui))
        .child(
            view()
                .w(px(340.0))
                .pt_safe_area()
                .p(px(14.0))
                .gap(px(4.0))
                .bg(move || rgb(ui.theme.with(|t| t.background.alt)))
                .child(title(ui))
                .child(
                    virtual_list(move || variants().len(), entry)
                        .estimated_item_size(px(ENTRY_H))
                        .element_ref(&list)
                        .flex_1(),
                ),
        )
}

/// The stepper's value and its range, in the setting's unit.
fn number_text(ui: Ui) -> (String, String) {
    let Some((s, value)) = editor(ui, |e| match e {
        Editor::Number { setting, value } => Some((*setting, *value)),
        _ => None,
    })
    .flatten() else {
        return Default::default();
    };
    let value = match s.ty {
        ValueType::Int { .. } => Value::Int(value),
        _ => Value::Duration(value),
    };
    let shown = words::value(s, &value);
    let range = grid(&s.ty).map_or(String::new(), |(min, max, step)| {
        format!(
            "From {} to {}, in steps of {}",
            words::amount(&s.ty, min),
            words::amount(&s.ty, max),
            words::amount(&s.ty, step)
        )
    });
    (shown, range)
}

/// A number or duration: − value +, the range, Cancel and Keep.
fn stepper(ui: Ui) -> impl IntoElement {
    let step = |by: i64| {
        move |ui: Ui| {
            update(ui, move |screen, _| {
                screen.step(by);
                Effect::None
            })
        }
    };
    let less = step(-1);
    let more = step(1);
    view()
        .absolute()
        .inset_0()
        .items_center()
        .justify_center()
        .child(backdrop(ui))
        .child(
            view()
                .w(px(440.0))
                .p(px(22.0))
                .gap(px(14.0))
                .rounded(px(RADIUS_LARGE))
                .bg(move || rgb(ui.theme.with(|t| t.background.alt)))
                .child(title(ui))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .gap(px(20.0))
                        .child(
                            text("−")
                                .w(px(56.0))
                                .text_center()
                                .py(px(6.0))
                                .rounded(px(RADIUS))
                                .text_size(px(24.0))
                                .text_color(rgb(WHITE))
                                .bg(tint(WHITE, RESTING_ALPHA * 2))
                                .on_press(move || {
                                    ui.touched();
                                    less(ui);
                                }),
                        )
                        .child(
                            text(move || format!("◀  {}  ▶", number_text(ui).0))
                                .text_size(px(26.0))
                                .text_color(move || rgb(ui.theme.with(|t| t.accent.light))),
                        )
                        .child(
                            text("+")
                                .w(px(56.0))
                                .text_center()
                                .py(px(6.0))
                                .rounded(px(RADIUS))
                                .text_size(px(24.0))
                                .text_color(rgb(WHITE))
                                .bg(tint(WHITE, RESTING_ALPHA * 2))
                                .on_press(move || {
                                    ui.touched();
                                    more(ui);
                                }),
                        ),
                )
                .child(
                    text(move || number_text(ui).1)
                        .px(px(12.0))
                        .text_size(px(13.0))
                        .text_color(secondary()),
                )
                .child(
                    view()
                        .flex_row()
                        .gap(px(12.0))
                        .justify_end()
                        .child(button(ui, "Cancel", cancel))
                        .child(button(ui, "Keep", keep)),
                ),
        )
}

/// Text: the wizard's entry sheet.
fn text_sheet(ui: Ui) -> View {
    let current = move || {
        editor(ui, |e| match e {
            Editor::Text { text, .. } => text.clone(),
            _ => String::new(),
        })
        .unwrap_or_default()
    };
    let note = move || {
        let invalid = editor(ui, |e| matches!(e, Editor::Text { invalid: true, .. }));
        if invalid == Some(true) {
            "That is not a valid value here".to_owned()
        } else {
            String::new()
        }
    };
    entry::panel(
        ui,
        Sheet {
            label: subject(ui).to_owned(),
            placeholder: "",
            keyboard: KeyboardType::Default,
            masked: false,
            current: Rc::new(current),
            note: Rc::new(note),
            set: Rc::new(move |t: &str| {
                let t = t.to_owned();
                update(ui, move |screen, _| {
                    screen.set_text(&t);
                    Effect::None
                });
            }),
            done: keep,
            cancel,
        },
    )
}

/// The question before a command that cannot be undone.
fn question(key: &str) -> &'static str {
    match key {
        "epg.clear" => {
            "The stored programmes are deleted. The guide stays empty until it is downloaded again."
        }
        _ => "This cannot be undone.",
    }
}

/// "Really?" before a command: what it does, Cancel and the command.
fn confirm(ui: Ui) -> impl IntoElement {
    let key = move || editor(ui, Editor::key).unwrap_or("");
    view()
        .absolute()
        .inset_0()
        .items_center()
        .justify_center()
        .child(backdrop(ui))
        .child(
            view()
                .w(px(440.0))
                .p(px(22.0))
                .gap(px(14.0))
                .rounded(px(RADIUS_LARGE))
                .bg(move || rgb(ui.theme.with(|t| t.background.alt)))
                .child(
                    text(move || format!("{}?", subject(ui)))
                        .text_size(px(18.0))
                        .text_color(rgb(WHITE)),
                )
                .child(
                    text(move || question(key()))
                        .text_size(px(14.0))
                        .text_color(secondary()),
                )
                .child(
                    view()
                        .flex_row()
                        .gap(px(12.0))
                        .justify_end()
                        .child(button(ui, "Cancel", cancel))
                        .child(
                            text(move || subject(ui))
                                .px(px(18.0))
                                .py(px(9.0))
                                .rounded(px(RADIUS))
                                .text_size(px(15.0))
                                .text_color(rgb(WHITE))
                                // OK goes ahead: the focused button.
                                .bg(move || accent_fill(ui))
                                .on_press(move || {
                                    ui.touched();
                                    keep(ui);
                                }),
                        ),
                ),
        )
}

/// The remote's keys and what they do (read-only).
fn key_list(ui: Ui) -> impl IntoElement {
    let lines = remote_keys::key_lines();
    let count = lines.len();
    let lines = Rc::new(lines);
    let list = element_ref();
    let focus = computed(move || {
        editor(ui, |e| match e {
            Editor::List { focus, .. } => Some(*focus),
            _ => None,
        })
        .flatten()
    });
    effect(move || {
        if let Some(i) = focus.get() {
            list.scroll_to_index(i as u32, percentage(0.5), px(0.0), false);
        }
    });
    let entry = move |at: Position| {
        let lines = lines.clone();
        let line = move || lines.get(at.index()).cloned();
        let heading = {
            let line = line.clone();
            move || line().is_some_and(|l| l.is_heading())
        };
        let (key, action) = (line.clone(), line);
        view()
            .h(px(ENTRY_H - 6.0))
            .px(px(12.0))
            .flex_row()
            .items_center()
            .gap(px(12.0))
            .rounded(px(RADIUS))
            .bg(move || {
                if ui.keys_shown() && focus.get() == Some(at.index()) {
                    accent_fill(ui)
                } else {
                    tint(WHITE, 0)
                }
            })
            .child(
                text(move || key().map(|l| l.key).unwrap_or_default())
                    .flex_1()
                    .number_of_lines(1)
                    .text_size(px(14.0))
                    .text_color(move || if heading() { secondary() } else { rgb(WHITE) }),
            )
            .child(
                text(move || action().map(|l| l.action).unwrap_or_default())
                    .number_of_lines(1)
                    .text_size(px(14.0))
                    .text_color(move || rgb(ui.theme.with(|t| t.accent.light))),
            )
    };
    view()
        .absolute()
        .inset_0()
        .flex_row()
        .justify_end()
        .child(backdrop(ui))
        .child(
            view()
                .w(px(440.0))
                .pt_safe_area()
                .p(px(14.0))
                .gap(px(4.0))
                .bg(move || rgb(ui.theme.with(|t| t.background.alt)))
                .child(title(ui))
                .child(
                    text("Changing keys comes in a later version.")
                        .px(px(12.0))
                        .text_size(px(12.0))
                        .text_color(secondary()),
                )
                .child(
                    virtual_list(move || count, entry)
                        .estimated_item_size(px(ENTRY_H - 6.0))
                        .element_ref(&list)
                        .flex_1(),
                ),
        )
}
