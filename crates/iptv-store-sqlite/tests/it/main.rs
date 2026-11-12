//! The store's integration tests.
//!
//! One test binary for the crate's integration tests (`tests/it/`, one
//! module per topic): each extra binary costs a link. Tests here share
//! a process, so none may rely on process-wide state (environment,
//! working directory, a global allocator); such a test is a binary of its
//! own in `tests/`: `epg_memory` and `epg_stream_memory`, which count
//! allocations through a global allocator. The helper modules stay beside
//! them in `tests/` and are shared.

#[path = "../common/mod.rs"]
mod common;
#[path = "../epg_synth/mod.rs"]
mod epg_synth;
#[path = "../staged/mod.rs"]
mod staged;
#[path = "../stream_support/mod.rs"]
mod stream_support;

mod catalog;
mod concurrency;
mod epg;
mod epg_stream;
mod guide_assign;
mod library_preferences;
mod migrations;
mod organise;
mod overlays;
mod perf;
mod personalization;
mod playlists;
mod redaction;
mod search;
