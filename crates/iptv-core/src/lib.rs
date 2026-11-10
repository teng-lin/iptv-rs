//! The UI-independent core of iptv-rs. Everything here is plain Rust: no NSH,
//! no platform APIs, no blocking I/O, and it must build for
//! `wasm32-unknown-unknown` (see README).
//!
//! Today: the guide's synthetic schedule and the remote/touch focus model.
//! Planned (see `.plans/`): domain, providers, epg, nav, player-ctl,
//! settings and view models as separate crates.

pub mod guide;
pub mod panels;
