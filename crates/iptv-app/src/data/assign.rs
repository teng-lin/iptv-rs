//! Choosing a channel's guide by hand (Long OK menu → "Choose guide
//! data…"): automatic matching, no guide, or one channel of the stored
//! guides. The choice is a channel overlay in the store (by channel key,
//! kept across refreshes) that every later match applies
//! (`stored::overrides`).
//!
//! It shows at once when the stored guide allows it: no guide, or a guide
//! channel whose programmes are stored (another playlist channel uses it).
//! Otherwise (a guide channel nothing used yet, or back to automatic,
//! which needs the full channel lists) the playlist's guide is downloaded
//! right away and matched with the choice.

use std::sync::mpsc::{self, TryRecvError};

use iptv_epg::{ChannelKey, MatchKind};
use iptv_providers::ContentId;
use iptv_store_sqlite::{EpgOverride, GuideChannel, SourceId, StoreError};
use nsh_core::prelude::timer;

use super::stored::{self, Matching};
use super::{GENERATION, Imported, POLL, Streams, open_store};
use crate::Ui;

/// A channel's guide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Matched by its guide id and name (no choice saved).
    Automatic,
    NoGuide,
    Channel {
        feed: iptv_epg::SourceId,
        epg_channel: String,
    },
}

impl Choice {
    fn overlay(&self) -> Option<EpgOverride> {
        match self {
            Self::Automatic => None,
            Self::NoGuide => Some(EpgOverride::Disabled),
            Self::Channel { feed, epg_channel } => {
                Some(EpgOverride::for_feed(*feed, epg_channel.clone()))
            }
        }
    }

    fn of(overlay: Option<EpgOverride>) -> Self {
        match overlay {
            None => Self::Automatic,
            Some(EpgOverride::Disabled) => Self::NoGuide,
            Some(o @ EpgOverride::Xmltv { .. }) => match (o.feed(), o) {
                (Some(feed), EpgOverride::Xmltv { epg_channel, .. }) => {
                    Self::Channel { feed, epg_channel }
                }
                _ => Self::Automatic,
            },
        }
    }
}

/// When a choice shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    /// The guide on screen shows it already.
    Now,
    /// The guide is downloading to apply it.
    Downloading,
    /// At the next guide update (nothing to download now).
    Later,
    /// It could not be saved.
    Failed,
}

/// What a channel's guide is now: the saved choice and the guide channel
/// its stored programmes come from (with how it was matched).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Current {
    pub choice: Option<Choice>,
    pub bound: Option<(iptv_epg::SourceId, String, MatchKind)>,
}

/// The saved choice and the binding of channel `key` of `source`.
pub fn current(source: SourceId, key: &str) -> Current {
    let read = open_store().and_then(|s| {
        let overlay = s
            .channel(source, &ContentId::from_stored(key))?
            .and_then(|row| row.epg_override);
        let bound = s.epg_binding(source, &ChannelKey::new(key))?;
        Ok(Current {
            choice: Some(Choice::of(overlay)),
            bound,
        })
    });
    read.unwrap_or_else(|e: StoreError| {
        log::warn!("iptv data: a channel's guide: {e}");
        Current::default()
    })
}

/// The channels the stored guides of `source` list.
pub fn guide_channels(source: SourceId) -> Vec<GuideChannel> {
    open_store()
        .and_then(|s| s.guide_channels(source))
        .unwrap_or_else(|e| {
            log::warn!("iptv data: guide channels: {e}");
            Vec::new()
        })
}

/// Saves `choice` for channel `key` of `source` (the playlist on screen)
/// and applies it; `done` hears when it shows.
pub fn choose(
    ui: Ui,
    source: SourceId,
    key: String,
    choice: Choice,
    done: impl FnOnce(Applied) + 'static,
) {
    super::library::revoke_import_handoff(source);
    let generation = GENERATION.with(std::cell::Cell::get);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(save_and_rebind(source, &key, &choice));
    });
    super::spawn_in_app(async move {
        let outcome = loop {
            match rx.try_recv() {
                Ok(outcome) => break outcome,
                Err(TryRecvError::Empty) => timer(POLL).await,
                Err(TryRecvError::Disconnected) => break Err(()),
            }
        };
        let applied = match outcome {
            Err(()) => Applied::Failed,
            Ok(Some(imported)) => {
                super::show(ui, imported, generation, false);
                Applied::Now
            }
            Ok(None) if stored::update_source(ui, source, || {}) => Applied::Downloading,
            Ok(None) => Applied::Later,
        };
        log::info!("iptv data: a channel's guide was chosen ({applied:?})");
        done(applied);
    });
}

/// On a worker thread: saves the choice, and when the stored guide can
/// show it, rebinds it and reads the guide back (`Some`); `None`: it needs
/// a download.
fn save_and_rebind(
    source: SourceId,
    key: &str,
    choice: &Choice,
) -> Result<Option<Box<Imported>>, ()> {
    let fail = |e: StoreError| log::warn!("iptv data: could not save a channel's guide: {e}");
    let mut store = open_store().map_err(fail)?;
    store
        .set_epg_override(
            source,
            &ContentId::from_stored(key),
            choice.overlay().as_ref(),
        )
        .map_err(fail)?;
    let to = match choice {
        Choice::Automatic => return Ok(None),
        Choice::NoGuide => None,
        Choice::Channel { feed, epg_channel } => Some((*feed, epg_channel.as_str())),
    };
    let (groups, channels) = stored::catalog(source).map_err(fail)?;
    let overrides = stored::overrides(source);
    let matching = Matching {
        channels: &channels,
        overrides: &overrides,
    };
    let rebound = store
        .rebind_epg(source, &ChannelKey::new(key), to, matching.fingerprint())
        .map_err(fail)?;
    if !rebound {
        return Ok(None);
    }
    let now = super::now_unix();
    Ok(Some(Box::new(Imported {
        guide: stored::load_guide(source, now).unwrap_or_default(),
        groups,
        channels,
        playback: Streams::Current,
        now,
        source,
        resume: false,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_round_trip_through_the_overlay() {
        let feed = iptv_epg::SourceId(1001);
        for choice in [
            Choice::Automatic,
            Choice::NoGuide,
            Choice::Channel {
                feed,
                epg_channel: "news.test".into(),
            },
        ] {
            assert_eq!(Choice::of(choice.overlay()), choice);
        }
        // A key that names no feed (not written by this app) reads as
        // automatic.
        let other = EpgOverride::Xmltv {
            epg_source: "elsewhere".into(),
            epg_channel: "x".into(),
        };
        assert_eq!(Choice::of(Some(other)), Choice::Automatic);
    }
}
