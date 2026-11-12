//! A live guide spanning independent sources. Provider identities and clocks
//! remain local; UI identities are qualified by the owning source.
use crate::EpgSchedule;
use iptv_core::guide::{Program, Schedule};
use std::rc::Rc;

pub fn qualified_key(source: i64, key: &str) -> String {
    format!("source:{source}:{key}")
}
pub fn split_key(key: &str) -> Option<(i64, &str)> {
    let (source, key) = key.strip_prefix("source:")?.split_once(':')?;
    Some((source.parse().ok()?, key))
}

pub struct SourceSchedule {
    pub id: i64,
    pub name: String,
    pub schedule: Rc<EpgSchedule>,
}

/// Immutable membership for a displayed generation. Child guide pages remain
/// lazily loadable, exactly as when a single source is displayed.
pub struct CombinedSchedule {
    sources: Vec<SourceSchedule>,
    channels: Vec<(usize, usize)>,
    groups: Vec<(usize, usize)>,
    epoch: i64,
}
impl CombinedSchedule {
    pub fn new(sources: Vec<SourceSchedule>, epoch: i64) -> Self {
        let channels = sources
            .iter()
            .enumerate()
            .flat_map(|(s, p)| (0..p.schedule.channel_count()).map(move |c| (s, c)))
            .collect();
        let groups = sources
            .iter()
            .enumerate()
            .flat_map(|(s, p)| (0..p.schedule.group_count()).map(move |g| (s, g)))
            .collect();
        Self {
            sources,
            channels,
            groups,
            epoch,
        }
    }
    pub fn sources(&self) -> &[SourceSchedule] {
        &self.sources
    }
    pub fn channel_source(&self, channel: usize) -> Option<(&SourceSchedule, usize)> {
        let &(s, c) = self.channels.get(channel)?;
        Some((&self.sources[s], c))
    }
    pub fn source(&self, id: i64) -> Option<&EpgSchedule> {
        self.sources
            .iter()
            .find(|s| s.id == id)
            .map(|s| &*s.schedule)
    }
    fn offset(&self, s: &EpgSchedule) -> i32 {
        ((self.epoch - s.epoch()) / 60) as i32
    }
    fn local(&self, s: &EpgSchedule, p: Program) -> Program {
        let d = self.offset(s);
        Program {
            start: p.start + d,
            end: p.end + d,
        }
    }
    fn global(&self, s: &EpgSchedule, p: Program) -> Program {
        let d = self.offset(s);
        Program {
            start: p.start - d,
            end: p.end - d,
        }
    }
    fn channel_offset(&self, source: usize) -> usize {
        self.sources[..source]
            .iter()
            .map(|s| s.schedule.channel_count())
            .sum()
    }
    pub fn description(&self, channel: usize, p: Program) -> Option<String> {
        let (s, c) = self.channel_source(channel)?;
        s.schedule.description(c, self.local(&s.schedule, p))
    }
    pub fn resolve_from(
        &self,
        channel: usize,
        archive: Option<Program>,
        offset: i64,
        clock: crate::playback::Clock,
    ) -> crate::playback::Resolution {
        let Some((s, c)) = self.channel_source(channel) else {
            return crate::playback::Resolution::Unavailable(
                crate::playback::Unavailable::NoChannel,
            );
        };
        s.schedule.resolve_from(
            c,
            archive.map(|p| self.local(&s.schedule, p)),
            offset,
            clock,
        )
    }
}
impl Schedule for CombinedSchedule {
    fn epoch_unix(&self) -> i64 {
        self.epoch
    }
    fn follows_wall_clock(&self) -> bool {
        true
    }
    fn channel_count(&self) -> usize {
        self.channels.len()
    }
    fn group_count(&self) -> usize {
        self.groups.len()
    }
    fn group_start(&self, g: usize) -> usize {
        self.group_channels(g).first().copied().unwrap_or(0)
    }
    fn group_key(&self, g: usize) -> String {
        self.groups
            .get(g)
            .map(|&(s, g)| {
                qualified_key(self.sources[s].id, &self.sources[s].schedule.group_key(g))
            })
            .unwrap_or_default()
    }
    fn group_channels(&self, g: usize) -> Vec<usize> {
        let Some(&(s, g)) = self.groups.get(g) else {
            return Vec::new();
        };
        let offset = self.channel_offset(s);
        self.sources[s]
            .schedule
            .group_channels(g)
            .into_iter()
            .map(|c| c + offset)
            .collect()
    }
    fn group_name(&self, g: usize) -> String {
        self.groups
            .get(g)
            .map(|&(s, g)| {
                format!(
                    "{} · {}",
                    self.sources[s].name,
                    self.sources[s].schedule.group_name(g)
                )
            })
            .unwrap_or_default()
    }
    fn program_at(&self, c: usize, m: i32) -> Program {
        let Some((s, c)) = self.channel_source(c) else {
            return Program {
                start: m,
                end: m + 60,
            };
        };
        self.global(
            &s.schedule,
            s.schedule.program_at(c, m + self.offset(&s.schedule)),
        )
    }
    fn programs_between(&self, c: usize, from: i32, minutes: i32) -> Vec<Program> {
        let Some((s, c)) = self.channel_source(c) else {
            return Vec::new();
        };
        s.schedule
            .programs_between(c, from + self.offset(&s.schedule), minutes)
            .into_iter()
            .map(|p| self.global(&s.schedule, p))
            .collect()
    }
    fn catchup_days(&self, c: usize) -> u8 {
        self.channel_source(c)
            .map_or(0, |(s, c)| s.schedule.catchup_days(c))
    }
    fn channel_name(&self, c: usize) -> String {
        self.channel_source(c)
            .map(|(s, c)| format!("{} · {}", s.schedule.channel_name(c), s.name))
            .unwrap_or_default()
    }
    fn channel_hidden(&self, c: usize) -> bool {
        self.channel_source(c)
            .is_none_or(|(s, c)| s.schedule.channel_hidden(c))
    }
    fn channel_number(&self, c: usize) -> u32 {
        self.channel_source(c)
            .map_or(0, |(s, c)| s.schedule.channel_number(c))
    }
    fn channel_logo(&self, c: usize) -> Option<String> {
        let (s, c) = self.channel_source(c)?;
        s.schedule.channel_logo(c)
    }
    fn channel_key(&self, c: usize) -> Option<String> {
        let (s, c) = self.channel_source(c)?;
        Some(qualified_key(s.id, &s.schedule.channel_key(c)?))
    }
    fn title(&self, c: usize, p: Program) -> String {
        self.channel_source(c)
            .map(|(s, c)| s.schedule.title(c, self.local(&s.schedule, p)))
            .unwrap_or_default()
    }
    fn stream_url(&self, c: usize) -> Option<String> {
        let (s, c) = self.channel_source(c)?;
        s.schedule.stream_url(c)
    }
    fn catchup_url(&self, c: usize, p: Program, now: i32) -> Option<String> {
        let (s, c) = self.channel_source(c)?;
        s.schedule.catchup_url(
            c,
            self.local(&s.schedule, p),
            now + self.offset(&s.schedule),
        )
    }
    fn playlist_order(&self) -> Vec<usize> {
        self.sources
            .iter()
            .enumerate()
            .flat_map(|(s, p)| {
                let offset = self.channel_offset(s);
                p.schedule
                    .playlist_order()
                    .into_iter()
                    .map(move |c| c + offset)
            })
            .collect()
    }
    fn browsable(&self, now: i32) -> (i32, i32) {
        self.sources
            .iter()
            .map(|s| {
                let d = self.offset(&s.schedule);
                let (a, b) = s.schedule.browsable(now + d);
                (a - d, b - d)
            })
            .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
            .unwrap_or((now, now + iptv_core::guide::HORIZON))
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// Concrete owning schedule and its local row (for tracks, portal tokens,
/// archive pages and descriptions). Never use a global row on a child.
pub fn epg_channel(schedule: &dyn Schedule, channel: usize) -> Option<(&EpgSchedule, usize)> {
    let any = schedule.as_any()?;
    if let Some(s) = any.downcast_ref::<EpgSchedule>() {
        return Some((s, channel));
    }
    let (s, c) = any
        .downcast_ref::<CombinedSchedule>()?
        .channel_source(channel)?;
    Some((&s.schedule, c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iptv_epg::{
        ChannelKey, GuideStore, MatchIndex, Overrides, ParseOptions, PlaylistChannel,
        SourceBuilder, StoreOptions, parse_xmltv,
    };
    use iptv_providers::{SecretUrl, model::M3uSourceConfig};
    const NOW: i64 = 1_800_000_000;
    fn part(id: i64, name: &str, now: i64) -> SourceSchedule {
        let parsed = iptv_providers::m3u::parse_m3u(
            "#EXTM3U\n#EXTINF:-1 tvg-id=\"same\" group-title=\"News\" catchup=\"append\" catchup-days=\"3\" catchup-source=\"?utc={utc}\",Shared\nhttp://stream.example.test/shared\n",
        );
        let channel = &parsed.channels.items[0];
        let xml = format!(
            "<tv><channel id=\"same\"><display-name>Shared</display-name></channel><programme channel=\"same\" start=\"20270115080000 +0000\" stop=\"20270115090000 +0000\"><title>{name} programme</title><desc>{name} description</desc></programme></tv>"
        );
        let xml = parse_xmltv(xml.as_bytes(), ParseOptions::default()).unwrap();
        let mut builder =
            SourceBuilder::new(iptv_epg::SourceId(1), 0, StoreOptions::default(), now);
        for c in xml.channels {
            builder.push_channel(c);
        }
        for p in xml.programmes {
            builder.push_programme(p);
        }
        let feed = builder.finish();
        let mut index = MatchIndex::new();
        index.add_source(feed.id(), feed.priority(), feed.channels());
        let key = ChannelKey::new(channel.id.as_str());
        let matched = index.match_channels(
            [PlaylistChannel {
                key: &key,
                tvg_id: Some("same"),
                name: "Shared",
                tvg_name: None,
            }],
            &Overrides::default(),
        );
        let mut guide = GuideStore::new();
        guide.replace_source(feed);
        guide.set_matches(matched.matches);
        let context = crate::playback::PlaybackContext::m3u(&M3uSourceConfig {
            location: SecretUrl::new("http://playlist.example.test/list.m3u"),
            user_agent: Some(name.into()),
            referrer: None,
        });
        SourceSchedule {
            id,
            name: name.into(),
            schedule: Rc::new(
                EpgSchedule::from_catalog(&parsed.groups, &parsed.channels.items, guide, now)
                    .with_playback(Some(context)),
            ),
        }
    }
    #[test]
    fn duplicate_provider_ids_and_group_names_are_source_qualified() {
        let a = part(1, "Alpha", NOW);
        let b = part(2, "Beta", NOW + 7200);
        assert_eq!(a.schedule.channel_key(0), b.schedule.channel_key(0));
        let all = CombinedSchedule::new(vec![a, b], NOW);
        assert_eq!(all.channel_count(), 2);
        assert_eq!(all.group_count(), 2);
        assert_ne!(all.channel_key(0), all.channel_key(1));
        assert_ne!(all.group_key(0), all.group_key(1));
        assert_eq!(all.group_name(1), "Beta · News");
        assert_eq!(all.group_channels(1), vec![1]);
        let beta = all.channel_key(1).unwrap();
        let (source, key) = split_key(&beta).unwrap();
        assert_eq!(source, 2);
        assert_eq!(all.sources[1].schedule.channel_key(0).as_deref(), Some(key));
        assert_eq!(all.playlist_order(), vec![0, 1]);
        let only_beta = CombinedSchedule::new(vec![part(2, "Beta", NOW)], NOW);
        assert_eq!(only_beta.find_channel(&beta), Some(0));
        assert_eq!(only_beta.find_channel(&all.channel_key(0).unwrap()), None);
        assert_eq!(
            split_key(&qualified_key(4, "custom:9:channel")),
            Some((4, "custom:9:channel"))
        );
        assert!(split_key("source:invalid:key").is_none());
    }
    #[test]
    fn guide_times_descriptions_and_headers_resolve_with_the_owning_source() {
        let all = CombinedSchedule::new(
            vec![part(1, "Alpha", NOW), part(2, "Beta", NOW + 7200)],
            NOW,
        );
        // The fixture's programme lies at the same absolute time in each child,
        // despite their different epochs.
        let t=iptv_epg::parse_xmltv(b"<tv><programme channel=\"same\" start=\"20270115080000 +0000\" stop=\"20270115090000 +0000\"><title>P</title></programme></tv>",ParseOptions::default()).unwrap().programmes[0].start;
        let minute = ((t - NOW) / 60) as i32;
        let a = all.program_at(0, minute);
        let b = all.program_at(1, minute);
        assert_eq!(a, b);
        assert_eq!(all.title(0, a), "Alpha programme");
        assert_eq!(all.title(1, b), "Beta programme");
        assert_eq!(all.description(1, b).as_deref(), Some("Beta description"));
        let clock = crate::playback::Clock {
            now: t + 7200,
            local_offset_secs: 0,
        };
        for (channel, name) in [(0, "Alpha"), (1, "Beta")] {
            let live = all.resolve_from(channel, None, 0, clock).ready().unwrap();
            assert!(
                live.headers
                    .iter()
                    .any(|(k, v)| k.eq_ignore_ascii_case("User-Agent") && v == name)
            );
            let archive = all
                .resolve_from(channel, Some(a), 0, clock)
                .ready()
                .unwrap();
            assert!(archive.url.contains(&format!("utc={t}")));
        }
    }
}
