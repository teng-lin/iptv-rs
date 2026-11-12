#![forbid(unsafe_code)]
//! Import pipeline of iptv-rs.
//!
//! Drives the sans-IO provider clients (`iptv-providers`) and the XMLTV
//! parser (`iptv-epg`) through an abstract async [`Fetch`]:
//!
//! - [`import_source`]: M3U (URL or file via the adapter), Xtream (account
//!   → live categories + streams, optional VOD/series) or Stalker
//!   (discovery → auth loop → genres → paginated channels) into a
//!   provider-neutral [`Catalog`]. [`import_m3u_text`] parses a playlist
//!   already in memory.
//! - [`import_epg`]: chunked XMLTV download per source with priorities,
//!   programmes streamed into a [`GuideSink`] as they are parsed (only the
//!   feeds' channel lists stay in memory), matched against the catalog's
//!   channels. [`MemoryGuide`] is an in-memory sink.
//!   [`Catalog::epg_sources`] turns the M3U header's `url-tvg`/`x-tvg-url`
//!   (or the Xtream export) into [`EpgSourceConfig`]s.
//! - [`decide`] / [`plan_startup`]: when to download a stored guide again
//!   ([`RefreshPolicy`] from the `epg.update_*` settings).
//!
//! Failures are typed ([`ImportError`], [`FailureKind`]) so a failure never
//! looks like an empty source. Progress goes to a [`ProgressSink`];
//! [`CancelToken`] stops an import between requests, between chunks and
//! while a request is in flight.
//!
//! **Portable.** No runtime (no tokio), no NSH, no platform APIs, no clock
//! (callers pass `now`); builds for `wasm32-unknown-unknown`. The native
//! adapter is `iptv-net`.

mod cancel;
mod catalog;
mod ctx;
mod epg;
mod error;
mod fetch;
mod link;
mod m3u;
mod progress;
mod refresh;
mod sink;
mod stalker;
mod xtream;

pub use cancel::{CancelToken, Cancelled};
pub use catalog::{Catalog, CatalogWarning, ImportOptions, ImportStats, SourceMeta};
pub use epg::{
    EpgImport, EpgSourceConfig, EpgSourceOutcome, EpgSourceStats, ListedChannel, import_epg,
};
pub use error::{AuthProblem, FailureKind, ImportError, ParseProblem, Step};
pub use fetch::{BodyStream, Fetch, FetchError, FetchErrorKind, MemoryBody, StreamingResponse};
pub use link::stalker_link;
pub use m3u::import_m3u_text;
pub use progress::{NoProgress, Phase, Progress, ProgressSink};
pub use refresh::{
    Decision, GuideState, RefreshPolicy, RefreshReason, StartupPlan, Trigger, decide,
    matching_fingerprint, plan_startup, store_options,
};
pub use sink::{GuideSink, MemoryGuide, SinkError};

use iptv_providers::model::SourceConfig;

/// Imports one configured source.
///
/// Network work happens only through `fetch`; `progress` receives events on
/// the calling task; `cancel` aborts with [`ImportError::Cancelled`].
pub async fn import_source<F: Fetch, P: ProgressSink>(
    fetch: &F,
    config: &SourceConfig,
    options: &ImportOptions,
    progress: &mut P,
    cancel: &CancelToken,
) -> Result<Catalog, ImportError> {
    let mut ctx = ctx::Ctx::new(fetch, progress, cancel);
    ctx.check()?;
    match config {
        SourceConfig::M3u(c) => m3u::import_m3u(&mut ctx, c).await,
        SourceConfig::Xtream(c) => xtream::import_xtream(&mut ctx, c, options).await,
        SourceConfig::Stalker(c) => stalker::import_stalker(&mut ctx, c, options).await,
    }
}
