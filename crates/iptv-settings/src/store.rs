//! The settings store: explicit values per level over the schema defaults.
//!
//! Rules:
//!
//! - Only explicit choices are stored; an unset key resolves to the next
//!   level, finally to the schema default. A value equal to the default is
//!   still an explicit choice and survives a default change in a later
//!   version.
//! - Writes validate type, range, step, membership, format and scope, and
//!   store nothing on failure. Dependencies do not block writes: a hidden
//!   or disabled setting keeps its value.
//! - Every mutation returns the [`Change`]s it caused: the key and the
//!   level whose resolved value differs. A change at a level also affects
//!   the levels that inherit from it (a global change reaches every
//!   playlist and group without an override); consumers re-resolve.

use std::collections::BTreeMap;

use crate::catalogue;
use crate::error::SettingsError;
use crate::keys::{BindingsKey, BoolKey, ColourKey, DurationKey, EnumKey, IntKey, TextKey};
use crate::schema::{Effect, Section, Setting, Test, When};
use crate::scope::Target;
use crate::validate;
use crate::value::{KeyBinding, Value};

/// Values stored at one level.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Layer {
    pub values: BTreeMap<&'static str, Value>,
    /// Entries kept verbatim from a loaded backup: unknown keys, and known
    /// keys whose value was rejected. They are written back by
    /// [`Settings::to_json`] until the key is set or reset here.
    pub raw: BTreeMap<String, serde_json::Value>,
}

impl Layer {
    fn is_empty(&self) -> bool {
        self.values.is_empty() && self.raw.is_empty()
    }
}

/// One effect of a mutation: `key` now resolves differently at `target`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub key: &'static str,
    pub target: Target,
}

/// The settings store. Cheap to clone; holds only explicit values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub(crate) layers: BTreeMap<Target, Layer>,
    /// Unknown top-level members of a loaded backup, written back as-is.
    pub(crate) extra: BTreeMap<String, serde_json::Value>,
}

fn lookup(key: &str) -> Result<&'static Setting, SettingsError> {
    catalogue::setting(key).ok_or_else(|| SettingsError::UnknownKey(key.to_owned()))
}

impl Settings {
    /// Every setting at its default.
    pub fn new() -> Settings {
        Settings::default()
    }

    /// The global value of `key`.
    pub fn get(&self, key: &str) -> Result<Value, SettingsError> {
        self.get_in(key, &Target::Global)
    }

    /// The value of `key` at `at`: group → playlist → global → default,
    /// skipping levels the setting's scope does not allow.
    pub fn get_in(&self, key: &str, at: &Target) -> Result<Value, SettingsError> {
        Ok(self.resolve(lookup(key)?, at))
    }

    /// The level that supplies `key`'s value at `at`; `None` when it is
    /// the default. Lets the UI mark overridden values.
    pub fn source(&self, key: &str, at: &Target) -> Result<Option<Target>, SettingsError> {
        let s = lookup(key)?;
        Ok(self.find(s, at).map(|(t, _)| t))
    }

    /// The explicit value stored at exactly `at`, if any.
    pub fn stored(&self, key: &str, at: &Target) -> Option<&Value> {
        self.layers.get(at)?.values.get(key)
    }

    fn find(&self, s: &Setting, at: &Target) -> Option<(Target, &Value)> {
        at.chain()
            .into_iter()
            .filter(|t| s.scope.allows(t))
            .find_map(|t| {
                let v = self.layers.get(&t)?.values.get(s.key)?;
                Some((t, v))
            })
    }

    pub(crate) fn resolve(&self, s: &Setting, at: &Target) -> Value {
        self.find(s, at)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| s.default_value())
    }

    /// Sets the global value.
    pub fn set(&mut self, key: &str, value: Value) -> Result<Vec<Change>, SettingsError> {
        self.set_in(key, &Target::Global, value)
    }

    /// Sets the value at `at` (an override unless `at` is global).
    pub fn set_in(
        &mut self,
        key: &str,
        at: &Target,
        value: Value,
    ) -> Result<Vec<Change>, SettingsError> {
        let s = lookup(key)?;
        if !s.scope.allows(at) {
            return Err(SettingsError::ScopeNotAllowed {
                key: s.key,
                target: at.clone(),
            });
        }
        validate::check(s, &value)?;
        let before = self.resolve(s, at);
        let layer = self.layers.entry(at.clone()).or_default();
        layer.raw.remove(s.key);
        layer.values.insert(s.key, value);
        Ok(self.changed(s, at, &before))
    }

    fn changed(&self, s: &Setting, at: &Target, before: &Value) -> Vec<Change> {
        if self.resolve(s, at) == *before {
            Vec::new()
        } else {
            vec![Change {
                key: s.key,
                target: at.clone(),
            }]
        }
    }

    /// Removes the value stored at exactly `at`, so `at` inherits again
    /// (for [`Target::Global`]: falls back to the default).
    pub fn clear_in(&mut self, key: &str, at: &Target) -> Result<Vec<Change>, SettingsError> {
        let s = lookup(key)?;
        let before = self.resolve(s, at);
        self.remove_at(s.key, at);
        Ok(self.changed(s, at, &before))
    }

    fn remove_at(&mut self, key: &str, at: &Target) {
        if let Some(layer) = self.layers.get_mut(at) {
            layer.values.remove(key);
            layer.raw.remove(key);
            if layer.is_empty() {
                self.layers.remove(at);
            }
        }
    }

    /// Resets `key` to its default everywhere: the global value and every
    /// override.
    pub fn reset(&mut self, key: &str) -> Result<Vec<Change>, SettingsError> {
        Ok(self.reset_setting(lookup(key)?))
    }

    fn reset_setting(&mut self, s: &Setting) -> Vec<Change> {
        let targets: Vec<Target> = self
            .layers
            .iter()
            .filter(|(_, l)| l.values.contains_key(s.key) || l.raw.contains_key(s.key))
            .map(|(t, _)| t.clone())
            .collect();
        let before: Vec<Value> = targets.iter().map(|t| self.resolve(s, t)).collect();
        for t in &targets {
            self.remove_at(s.key, t);
        }
        targets
            .into_iter()
            .zip(before)
            .filter(|(t, b)| self.resolve(s, t) != *b)
            .map(|(target, _)| Change { key: s.key, target })
            .collect()
    }

    /// Resets every setting of `section` everywhere.
    pub fn reset_section(&mut self, section: Section) -> Vec<Change> {
        catalogue::section(section)
            .iter()
            .flat_map(|s| self.reset_setting(s))
            .collect()
    }

    /// Resets every setting everywhere and drops everything preserved from
    /// backups (unknown keys and members included).
    pub fn reset_all(&mut self) -> Vec<Change> {
        let changes = catalogue::settings()
            .flat_map(|s| self.reset_setting(s))
            .collect();
        self.layers.clear();
        self.extra.clear();
        changes
    }

    /// Drops a deleted playlist's overrides and its groups' overrides.
    pub fn remove_playlist(&mut self, playlist: &str) {
        self.layers.retain(|t, _| match t {
            Target::Playlist(p) | Target::Group(p, _) => p != playlist,
            Target::Global => true,
        });
    }

    /// Drops a deleted group's overrides.
    pub fn remove_group(&mut self, playlist: &str, group: &str) {
        self.layers
            .remove(&Target::Group(playlist.to_owned(), group.to_owned()));
    }

    /// Whether the settings UI shows `key` at `at`: its visibility
    /// conditions hold and every setting they refer to is visible itself.
    pub fn is_visible(&self, key: &str, at: &Target) -> Result<bool, SettingsError> {
        Ok(self.passes(lookup(key)?, Effect::Visible, at, 0))
    }

    /// Whether `key` is visible and editable at `at`.
    pub fn is_enabled(&self, key: &str, at: &Target) -> Result<bool, SettingsError> {
        let s = lookup(key)?;
        Ok(self.passes(s, Effect::Visible, at, 0) && self.passes(s, Effect::Enabled, at, 0))
    }

    fn passes(&self, s: &Setting, effect: Effect, at: &Target, depth: usize) -> bool {
        // The schema test proves dependencies are acyclic; the bound is a
        // backstop.
        depth < 16
            && s.depends
                .iter()
                .filter(|d| d.effect == effect)
                .all(|d| d.any_of.iter().any(|w| self.holds(w, effect, at, depth)))
    }

    fn holds(&self, w: &When, effect: Effect, at: &Target, depth: usize) -> bool {
        let Some(s) = catalogue::setting(w.key) else {
            return false;
        };
        if !self.passes(s, effect, at, depth + 1) {
            return false;
        }
        match (w.test, self.resolve(s, at)) {
            (Test::IsTrue, Value::Bool(b)) => b,
            (Test::IsFalse, Value::Bool(b)) => !b,
            (Test::EnumIn(variants), Value::Enum(v) | Value::Colour(v)) => {
                variants.contains(&v.as_str())
            }
            (Test::NotEmpty, Value::Text(t)) => !t.is_empty(),
            (Test::NonZero, Value::Int(n) | Value::Duration(n)) => n != 0,
            _ => false,
        }
    }

    // Typed reads. The keys in [`crate::keys`] are checked against the
    // schema by the test suite, so a mismatch here is a bug in this crate.

    fn typed(&self, key: &'static str, at: &Target) -> Value {
        self.get_in(key, at)
            .unwrap_or_else(|e| panic!("typed settings key: {e}"))
    }

    pub fn bool(&self, key: BoolKey, at: &Target) -> bool {
        match self.typed(key.0, at) {
            Value::Bool(b) => b,
            v => mismatch(key.0, &v),
        }
    }

    pub fn int(&self, key: IntKey, at: &Target) -> i64 {
        match self.typed(key.0, at) {
            Value::Int(n) => n,
            v => mismatch(key.0, &v),
        }
    }

    /// Milliseconds (may be negative for offsets).
    pub fn duration_ms(&self, key: DurationKey, at: &Target) -> i64 {
        match self.typed(key.0, at) {
            Value::Duration(ms) => ms,
            v => mismatch(key.0, &v),
        }
    }

    /// The variant key, as the schema's `'static` string.
    pub fn choice(&self, key: EnumKey, at: &Target) -> &'static str {
        self.interned(key.0, at)
    }

    /// The palette key, as the schema's `'static` string.
    pub fn colour(&self, key: ColourKey, at: &Target) -> &'static str {
        self.interned(key.0, at)
    }

    fn interned(&self, key: &'static str, at: &Target) -> &'static str {
        let s = lookup(key).unwrap_or_else(|e| panic!("typed settings key: {e}"));
        match (s.ty.choices(), self.resolve(s, at)) {
            (Some(c), Value::Enum(v) | Value::Colour(v)) => {
                c.intern(&v).expect("stored variants are validated")
            }
            (_, v) => mismatch(key, &v),
        }
    }

    pub fn text(&self, key: TextKey, at: &Target) -> String {
        match self.typed(key.0, at) {
            Value::Text(t) => t,
            v => mismatch(key.0, &v),
        }
    }

    pub fn key_bindings(&self, key: BindingsKey, at: &Target) -> Vec<KeyBinding> {
        match self.typed(key.0, at) {
            Value::KeyBindings(list) => list,
            v => mismatch(key.0, &v),
        }
    }
}

fn mismatch(key: &str, value: &Value) -> ! {
    panic!("typed settings key {key} holds a {}", value.type_name())
}
