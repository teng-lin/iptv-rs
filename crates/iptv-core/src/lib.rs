//! The UI-independent core of iptv-rs. Everything here is plain Rust: no NSH,
//! no platform APIs, no blocking I/O, and it must build for
//! `wasm32-unknown-unknown` (see README).
//!
//! Today: the guide's synthetic schedule and the remote/touch focus model.
//! Planned (see `.plans/`): domain, providers, epg, nav, player-ctl,
//! settings and view models as separate crates.

pub mod channel_list;
pub mod clock;
pub mod editor;
pub mod guide;
pub mod menu;
pub mod panels;
pub mod playback;
pub mod player_info;
pub mod player_options;
pub mod reorder;
pub mod sound;
pub mod tracks;
pub mod wizard;
