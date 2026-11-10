//! The settings layer of iptv-rs: one declarative schema that drives
//! storage, backup/restore, the settings UI and validation.
//!
//! - [`schema`]: what a setting is — stable key, section/subsection, value
//!   type with its default, [`Scope`], [`Flags`], dependencies, and
//!   i18n label keys derived from the key.
//! - [`catalogue`]: every setting and command of iptv-rs, plus the UI
//!   [`tree`](catalogue::tree).
//!
//! Plain Rust: no NSH, no platform APIs, no I/O (callers persist the JSON
//! text), and it builds for `wasm32-unknown-unknown`.

pub mod catalogue;
mod error;
pub mod schema;
mod scope;
pub mod validate;
mod value;

pub use error::{LoadError, SettingsError};
pub use schema::{Flags, Section, Setting, ValueType};
pub use scope::{Scope, Target};
pub use value::{KeyBinding, PressKind, Value};
