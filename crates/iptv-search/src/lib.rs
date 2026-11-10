//! Search layer of iptv-rs.
//!
//! - [`tokens`] / [`Query`]: text folding shared with EPG matching (iptv-epg's
//!   [`normalize_name`](iptv_epg::normalize_name), word by word) and the one
//!   matcher every search uses: all query tokens must match, the last one
//!   may be the start of a word; quality is [`MatchQuality`].
//! - [`ChannelIndex`]: channel names (plus extra keys such as numbers),
//!   favourites first on request.
//! - [`ProgrammeIndex`]: an inverted index of programme titles from an
//!   [`iptv_epg::GuideStore`], with time window, catch-up and grouping.
//! - [`SearchHistory`]: recent queries, de-duplicated, saved as a string list.
//! - [`SearchState`]: the search screen (history, typing, Channels/Programs
//!   results, no results) with focus kept by result identity.
//!
//! No NSH, no platform APIs, no I/O and no clock: `now` comes from the
//! caller. Builds for `wasm32-unknown-unknown`.

mod channels;
mod programmes;
mod text;

pub use channels::{ChannelHit, ChannelIndex, ChannelSearchOptions};
pub use programmes::{GroupBy, ProgrammeHit, ProgrammeIndex, ProgrammeSearchOptions};
pub use text::{MIN_INFIX, MatchQuality, Query, fold, tokens};
