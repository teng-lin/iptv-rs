//! The UI-independent core of iptv-rs. Everything here is plain Rust: no NSH,
//! no platform APIs, no blocking I/O, and it must build for
//! `wasm32-unknown-unknown` (see README).
//!
//! Today: the guide's synthetic schedule, the remote/touch focus model and
//! its columns (rail, groups, lists), favourites, menus, panels, playback.
//! Planned (see `.plans/`): domain, providers, epg, nav, player-ctl,
//! settings and view models as separate crates.

pub mod browse;
pub mod channel_list;
pub mod clock;
pub mod editor;
pub mod favorites;
pub mod guide;
pub mod lists;
pub mod menu;
pub mod multiview;
pub mod panels;
pub mod pip;
pub mod pip_window;
pub mod play_bar;
pub mod playback;
pub mod player_info;
pub mod player_options;
pub mod reorder;
pub mod seek_hold;
pub mod sound;
pub mod timeline;
pub mod tracks;
pub mod wizard;

pub mod organization;
