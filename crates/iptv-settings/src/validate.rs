//! Validation of values against their setting's type.

use std::collections::BTreeSet;

use crate::error::SettingsError;
use crate::schema::{Setting, TextKind, ValueType};
use crate::value::{KeyBinding, Value};

/// Longest key, context or action token in a binding.
pub const MAX_TOKEN_LEN: usize = 32;

/// Checks type, range, step, membership and format. Dependencies are not
/// checked: a disabled setting keeps (and accepts) its value.
pub fn check(setting: &Setting, value: &Value) -> Result<(), SettingsError> {
    let key = setting.key;
    let mismatch = || SettingsError::TypeMismatch {
        key,
        expected: setting.ty.type_name(),
        found: value.type_name(),
    };
    match (&setting.ty, value) {
        (ValueType::Bool { .. }, Value::Bool(_)) => Ok(()),
        (ValueType::Int { min, max, step, .. }, Value::Int(v)) => {
            check_grid(key, *v, *min, *max, *step)
        }
        (
            ValueType::Duration {
                min_ms,
                max_ms,
                step_ms,
                ..
            },
            Value::Duration(v),
        ) => check_grid(key, *v, *min_ms, *max_ms, *step_ms),
        (ValueType::Enum { choices, .. }, Value::Enum(v))
        | (
            ValueType::Colour {
                palette: choices, ..
            },
            Value::Colour(v),
        ) => {
            if choices.contains(v) {
                Ok(())
            } else {
                Err(SettingsError::UnknownVariant {
                    key,
                    variant: v.clone(),
                })
            }
        }
        (ValueType::Text { kind, max_len, .. }, Value::Text(v)) => {
            check_text(key, *kind, *max_len, v)
        }
        (ValueType::KeyBindings { max_len }, Value::KeyBindings(list)) => {
            check_bindings(key, *max_len, list)
        }
        _ => Err(mismatch()),
    }
}

fn check_grid(
    key: &'static str,
    value: i64,
    min: i64,
    max: i64,
    step: i64,
) -> Result<(), SettingsError> {
    if value < min || value > max {
        return Err(SettingsError::OutOfRange {
            key,
            value,
            min,
            max,
        });
    }
    if (value - min) % step != 0 {
        return Err(SettingsError::OffStep { key, value, step });
    }
    Ok(())
}

/// Brings a number into `min..=max` and down onto the step grid. Used only
/// when loading backups (see [`crate::backup`]).
pub fn clamp_to_grid(value: i64, min: i64, max: i64, step: i64) -> i64 {
    let v = value.clamp(min, max);
    min + (v - min) / step * step
}

fn invalid(key: &'static str, reason: impl Into<String>) -> SettingsError {
    SettingsError::InvalidFormat {
        key,
        reason: reason.into(),
    }
}

fn check_text(
    key: &'static str,
    kind: TextKind,
    max_len: usize,
    text: &str,
) -> Result<(), SettingsError> {
    let len = text.chars().count();
    if len > max_len {
        return Err(SettingsError::TooLong {
            key,
            len,
            max: max_len,
        });
    }
    if text.chars().any(char::is_control) {
        return Err(invalid(key, "contains control characters"));
    }
    match kind {
        TextKind::Plain | TextKind::CommaList | TextKind::Path => Ok(()),
        TextKind::HostPort => {
            if text.is_empty() || is_host_port(text) {
                Ok(())
            } else {
                Err(invalid(key, "expected host:port"))
            }
        }
        TextKind::Locale => {
            if text == "system" || is_language_tag(text) {
                Ok(())
            } else {
                Err(invalid(key, "expected \"system\" or a language tag"))
            }
        }
    }
}

fn is_host_port(text: &str) -> bool {
    let Some((host, port)) = text.rsplit_once(':') else {
        return false;
    };
    !host.is_empty()
        && !host.chars().any(char::is_whitespace)
        && port.parse::<u16>().is_ok_and(|p| p > 0)
}

/// `ll`, `lll`, then `-` subtags of 1–8 alphanumerics.
fn is_language_tag(text: &str) -> bool {
    let mut parts = text.split('-');
    let lang = parts.next().unwrap_or_default();
    (2..=3).contains(&lang.len())
        && lang.chars().all(|c| c.is_ascii_lowercase())
        && parts.all(|p| (1..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// `[a-z][a-z0-9_]*`, at most [`MAX_TOKEN_LEN`] characters.
pub fn is_token(token: &str) -> bool {
    let mut chars = token.chars();
    token.len() <= MAX_TOKEN_LEN
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn check_bindings(
    key: &'static str,
    max_len: usize,
    list: &[KeyBinding],
) -> Result<(), SettingsError> {
    if list.len() > max_len {
        return Err(SettingsError::TooLong {
            key,
            len: list.len(),
            max: max_len,
        });
    }
    let mut seen = BTreeSet::new();
    for b in list {
        let tokens = [b.context.as_deref(), Some(&b.key), Some(&b.action)];
        if let Some(bad) = tokens.into_iter().flatten().find(|t| !is_token(t)) {
            return Err(invalid(key, format!("{bad:?} is not a valid name")));
        }
        if !seen.insert((&b.context, &b.key, b.press)) {
            return Err(invalid(
                key,
                format!("{} {} is bound twice", b.key, b.press.key()),
            ));
        }
    }
    Ok(())
}
