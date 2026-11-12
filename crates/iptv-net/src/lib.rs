#![forbid(unsafe_code)]
//! Native network adapter of iptv-rs.
//!
//! [`NetClient`] performs `iptv_providers::HttpRequest`s with reqwest and
//! implements [`iptv_import::Fetch`] (buffered [`NetClient::fetch`] and
//! chunked [`NetClient::fetch_stream`]).
//!
//! - **TLS:** rustls with the ring provider and the Mozilla root set
//!   (webpki-roots), plus optional extra roots. No OpenSSL, no aws-lc, no
//!   platform verifier, so it cross-compiles for Android with only the NDK
//!   clang (for ring) and needs no JNI initialisation. User-installed
//!   Android CAs are therefore not trusted (pass them as extra roots).
//! - **HTTP:** HTTP/1.1; `gzip`/`deflate` `Content-Encoding` decoded
//!   transparently (a `.gz` *file* is passed through for the parsers to
//!   detect); up to [`NetConfig::max_redirects`] redirects, including
//!   http↔https. A redirect to another origin drops `Cookie`,
//!   `Authorization` and `SN`, and no `Referer` is ever added (it would
//!   carry Xtream credentials from the query), so portal identity never
//!   leaks to a CDN.
//! - **Headers:** sent exactly as the request carries them (User-Agent,
//!   Cookie, Referer…). [`NetConfig::user_agent`] applies only when the
//!   request has none. No cookie store: `Set-Cookie` is ignored.
//! - **Timeouts:** connect, idle, and total import/response deadlines; streamed
//!   and buffered bodies both enforce byte budgets.
//! - **Local files:** `file://` URLs and absolute paths are read with tokio
//!   (picked playlists), unless [`NetConfig::allow_files`] is off.
//! - **Redaction:** logs use `HttpRequest::redacted_url`; errors never carry
//!   URLs (`reqwest::Error::without_url`) or bodies.
//!
//! Needs a tokio runtime (reqwest's I/O driver). Not part of the portable
//! core.

mod body;
mod client;
mod error;
mod file;
mod redirect;
mod tls;

pub use body::NetBody;
pub use client::{NetClient, NetConfig};
pub use iptv_import::{FetchError, FetchErrorKind, StreamingResponse};

mod public_resolver;
pub use public_resolver::PublicResolver;
