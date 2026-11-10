//! Errors from writing settings and loading backups.

use crate::scope::Target;

/// Why a write was refused. Nothing is stored when a write fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsError {
    UnknownKey(String),
    TypeMismatch {
        key: &'static str,
        expected: &'static str,
        found: &'static str,
    },
    OutOfRange {
        key: &'static str,
        value: i64,
        min: i64,
        max: i64,
    },
    /// Inside the range but not on the `min + n * step` grid.
    OffStep {
        key: &'static str,
        value: i64,
        step: i64,
    },
    UnknownVariant {
        key: &'static str,
        variant: String,
    },
    TooLong {
        key: &'static str,
        len: usize,
        max: usize,
    },
    /// The text or binding list is well typed but malformed.
    InvalidFormat {
        key: &'static str,
        reason: String,
    },
    /// The setting cannot be overridden at this level.
    ScopeNotAllowed {
        key: &'static str,
        target: Target,
    },
}

impl core::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SettingsError::UnknownKey(k) => write!(f, "unknown setting {k:?}"),
            SettingsError::TypeMismatch {
                key,
                expected,
                found,
            } => write!(f, "{key}: expected {expected}, got {found}"),
            SettingsError::OutOfRange {
                key,
                value,
                min,
                max,
            } => write!(f, "{key}: {value} is outside {min}..={max}"),
            SettingsError::OffStep { key, value, step } => {
                write!(f, "{key}: {value} is not a multiple of the step {step}")
            }
            SettingsError::UnknownVariant { key, variant } => {
                write!(f, "{key}: {variant:?} is not a valid choice")
            }
            SettingsError::TooLong { key, len, max } => {
                write!(f, "{key}: {len} characters, at most {max} allowed")
            }
            SettingsError::InvalidFormat { key, reason } => write!(f, "{key}: {reason}"),
            SettingsError::ScopeNotAllowed { key, target } => {
                write!(f, "{key} cannot be overridden for {target}")
            }
        }
    }
}

impl std::error::Error for SettingsError {}

/// Why a backup document could not be loaded at all. Problems with single
/// entries never fail a load; they are listed in the
/// [`LoadReport`](crate::LoadReport).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// Not JSON, or not a JSON object.
    NotADocument(String),
    /// The `format` field is missing or names another format.
    WrongFormat,
    /// The `version` field is missing or not a positive integer.
    BadVersion,
    /// No migration leads from this version towards the current one.
    NoMigration { from: u32 },
    /// A `global` / `playlists` / `groups` member has the wrong shape.
    BadLayer(String),
}

impl core::fmt::Display for LoadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LoadError::NotADocument(why) => write!(f, "not a settings document: {why}"),
            LoadError::WrongFormat => f.write_str("not an iptv-rs settings document"),
            LoadError::BadVersion => f.write_str("missing or invalid version"),
            LoadError::NoMigration { from } => write!(f, "no migration from version {from}"),
            LoadError::BadLayer(path) => write!(f, "{path} is not an object of settings"),
        }
    }
}

impl std::error::Error for LoadError {}
