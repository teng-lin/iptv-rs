//! Provider layer of iptv-rs: M3U/M3U8 playlists, Xtream Codes panels and
//! Stalker/Ministra portals, mapped to one provider-neutral model.
//!
//! **Sans-IO.** Clients build [`http::HttpRequest`] descriptions and parse
//! [`http::HttpResponse`] bodies; an adapter crate does the networking
//! (reqwest natively, `fetch` on the web). No NSH, no platform APIs, no
//! blocking, no clock: callers pass "now". Builds for
//! `wasm32-unknown-unknown`.
//!
//! **Tolerant.** Bad playlist lines and malformed catalog entries are skipped
//! with a warning ([`model::Import`]); they never abort an import. Fields
//! that panels send as numbers or strings, `null`, `""`, `"N/A"` or PHP's
//! `[]` become `None` instead of failing.
//!
//! **Stable ids.** See [`id`] for the rule that lets favourites, order and
//! hidden flags survive provider refreshes.
//!
//! **No secret leaks.** Credentials, tokens and MACs are wrapped in types
//! whose `Debug`/`Display` redact; requests and responses redact in `Debug`;
//! errors never quote bodies.
//!
//! Behaviour follows IPTVnator's documented provider compatibility rules,
//! rewritten in Rust.
//!
//! Known assumption: Stalker `tv_archive_duration` is read as hours
//! (converted to whole days, rounded up).

mod b64;
pub mod error;
pub mod http;
pub mod id;
mod lenient;
pub mod m3u;
pub mod model;
pub mod secret;
pub mod time;
mod urlenc;
pub mod xtream;

pub use error::{ProviderError, StalkerError, StalkerErrorKind};
pub use http::{HttpRequest, HttpResponse, Method};
pub use id::{ContentId, GroupId};
pub use secret::{Secret, SecretUrl};
