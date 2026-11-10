//! The settings screen's state: the chosen section, the focused row (by
//! key, never by position) and the open value editor. Toolkit-free and
//! pure like the rest of the crate: the app draws it, feeds it keys and
//! taps, and carries out the [`Effect`]s it answers (store a value, run a
//! command, close).
//!
//! Layout: the [`Section`]s, and the [`rows`] of the chosen one —
//! subsection headings, then settings and commands in catalogue order.
//! Rows whose visibility conditions fail are left out; advanced settings
//! show only with `advanced`. Every setting and command takes the focus,
//! also those the app cannot act on yet ([`Status`]), so the UI can say
//! why OK does nothing there.
//!
//! Keys. Sections: Up/Down choose (the rows follow), Right or OK enter the
//! rows, Back closes. Rows: Up/Down move, Page keys move five, Left or
//! Back return to the sections, OK edits — a switch flips at once, a
//! choice opens its list, a number or duration opens a stepper (Left/Down
//! less, Right/Up more, Page keys ten steps, OK keeps, Back cancels), text
//! opens a text editor (OK keeps a valid text, Back cancels).

use crate::catalogue::{self, Command};
use crate::schema::{Section, Setting, ValueType};
use crate::scope::Target;
use crate::store::Settings;
use crate::validate;
use crate::value::Value;

/// Rows (or list entries) one Page key moves.
const PAGE: usize = 5;
/// Steps one Page key moves in the stepper.
const BIG_STEP: i64 = 10;

/// One line of the rows pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row {
    /// A subsection title; never focused.
    Heading {
        section: Section,
        subsection: &'static str,
    },
    Setting(&'static Setting),
    Command(&'static Command),
}

impl Row {
    /// The key of a setting or command row.
    pub fn key(&self) -> Option<&'static str> {
        match self {
            Row::Heading { .. } => None,
            Row::Setting(s) => Some(s.key),
            Row::Command(c) => Some(c.key),
        }
    }
}

/// What OK on a row can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The app acts on the value or runs the command.
    Active,
    /// The app does not act on it yet (or it has no editor).
    Unavailable,
    /// Another setting switches it off (an `enabled_if` condition fails).
    Disabled,
}

/// What the screen reads from the app.
pub struct Ctx<'a> {
    pub settings: &'a Settings,
    /// Whether the app acts on a setting or command key.
    pub available: &'a dyn Fn(&str) -> bool,
}

impl Ctx<'_> {
    pub fn status(&self, row: &Row) -> Status {
        match row {
            Row::Heading { .. } => Status::Unavailable,
            Row::Command(c) if (self.available)(c.key) => Status::Active,
            Row::Command(_) => Status::Unavailable,
            Row::Setting(s) => {
                let editable = !matches!(s.ty, ValueType::KeyBindings { .. });
                if !editable || !(self.available)(s.key) {
                    Status::Unavailable
                } else if !self
                    .settings
                    .is_enabled(s.key, &Target::Global)
                    .unwrap_or(false)
                {
                    Status::Disabled
                } else {
                    Status::Active
                }
            }
        }
    }

    fn value(&self, s: &Setting) -> Value {
        self.settings
            .get(s.key)
            .unwrap_or_else(|_| s.default_value())
    }
}

/// The rows of `section` with the current values' visibility.
pub fn rows(section: Section, settings: &Settings, advanced: bool) -> Vec<Row> {
    let Some(node) = catalogue::tree(advanced)
        .into_iter()
        .find(|n| n.section == section)
    else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for group in node.groups {
        let start = rows.len();
        if let Some(subsection) = group.subsection {
            rows.push(Row::Heading {
                section,
                subsection,
            });
        }
        let before = rows.len();
        let visible = |s: &&&Setting| settings.is_visible(s.key, &Target::Global).unwrap_or(false);
        rows.extend(
            group
                .settings
                .iter()
                .filter(visible)
                .map(|s| Row::Setting(s)),
        );
        rows.extend(group.commands.iter().map(|c| Row::Command(c)));
        if rows.len() == before {
            rows.truncate(start);
        }
    }
    rows
}

/// `(min, max, step)` of an integer or duration.
pub fn grid(ty: &ValueType) -> Option<(i64, i64, i64)> {
    match *ty {
        ValueType::Int { min, max, step, .. } => Some((min, max, step)),
        ValueType::Duration {
            min_ms,
            max_ms,
            step_ms,
            ..
        } => Some((min_ms, max_ms, step_ms)),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Sections,
    Rows,
}

/// An open value editor.
#[derive(Clone, Debug, PartialEq)]
pub enum Editor {
    /// An enum or colour: its variants, the focused one and the current one.
    Choice {
        setting: &'static Setting,
        focus: usize,
        selected: usize,
    },
    /// An integer or duration being moved by whole steps (not stored yet).
    Number {
        setting: &'static Setting,
        value: i64,
    },
    /// Text being typed; `invalid` after a rejected OK.
    Text {
        setting: &'static Setting,
        text: String,
        invalid: bool,
    },
}

impl Editor {
    pub fn setting(&self) -> &'static Setting {
        match self {
            Editor::Choice { setting, .. }
            | Editor::Number { setting, .. }
            | Editor::Text { setting, .. } => setting,
        }
    }
}

/// A remote key, as the screen sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Ok,
    Back,
    PageUp,
    PageDown,
}

/// What the app should do after a key or tap.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    None,
    /// Store `value` for `key` (globally).
    Set {
        key: &'static str,
        value: Value,
    },
    /// Run the command `key`.
    Run(&'static str),
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    section: Section,
    pane: Pane,
    focus: Option<&'static str>,
    editor: Option<Editor>,
    advanced: bool,
}

impl Screen {
    /// Opens on `section`, with the focus in the sections list.
    pub fn new(section: Section) -> Screen {
        Screen {
            section,
            pane: Pane::Sections,
            focus: None,
            editor: None,
            advanced: false,
        }
    }

    pub fn section(&self) -> Section {
        self.section
    }

    pub fn pane(&self) -> Pane {
        self.pane
    }

    /// The focused row's key (meaningful in [`Pane::Rows`]).
    pub fn focus(&self) -> Option<&'static str> {
        self.focus
    }

    pub fn editor(&self) -> Option<&Editor> {
        self.editor.as_ref()
    }

    pub fn advanced(&self) -> bool {
        self.advanced
    }

    pub fn rows(&self, settings: &Settings) -> Vec<Row> {
        rows(self.section, settings, self.advanced)
    }

    /// The row with the focus, if the rows pane has it.
    pub fn focused_row(&self, settings: &Settings) -> Option<Row> {
        let key = self.focus.filter(|_| self.pane == Pane::Rows)?;
        self.rows(settings)
            .into_iter()
            .find(|r| r.key() == Some(key))
    }

    pub fn key(&mut self, key: Key, ctx: &Ctx) -> Effect {
        if self.editor.is_some() {
            return self.editor_key(key);
        }
        match self.pane {
            Pane::Sections => self.sections_key(key, ctx),
            Pane::Rows => self.rows_key(key, ctx),
        }
    }

    fn sections_key(&mut self, key: Key, ctx: &Ctx) -> Effect {
        let at = Section::ALL
            .iter()
            .position(|s| *s == self.section)
            .unwrap_or(0);
        let to = match key {
            Key::Up => at.saturating_sub(1),
            Key::Down => at + 1,
            Key::PageUp => at.saturating_sub(PAGE),
            Key::PageDown => at + PAGE,
            Key::Right | Key::Ok => {
                self.enter_rows(ctx);
                return Effect::None;
            }
            Key::Back => return Effect::Close,
            Key::Left => at,
        };
        let to = Section::ALL[to.min(Section::ALL.len() - 1)];
        if to != self.section {
            self.section = to;
            self.focus = None;
        }
        Effect::None
    }

    /// Focus moves into the rows: the remembered row, else the first one
    /// the app acts on, else the first row.
    fn enter_rows(&mut self, ctx: &Ctx) {
        let rows = self.rows(ctx.settings);
        let keys: Vec<&'static str> = rows.iter().filter_map(Row::key).collect();
        if keys.is_empty() {
            return;
        }
        if self.focus.is_none_or(|k| !keys.contains(&k)) {
            let active = rows
                .iter()
                .find(|r| r.key().is_some() && ctx.status(r) == Status::Active);
            self.focus = active.and_then(Row::key).or(Some(keys[0]));
        }
        self.pane = Pane::Rows;
    }

    fn rows_key(&mut self, key: Key, ctx: &Ctx) -> Effect {
        let rows = self.rows(ctx.settings);
        let keys: Vec<&'static str> = rows.iter().filter_map(Row::key).collect();
        if keys.is_empty() {
            self.pane = Pane::Sections;
            return Effect::None;
        }
        // A focused row that left (its visibility changed) counts as the first.
        let at = self.focus.and_then(|k| keys.iter().position(|x| *x == k));
        let moved = |by: isize| {
            let i = at.unwrap_or(0) as isize + by;
            keys.get(i.clamp(0, keys.len() as isize - 1) as usize)
                .copied()
        };
        match key {
            Key::Up => self.focus = moved(-1),
            Key::Down => self.focus = moved(1),
            Key::PageUp => self.focus = moved(-(PAGE as isize)),
            Key::PageDown => self.focus = moved(PAGE as isize),
            Key::Left | Key::Back => self.pane = Pane::Sections,
            Key::Right => {}
            Key::Ok => {
                let row = at.and_then(|i| rows.iter().find(|r| r.key() == Some(keys[i])));
                return match row {
                    Some(row) => self.activate(*row, ctx),
                    None => Effect::None,
                };
            }
        }
        Effect::None
    }

    /// OK (or a tap) on `row`.
    fn activate(&mut self, row: Row, ctx: &Ctx) -> Effect {
        if ctx.status(&row) != Status::Active {
            return Effect::None;
        }
        let s = match row {
            Row::Setting(s) => s,
            Row::Command(c) => return Effect::Run(c.key),
            Row::Heading { .. } => return Effect::None,
        };
        let current = ctx.value(s);
        match (&s.ty, current) {
            (ValueType::Bool { .. }, Value::Bool(b)) => Effect::Set {
                key: s.key,
                value: Value::Bool(!b),
            },
            (ty, Value::Enum(v) | Value::Colour(v)) => {
                let variants = ty.choices().map_or(&[][..], |c| c.variants);
                let selected = variants.iter().position(|x| *x == v).unwrap_or(0);
                self.editor = Some(Editor::Choice {
                    setting: s,
                    focus: selected,
                    selected,
                });
                Effect::None
            }
            (_, Value::Int(value) | Value::Duration(value)) => {
                self.editor = Some(Editor::Number { setting: s, value });
                Effect::None
            }
            (_, Value::Text(text)) => {
                self.editor = Some(Editor::Text {
                    setting: s,
                    text,
                    invalid: false,
                });
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn editor_key(&mut self, key: Key) -> Effect {
        if key == Key::Back {
            self.cancel();
            return Effect::None;
        }
        match self.editor {
            Some(Editor::Choice { setting, focus, .. }) => {
                let last = setting.ty.choices().map_or(0, |c| c.variants.len()) as isize - 1;
                let by = match key {
                    Key::Up => -1,
                    Key::Down => 1,
                    Key::PageUp => -(PAGE as isize),
                    Key::PageDown => PAGE as isize,
                    Key::Ok => return self.pick(focus),
                    // Left/Right do nothing in a list (as in menus).
                    _ => 0,
                };
                let to = (focus as isize + by).clamp(0, last.max(0)) as usize;
                if let Some(Editor::Choice { focus, .. }) = &mut self.editor {
                    *focus = to;
                }
            }
            Some(Editor::Number { .. }) => match key {
                Key::Left | Key::Down => self.step(-1),
                Key::Right | Key::Up => self.step(1),
                Key::PageDown => self.step(-BIG_STEP),
                Key::PageUp => self.step(BIG_STEP),
                _ => return self.commit(),
            },
            // Typing belongs to the text field; OK keeps the text.
            Some(Editor::Text { .. }) if key == Key::Ok => return self.commit(),
            _ => {}
        }
        Effect::None
    }

    /// A tap on a section.
    pub fn tap_section(&mut self, section: Section) {
        self.editor = None;
        self.pane = Pane::Sections;
        if section != self.section {
            self.section = section;
            self.focus = None;
        }
    }

    /// A tap on a row: it takes the focus and acts as OK.
    pub fn tap_row(&mut self, key: &str, ctx: &Ctx) -> Effect {
        self.editor = None;
        let Some(row) = self
            .rows(ctx.settings)
            .into_iter()
            .find(|r| r.key() == Some(key))
        else {
            return Effect::None;
        };
        self.pane = Pane::Rows;
        self.focus = row.key();
        self.activate(row, ctx)
    }

    /// Picks entry `index` of an open choice list.
    pub fn pick(&mut self, index: usize) -> Effect {
        let Some(Editor::Choice { setting, .. }) = self.editor else {
            return Effect::None;
        };
        let variant = setting
            .ty
            .choices()
            .and_then(|c| c.variants.get(index).copied());
        let Some(variant) = variant else {
            return Effect::None;
        };
        self.editor = None;
        let value = match setting.ty {
            ValueType::Colour { .. } => Value::colour(variant),
            _ => Value::choice(variant),
        };
        Effect::Set {
            key: setting.key,
            value,
        }
    }

    /// Moves an open stepper by `steps` (negative: less), within range.
    pub fn step(&mut self, steps: i64) {
        if let Some(Editor::Number { setting, value }) = &mut self.editor
            && let Some((min, max, step)) = grid(&setting.ty)
        {
            *value = validate::clamp_to_grid(value.saturating_add(steps * step), min, max, step);
        }
    }

    pub fn set_text(&mut self, text: &str) {
        if let Some(Editor::Text {
            text: t, invalid, ..
        }) = &mut self.editor
        {
            text.clone_into(t);
            *invalid = false;
        }
    }

    /// Keeps the stepper's value or the typed text. A text that does not
    /// validate keeps the editor open, marked invalid.
    pub fn commit(&mut self) -> Effect {
        let (setting, value) = match &self.editor {
            Some(Editor::Number { setting, value }) => {
                let v = match setting.ty {
                    ValueType::Int { .. } => Value::Int(*value),
                    _ => Value::Duration(*value),
                };
                (*setting, v)
            }
            Some(Editor::Text { setting, text, .. }) => (*setting, Value::Text(text.clone())),
            Some(Editor::Choice { focus, .. }) => return self.pick(*focus),
            None => return Effect::None,
        };
        if validate::check(setting, &value).is_err() {
            if let Some(Editor::Text { invalid, .. }) = &mut self.editor {
                *invalid = true;
            }
            return Effect::None;
        }
        self.editor = None;
        Effect::Set {
            key: setting.key,
            value,
        }
    }

    /// Closes the editor without storing anything.
    pub fn cancel(&mut self) {
        self.editor = None;
    }
}
