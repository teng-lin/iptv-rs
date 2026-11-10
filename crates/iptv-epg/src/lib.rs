//! EPG layer of iptv-rs.
//!
//! - [`XmltvParser`]: sans-IO, incremental XMLTV parser. Feed it bytes in
//!   any chunking (plain or gzip); it yields [`XmltvChannel`] and
//!   [`XmltvProgramme`] records, skipping malformed entries with counted
//!   [`ParseWarnings`].
//! - [`MatchIndex`]: maps playlist channels ([`ChannelKey`]) to EPG channel
//!   ids by tvg-id, then by normalised name, with manual [`Override`]s.
//!
//! No NSH, no platform APIs, no clock, no blocking I/O; builds for
//! `wasm32-unknown-unknown`. Times are unix seconds ([`UnixTime`]) and every
//! time-dependent call takes `now` from the caller.

mod decode;
mod matching;
mod normalize;
mod parser;
mod time;
mod xmltv;

pub use decode::{Compression, DecodeError};
pub use matching::{
    ChannelKey, EpgRef, MatchIndex, MatchKind, MatchReport, MatchResult, Override, Overrides,
    PlaylistChannel, SourceId,
};
pub use normalize::{normalize_name, strip_tag_prefix};
pub use parser::{ParseOptions, ParseStats, ParseWarnings, ParsedXmltv, XmltvParser, parse_xmltv};
pub use time::{DAY, TimePolicy, UnixTime, format_xmltv_time, parse_xmltv_time};
pub use xmltv::{EpisodeNum, Rating, XmltvChannel, XmltvItem, XmltvProgramme};
