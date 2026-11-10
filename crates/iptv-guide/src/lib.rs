//! The guide schedule built from real data: imported channels and groups
//! (`iptv-providers`) plus the EPG store (`iptv-epg`), behind
//! `iptv_core::guide::Schedule` so the focus model and the UI work on it
//! exactly as on the synthetic schedule.
//!
//! Guide minutes are relative to `now` (minute 0), fixed when the schedule
//! is built; rebuild it to move "now". Where a channel has no programme, the
//! gap is filled with hour-aligned placeholder slots clipped to the
//! neighbouring programmes, so navigation always has a cell to land on.

use iptv_core::guide::{Program, Schedule};
use iptv_epg::{ChannelKey, GuideStore, UnixTime};
use iptv_providers::model::{Channel, Group};

/// Gap slots are aligned to the hour.
const SLOT: i64 = 3600;
/// Title of a gap slot.
pub const NO_INFORMATION: &str = "No information";

/// A channel as the guide needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuideChannel {
    /// The key the EPG store matched the channel under (its stable id).
    pub key: ChannelKey,
    pub name: String,
    pub catchup_days: u8,
}

pub struct EpgSchedule {
    channels: Vec<GuideChannel>,
    /// The imported channel behind each guide channel (stream and catch-up).
    sources: Vec<Option<Channel>>,
    group_names: Vec<String>,
    /// The index of each group's first channel; channels are stored grouped.
    group_starts: Vec<usize>,
    store: GuideStore,
    now: UnixTime,
}

impl EpgSchedule {
    /// `groups`: names with their channels, in display order. `now` is
    /// rounded down to the minute.
    pub fn new(groups: Vec<(String, Vec<GuideChannel>)>, store: GuideStore, now: UnixTime) -> Self {
        let mut channels = Vec::new();
        let mut group_names = Vec::new();
        let mut group_starts = Vec::new();
        for (name, members) in groups.into_iter().filter(|(_, m)| !m.is_empty()) {
            group_names.push(name);
            group_starts.push(channels.len());
            channels.extend(members);
        }
        EpgSchedule {
            sources: vec![None; channels.len()],
            channels,
            group_names,
            group_starts,
            store,
            now: now - now.rem_euclid(60),
        }
    }

    /// From an imported catalog: each channel in its first group (in the
    /// catalog's group order), channels without a group last under "Other".
    pub fn from_catalog(
        groups: &[Group],
        channels: &[Channel],
        store: GuideStore,
        now: UnixTime,
    ) -> Self {
        let mut buckets: Vec<Vec<&Channel>> = vec![Vec::new(); groups.len() + 1];
        for c in channels {
            let slot = c
                .groups
                .first()
                .and_then(|gid| groups.iter().position(|g| &g.id == gid));
            buckets[slot.unwrap_or(groups.len())].push(c);
        }
        let names = groups
            .iter()
            .map(|g| g.name.clone())
            .chain(["Other".to_owned()]);
        let mut ordered = Vec::new();
        let grouped = names
            .zip(buckets)
            .map(|(name, members)| {
                let guide = members
                    .iter()
                    .map(|c| GuideChannel {
                        key: ChannelKey::new(c.id.as_str()),
                        name: c.name.clone(),
                        catchup_days: c.catch_up.as_ref().map_or(0, |cu| {
                            cu.window_days(iptv_providers::m3u::DEFAULT_CATCHUP_WINDOW_DAYS)
                                .min(u32::from(u8::MAX)) as u8
                        }),
                    })
                    .collect::<Vec<_>>();
                ordered.extend(members.into_iter().cloned());
                (name, guide)
            })
            .collect();
        let mut schedule = EpgSchedule::new(grouped, store, now);
        schedule.sources = ordered.into_iter().map(Some).collect();
        schedule
    }

    pub fn now(&self) -> UnixTime {
        self.now
    }

    pub fn store(&self) -> &GuideStore {
        &self.store
    }

    pub fn channel(&self, channel: usize) -> Option<&GuideChannel> {
        self.channels.get(channel)
    }

    fn minute(&self, t: UnixTime) -> i32 {
        (t - self.now).div_euclid(60) as i32
    }

    fn unix(&self, minute: i32) -> UnixTime {
        self.now + i64::from(minute) * 60
    }

    fn key(&self, channel: usize) -> Option<&ChannelKey> {
        self.channels.get(channel).map(|c| &c.key)
    }

    /// The description of `program`, when the EPG has one.
    pub fn description(&self, channel: usize, program: Program) -> Option<String> {
        let key = self.key(channel)?;
        let p = self.store.programme_at(key, self.probe(program))?;
        p.desc().map(str::to_owned)
    }

    /// A time inside `program` (its first second, or its middle if short).
    fn probe(&self, program: Program) -> UnixTime {
        let (a, b) = (self.unix(program.start), self.unix(program.end));
        (a + 59).min((a + b) / 2).max(a)
    }

    /// The placeholder slot around `t` for a channel with no programme then.
    fn gap(&self, key: Option<&ChannelKey>, t: UnixTime) -> (UnixTime, UnixTime) {
        let slot_start = t - t.rem_euclid(SLOT);
        let mut start = slot_start;
        let mut end = slot_start + SLOT;
        if let Some(key) = key {
            if let Some(prev_end) = self
                .store
                .programmes(key, slot_start, t + 1)
                .map(|p| p.stop())
                .filter(|&s| s <= t)
                .max()
            {
                start = start.max(prev_end);
            }
            if let Some(next) = self.store.next_after(key, t) {
                if next.start() > t {
                    end = end.min(next.start());
                }
            }
        }
        (start, end)
    }
}

impl Schedule for EpgSchedule {
    fn channel_count(&self) -> usize {
        self.channels.len()
    }

    fn group_count(&self) -> usize {
        self.group_names.len()
    }

    fn group_start(&self, group: usize) -> usize {
        self.group_starts.get(group).copied().unwrap_or(0)
    }

    fn program_at(&self, channel: usize, minute: i32) -> Program {
        let t = self.unix(minute);
        let key = self.key(channel);
        let (start, end) = match key.and_then(|k| self.store.programme_at(k, t)) {
            Some(p) => (p.start(), p.stop()),
            None => self.gap(key, t),
        };
        let (start, end) = (self.minute(start), self.minute(end));
        // Sub-minute programmes still get a one-minute cell.
        Program {
            start,
            end: end.max(start + 1),
        }
    }

    fn programs_between(&self, channel: usize, from: i32, minutes: i32) -> Vec<Program> {
        let to = from + minutes;
        let mut out = Vec::new();
        let mut at = from;
        while at < to {
            let p = self.program_at(channel, at);
            out.push(p);
            at = p.end.max(at + 1);
        }
        out
    }

    fn catchup_days(&self, channel: usize) -> u8 {
        self.channels.get(channel).map_or(0, |c| c.catchup_days)
    }

    fn channel_name(&self, channel: usize) -> String {
        self.channels
            .get(channel)
            .map_or_else(String::new, |c| c.name.clone())
    }

    fn group_name(&self, group: usize) -> String {
        self.group_names.get(group).cloned().unwrap_or_default()
    }

    fn stream_url(&self, channel: usize) -> Option<String> {
        match &self.sources.get(channel)?.as_ref()?.stream {
            iptv_providers::model::StreamLocator::Url { url, .. } => Some(url.clone()),
            // Xtream and Stalker URLs need their clients (session tokens).
            _ => None,
        }
    }

    fn catchup_url(&self, channel: usize, program: Program) -> Option<String> {
        let c = self.sources.get(channel)?.as_ref()?;
        let req = iptv_providers::m3u::CatchupRequest::new(
            self.unix(program.start),
            self.unix(program.end),
            self.now,
        );
        iptv_providers::m3u::build_catchup_url(c, req).ok()
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn title(&self, channel: usize, program: Program) -> String {
        let found = self
            .key(channel)
            .and_then(|k| self.store.programme_at(k, self.probe(program)))
            .filter(|p| self.minute(p.start()) == program.start);
        found.map_or_else(|| NO_INFORMATION.to_owned(), |p| p.title().to_owned())
    }
}

#[cfg(test)]
mod tests;
