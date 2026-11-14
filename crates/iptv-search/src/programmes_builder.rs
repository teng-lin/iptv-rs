//! A [`ProgrammeIndex`] from programmes read one at a time (a stored
//! guide's titles streamed from disk) instead of a [`iptv_epg::GuideStore`]
//! held in memory.

use std::collections::HashMap;

use iptv_epg::UnixTime;

use crate::programmes::ProgrammeIndex;

/// Collects programmes for a [`ProgrammeIndex`].
///
/// ```
/// use iptv_search::{ProgrammeIndexBuilder, ProgrammeSearchOptions};
/// let mut b = ProgrammeIndexBuilder::new(2);
/// b.push(1, 1000, 4600, "Evening News");
/// let index = b.finish();
/// let opts = ProgrammeSearchOptions { include_past_without_catchup: true, ..Default::default() };
/// let hits = index.search_programmes("news", 2000, opts);
/// assert_eq!((hits[0].channel, hits[0].start), (1, 1000));
/// ```
#[derive(Default)]
pub struct ProgrammeIndexBuilder {
    channels: usize,
    ids: HashMap<Box<str>, u32>,
    /// Title id, channel, start, stop.
    raw: Vec<(u32, u32, i64, i64)>,
}

impl ProgrammeIndexBuilder {
    /// For `channels` channels (indexes `0..channels`).
    pub fn new(channels: usize) -> Self {
        ProgrammeIndexBuilder {
            channels,
            ..Default::default()
        }
    }

    /// Adds one programme of `channel` (beyond `channels`: ignored).
    pub fn push(&mut self, channel: usize, start: UnixTime, stop: UnixTime, title: &str) {
        if channel >= self.channels {
            return;
        }
        let next = self.ids.len() as u32;
        let id = match self.ids.get(title) {
            Some(&id) => id,
            None => {
                self.ids.insert(title.into(), next);
                next
            }
        };
        self.raw.push((id, channel as u32, start, stop));
    }

    /// Programmes added.
    pub fn len(&self) -> usize {
        self.raw.len()
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    pub fn finish(self) -> ProgrammeIndex {
        let mut titles = vec![""; self.ids.len()];
        for (title, &id) in &self.ids {
            titles[id as usize] = title;
        }
        ProgrammeIndex::assemble(&titles, &self.raw, self.channels)
    }
}

#[cfg(test)]
mod tests {
    use iptv_epg::{
        ChannelKey, EpgRef, FeedId, GuideStore, MatchKind, SourceBuilder, StoreOptions,
    };

    use super::*;
    use crate::ProgrammeSearchOptions;

    #[test]
    fn built_from_rows_it_finds_what_the_store_index_finds() {
        let rows = [
            (0, 1000, 2000, "Morning News"),
            (0, 2000, 3000, "Cooking Hour"),
            (1, 1500, 2500, "News at Noon"),
            (1, 2500, 9000, "Morning News"),
        ];
        let mut b = SourceBuilder::new(FeedId(1), 0, StoreOptions::default(), 0);
        let mut built = ProgrammeIndexBuilder::new(2);
        for (ch, start, stop, title) in rows {
            b.push_data(
                &format!("e{ch}"),
                &iptv_epg::ProgrammeData {
                    start,
                    stop: Some(stop),
                    title,
                    ..Default::default()
                },
            );
            built.push(ch, start, stop, title);
        }
        built.push(7, 0, 10, "beyond the channels");
        assert_eq!(built.len(), 4);
        let mut store = GuideStore::new();
        store.replace_source(b.finish());
        let keys = [ChannelKey::new("a"), ChannelKey::new("b")];
        let matches = keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let cand = EpgRef {
                    source: FeedId(1),
                    epg_id: format!("e{i}"),
                    kind: MatchKind::TvgId,
                };
                (k.clone(), vec![cand])
            })
            .collect();
        store.set_matches(matches);
        let from_store = ProgrammeIndex::build(&store, &keys);
        let built = built.finish();
        assert_eq!(built.programme_count(), from_store.programme_count());
        assert_eq!(built.title_count(), from_store.title_count());
        let opts = ProgrammeSearchOptions {
            include_past_without_catchup: true,
            ..Default::default()
        };
        for q in ["news", "morning", "cook", "noon"] {
            assert_eq!(
                built.search_programmes(q, 2200, opts),
                from_store.search_programmes(q, 2200, opts),
                "{q}"
            );
        }
    }
}
