//! Text entry for one wizard field: a sheet with one NSH input, focused
//! when it opens so the platform keyboard (the TV's IME) comes up. Return
//! or Done keeps the text, Back keeps it too (as dismissing a keyboard
//! does), Cancel restores the field. Other screens (the playlists screen)
//! use the same sheet through [`text_sheet`].

use iptv_core::wizard::{Field, WizardEvent};
use nsh_core::element::input;
use nsh_core::prelude::*;

use super::{RESTING, WHITE, words};
use crate::theme::{RADIUS, RADIUS_LARGE, TEXT_SECONDARY_ALPHA};
use crate::{Ui, tint};

/// The field being typed into and its text so far.
#[derive(Clone, PartialEq, Eq)]
pub struct Editing {
    pub field: Field,
    pub text: String,
}

pub fn begin(ui: Ui, field: Field) {
    ui.wizard.update(|w| {
        if let Some(open) = w {
            let text = open.wizard.value(field).to_owned();
            open.editing = Some(Editing { field, text });
        }
    });
}

pub fn is_editing(ui: Ui) -> bool {
    ui.wizard
        .with(|w| w.as_ref().is_some_and(|o| o.editing.is_some()))
}

/// Ends entry and gives the wizard the text.
pub fn commit(ui: Ui) {
    let mut done = None;
    ui.wizard
        .update(|w| done = w.as_mut().and_then(|o| o.editing.take()));
    dismiss_keyboard();
    if let Some(Editing { field, text }) = done {
        super::send(ui, WizardEvent::SetField { field, value: text });
    }
}

pub fn cancel(ui: Ui) {
    ui.wizard
        .update(|w| w.iter_mut().for_each(|o| o.editing = None));
    dismiss_keyboard();
}

fn set_text(ui: Ui, text: &str) {
    ui.wizard.update(|w| {
        if let Some(e) = w.as_mut().and_then(|o| o.editing.as_mut()) {
            e.text = text.to_owned();
        }
    });
}

/// A text sheet: what it edits and how its owner keeps the text.
#[derive(Clone, Copy)]
pub struct TextSheet {
    pub label: &'static str,
    pub placeholder: &'static str,
    /// A link: the keyboard offers URL keys.
    pub url: bool,
    /// A password: typed characters are hidden.
    pub masked: bool,
    pub text: fn(Ui) -> String,
    pub set_text: fn(Ui, &str),
    /// Return, Done (and the owner's Back) keep the text.
    pub commit: fn(Ui),
    pub cancel: fn(Ui),
}

fn wizard_text(ui: Ui) -> String {
    ui.wizard.with(|w| {
        w.as_ref()
            .and_then(|o| o.editing.as_ref().map(|e| e.text.clone()))
            .unwrap_or_default()
    })
}

fn button(label: &'static str, act: fn(Ui), ui: Ui) -> impl IntoElement {
    text(label)
        .px(px(20.0))
        .py(px(10.0))
        .rounded(px(RADIUS))
        .text_size(px(15.0))
        .text_color(rgb(WHITE))
        .bg(tint(WHITE, RESTING))
        .on_press(move || {
            ui.touched();
            act(ui);
        })
}

/// The wizard's sheet; made when entry starts, so its input exists only
/// then.
pub fn sheet(ui: Ui) -> impl IntoElement {
    let Some(field) = ui
        .wizard
        .with_untracked(|w| w.as_ref().and_then(|o| o.editing.as_ref().map(|e| e.field)))
    else {
        return view();
    };
    text_sheet(
        ui,
        TextSheet {
            label: words::label(field),
            placeholder: words::placeholder(field),
            url: field.is_url(),
            masked: field == Field::Password,
            text: wizard_text,
            set_text,
            commit,
            cancel,
        },
    )
}

/// A sheet with one text input over the screen, focused at once so the
/// platform keyboard comes up. Make it when entry starts.
pub fn text_sheet(ui: Ui, sheet: TextSheet) -> View {
    let TextSheet {
        label,
        placeholder,
        url,
        masked,
        text: current,
        set_text,
        commit,
        cancel,
    } = sheet;
    let keyboard = if url {
        KeyboardType::Url
    } else {
        KeyboardType::Default
    };
    view()
        .absolute()
        .inset_0()
        .pt_safe_area()
        .items_center()
        // Near the top: the keyboard takes the lower half.
        .pt(px(40.0))
        .bg(tint(0x000000, 0xA0))
        .child(
            view()
                .w(px(640.0))
                .p(px(24.0))
                .gap(px(14.0))
                .rounded(px(RADIUS_LARGE))
                .bg(move || rgb(ui.theme.with(|t| t.background.panel)))
                .child(
                    text(label)
                        .text_size(px(14.0))
                        .text_color(tint(WHITE, TEXT_SECONDARY_ALPHA)),
                )
                .child(
                    input()
                        .value(move || current(ui))
                        .on_change(move |t: &str| set_text(ui, t))
                        .on_submit(move |_: &str| commit(ui))
                        .auto_focus(true)
                        .masked(masked)
                        .keyboard_type(keyboard)
                        .auto_capitalize(AutoCapitalize::None)
                        .auto_correct(false)
                        .placeholder(placeholder)
                        .text_size(px(18.0))
                        .text_color(rgb(WHITE))
                        .px(px(14.0))
                        .py(px(10.0))
                        .rounded(px(RADIUS))
                        .border_1()
                        .border_color(move || rgb(ui.theme.with(|t| t.accent.main)))
                        .bg(move || rgb(ui.theme.with(|t| t.background.window))),
                )
                .child(
                    view()
                        .flex_row()
                        .gap(px(12.0))
                        .justify_end()
                        .child(button("Cancel", cancel, ui))
                        .child(button("Done", commit, ui)),
                ),
        )
}
