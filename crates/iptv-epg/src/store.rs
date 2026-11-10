//! Compact programme storage for the guide.
//!
//! Each EPG source becomes an [`EpgSource`]: per EPG channel a sorted,
//! non-overlapping array of 16-byte slots plus an interned string arena.
//! A [`GuideStore`] holds several sources and binds playlist channels to
//! the best one (highest priority with data). Every query is a hash lookup
//! plus a binary search: O(log n) in the channel's programme count.

use std::collections::HashMap;

use crate::intern::{Interner, NONE, Sym};
use crate::matching::{ChannelKey, EpgRef, SourceId};
use crate::time::{DAY, UnixTime};
use crate::xmltv::{XmltvChannel, XmltvItem, XmltvProgramme};

/// Storage settings (the reference UX's "store descriptions" and "past days
/// to keep").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreOptions {
    pub store_descriptions: bool,
    /// Programmes that ended more than this many days before `now` are
    /// dropped. `None` keeps everything.
    pub past_days_to_keep: Option<u32>,
}

impl Default for StoreOptions {
    fn default() -> Self {
        StoreOptions {
            store_descriptions: true,
            past_days_to_keep: None,
        }
    }
}

/// What happened to the programmes of one source during ingest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IngestReport {
    pub programmes_in: u64,
    pub kept: u64,
    /// EPG channels with at least one programme.
    pub channels_with_programmes: u64,
    /// `stop <= start`.
    pub dropped_bad_duration: u64,
    /// No `stop` and no following programme to end it.
    pub dropped_no_stop: u64,
    /// Older than `past_days_to_keep`.
    pub dropped_retention: u64,
    /// Same channel and start as an earlier (longer or equal) programme.
    pub dropped_duplicate: u64,
    /// Programmes shortened to end where the next one starts.
    pub overlaps_trimmed: u64,
}

const NO_EXTRA: u32 = u32::MAX;
const NO_STOP: i64 = i64::MIN;

/// One programme in a schedule: 16 bytes, times relative to the schedule
/// base.
#[derive(Clone, Copy, Debug)]
struct Slot {
    start: u32,
    dur: u32,
    title: Sym,
    extra: u32,
}

/// Fields most guide cells do not need.
#[derive(Clone, Copy, Debug, Default)]
struct Extra {
    sub_title: Sym,
    desc: Sym,
    category: Sym,
    icon: Sym,
    rating: Sym,
    catchup_id: Sym,
    /// One-based; 0 = unknown.
    season: u16,
    episode: u16,
}

#[derive(Default)]
struct Schedule {
    base: i64,
    slots: Vec<Slot>,
}

impl Schedule {
    fn start(&self, s: &Slot) -> i64 {
        self.base + i64::from(s.start)
    }
    fn stop(&self, s: &Slot) -> i64 {
        self.base + i64::from(s.start) + i64::from(s.dur)
    }
}

/// The ingested guide data of one EPG source.
pub struct EpgSource {
    id: SourceId,
    priority: i32,
    strings: Interner,
    extras: Vec<Extra>,
    schedules: Vec<Schedule>,
    by_epg_id: HashMap<Box<str>, u32>,
    channels: Vec<XmltvChannel>,
    report: IngestReport,
}

impl EpgSource {
    pub fn id(&self) -> SourceId {
        self.id
    }

    pub fn priority(&self) -> i32 {
        self.priority
    }

    /// The source's `<channel>` records, for [`crate::MatchIndex::add_source`].
    pub fn channels(&self) -> &[XmltvChannel] {
        &self.channels
    }

    pub fn report(&self) -> IngestReport {
        self.report
    }

    /// Schedule of one EPG channel id of this source.
    pub fn schedule(&self, epg_id: &str) -> Option<ScheduleRef<'_>> {
        let i = *self.by_epg_id.get(epg_id)?;
        Some(ScheduleRef {
            src: self,
            sched: &self.schedules[i as usize],
        })
    }

    /// Number of programmes held.
    pub fn programme_count(&self) -> usize {
        self.schedules.iter().map(|s| s.slots.len()).sum()
    }

    /// Distinct strings held (titles, descriptions, ...).
    pub fn string_count(&self) -> usize {
        self.strings.len()
    }

    /// Rough heap usage in bytes.
    pub fn heap_bytes(&self) -> usize {
        self.strings.heap_bytes()
            + self.extras.capacity() * size_of::<Extra>()
            + self
                .schedules
                .iter()
                .map(|s| s.slots.capacity() * size_of::<Slot>())
                .sum::<usize>()
            + self.schedules.capacity() * size_of::<Schedule>()
            + self.by_epg_id.keys().map(|k| k.len() + 24).sum::<usize>()
    }

    fn prune(&mut self, cutoff: i64) {
        for s in &mut self.schedules {
            let cut = s
                .slots
                .partition_point(|x| s.base + i64::from(x.start) + i64::from(x.dur) <= cutoff);
            s.slots.drain(..cut);
        }
    }
}

struct RawProgramme {
    start: i64,
    stop: i64,
    title: Sym,
    extra: u32,
}

/// Builds an [`EpgSource`] from parser output, one record at a time.
///
/// ```
/// use iptv_epg::*;
/// let mut b = SourceBuilder::new(SourceId(1), 10, StoreOptions::default(), 0);
/// b.push_programme(XmltvProgramme {
///     channel: "one.example".into(),
///     start: 1000,
///     stop: Some(2000),
///     title: "Show".into(),
///     ..Default::default()
/// });
/// let src = b.finish();
/// let p = src.schedule("one.example").unwrap().at(1500).unwrap();
/// assert_eq!((p.title(), p.start(), p.stop()), ("Show", 1000, 2000));
/// ```
pub struct SourceBuilder {
    id: SourceId,
    priority: i32,
    opts: StoreOptions,
    cutoff: Option<i64>,
    strings: Interner,
    extras: Vec<Extra>,
    by_epg_id: HashMap<Box<str>, u32>,
    raw: Vec<Vec<RawProgramme>>,
    channels: Vec<XmltvChannel>,
    report: IngestReport,
}

impl SourceBuilder {
    /// `now` is only used for retention.
    pub fn new(id: SourceId, priority: i32, opts: StoreOptions, now: UnixTime) -> Self {
        SourceBuilder {
            id,
            priority,
            cutoff: opts.past_days_to_keep.map(|d| now - i64::from(d) * DAY),
            opts,
            strings: Interner::new(),
            extras: Vec::new(),
            by_epg_id: HashMap::new(),
            raw: Vec::new(),
            channels: Vec::new(),
            report: IngestReport::default(),
        }
    }

    pub fn push(&mut self, item: XmltvItem) {
        match item {
            XmltvItem::Channel(c) => self.push_channel(c),
            XmltvItem::Programme(p) => self.push_programme(p),
        }
    }

    pub fn push_channel(&mut self, c: XmltvChannel) {
        self.channels.push(c);
    }

    pub fn push_programme(&mut self, p: XmltvProgramme) {
        self.report.programmes_in += 1;
        let stop = p.stop.unwrap_or(NO_STOP);
        if p.stop.is_some() && stop <= p.start {
            self.report.dropped_bad_duration += 1;
            return;
        }
        if p.stop.is_some() && self.cutoff.is_some_and(|c| stop <= c) {
            self.report.dropped_retention += 1;
            return;
        }
        let s = &mut self.strings;
        let (season, episode) = p
            .episode_num
            .iter()
            .find_map(|e| e.season_episode())
            .unwrap_or_default();
        let extra = Extra {
            sub_title: p.sub_title.as_deref().map_or(NONE, |v| s.intern(v)),
            desc: match &p.desc {
                Some(d) if self.opts.store_descriptions => s.intern(d),
                _ => NONE,
            },
            category: p.categories.first().map_or(NONE, |v| s.intern(v)),
            icon: p.icon.as_deref().map_or(NONE, |v| s.intern(v)),
            rating: p.rating.as_ref().map_or(NONE, |r| s.intern(&r.value)),
            catchup_id: p.catchup_id.as_deref().map_or(NONE, |v| s.intern(v)),
            season: season.map_or(0, |n| n.min(u32::from(u16::MAX)) as u16),
            episode: episode.map_or(0, |n| n.min(u32::from(u16::MAX)) as u16),
        };
        let has_extra = extra.sub_title
            | extra.desc
            | extra.category
            | extra.icon
            | extra.rating
            | extra.catchup_id
            != NONE
            || extra.season != 0
            || extra.episode != 0;
        let extra = if has_extra {
            self.extras.push(extra);
            (self.extras.len() - 1) as u32
        } else {
            NO_EXTRA
        };
        let title = s.intern(&p.title);
        let next = self.raw.len() as u32;
        let ch = *self
            .by_epg_id
            .entry(p.channel.into_boxed_str())
            .or_insert(next);
        if ch == next {
            self.raw.push(Vec::new());
        }
        self.raw[ch as usize].push(RawProgramme {
            start: p.start,
            stop,
            title,
            extra,
        });
    }

    /// Sorts, resolves overlaps and freezes the source.
    ///
    /// Per channel, deterministically: programmes are ordered by start; of
    /// several with the same start the longest (then the first in input)
    /// is kept; a missing stop becomes the next programme's start; a
    /// programme running into the next one is cut at the next start.
    pub fn finish(mut self) -> EpgSource {
        let mut schedules = Vec::with_capacity(self.raw.len());
        let r = &mut self.report;
        for mut raw in std::mem::take(&mut self.raw) {
            // Stable: equal (start, stop) keep input order. NO_STOP sorts last.
            raw.sort_by(|a, b| a.start.cmp(&b.start).then(b.stop.cmp(&a.stop)));
            let before = raw.len();
            raw.dedup_by(|cur, prev| cur.start == prev.start);
            r.dropped_duplicate += (before - raw.len()) as u64;
            let n = raw.len();
            for i in 0..n {
                let next_start = raw.get(i + 1).map(|x| x.start);
                let p = &mut raw[i];
                if p.stop == NO_STOP {
                    p.stop = next_start.unwrap_or(NO_STOP);
                } else if let Some(ns) = next_start.filter(|&ns| p.stop > ns) {
                    p.stop = ns;
                    r.overlaps_trimmed += 1;
                }
            }
            let mut slots = Vec::with_capacity(n);
            let base = raw.first().map_or(0, |p| p.start);
            for p in raw {
                if p.stop == NO_STOP {
                    r.dropped_no_stop += 1;
                } else if self.cutoff.is_some_and(|c| p.stop <= c) {
                    r.dropped_retention += 1;
                } else if let (Ok(start), Ok(dur)) = (
                    u32::try_from(p.start - base),
                    u32::try_from(p.stop - p.start),
                ) {
                    slots.push(Slot {
                        start,
                        dur,
                        title: p.title,
                        extra: p.extra,
                    });
                } else {
                    r.dropped_bad_duration += 1;
                }
            }
            r.kept += slots.len() as u64;
            r.channels_with_programmes += u64::from(!slots.is_empty());
            // Re-base so the first kept slot starts at 0.
            let shift = slots.first().map_or(0, |s| s.start);
            for s in &mut slots {
                s.start -= shift;
            }
            schedules.push(Schedule {
                base: base + i64::from(shift),
                slots,
            });
        }
        self.strings.shrink();
        self.extras.shrink_to_fit();
        EpgSource {
            id: self.id,
            priority: self.priority,
            strings: self.strings,
            extras: self.extras,
            schedules,
            by_epg_id: self.by_epg_id,
            channels: self.channels,
            report: self.report,
        }
    }
}

/// A borrowed view of one programme.
#[derive(Clone, Copy)]
pub struct ProgrammeRef<'a> {
    src: &'a EpgSource,
    sched: &'a Schedule,
    slot: &'a Slot,
}

impl<'a> ProgrammeRef<'a> {
    pub fn start(&self) -> UnixTime {
        self.sched.start(self.slot)
    }

    pub fn stop(&self) -> UnixTime {
        self.sched.stop(self.slot)
    }

    pub fn title(&self) -> &'a str {
        self.src.strings.get(self.slot.title)
    }

    pub fn source(&self) -> SourceId {
        self.src.id
    }

    fn extra(&self) -> Option<&'a Extra> {
        self.src.extras.get(self.slot.extra as usize)
    }

    fn text(&self, f: impl Fn(&Extra) -> Sym) -> Option<&'a str> {
        self.extra().and_then(|e| self.src.strings.opt(f(e)))
    }

    pub fn sub_title(&self) -> Option<&'a str> {
        self.text(|e| e.sub_title)
    }

    /// `None` also when the source was stored without descriptions.
    pub fn desc(&self) -> Option<&'a str> {
        self.text(|e| e.desc)
    }

    /// First category.
    pub fn category(&self) -> Option<&'a str> {
        self.text(|e| e.category)
    }

    pub fn icon(&self) -> Option<&'a str> {
        self.text(|e| e.icon)
    }

    pub fn rating(&self) -> Option<&'a str> {
        self.text(|e| e.rating)
    }

    pub fn catchup_id(&self) -> Option<&'a str> {
        self.text(|e| e.catchup_id)
    }

    /// One-based season and episode, when known.
    pub fn season_episode(&self) -> (Option<u16>, Option<u16>) {
        let nz = |n: u16| (n != 0).then_some(n);
        self.extra()
            .map_or((None, None), |e| (nz(e.season), nz(e.episode)))
    }

    pub fn is_airing(&self, now: UnixTime) -> bool {
        self.start() <= now && now < self.stop()
    }

    /// Fraction elapsed at `now`, clamped to `0.0..=1.0`.
    pub fn progress(&self, now: UnixTime) -> f32 {
        let dur = (self.stop() - self.start()).max(1) as f64;
        ((now - self.start()) as f64 / dur).clamp(0.0, 1.0) as f32
    }
}

impl std::fmt::Debug for ProgrammeRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Programme")
            .field("start", &self.start())
            .field("stop", &self.stop())
            .field("title", &self.title())
            .finish()
    }
}

/// A borrowed view of one channel's schedule.
#[derive(Clone, Copy)]
pub struct ScheduleRef<'a> {
    src: &'a EpgSource,
    sched: &'a Schedule,
}

impl<'a> ScheduleRef<'a> {
    pub fn len(&self) -> usize {
        self.sched.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sched.slots.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<ProgrammeRef<'a>> {
        let slot = self.sched.slots.get(i)?;
        Some(ProgrammeRef {
            src: self.src,
            sched: self.sched,
            slot,
        })
    }

    pub fn source(&self) -> SourceId {
        self.src.id
    }

    /// Index of the first programme starting after `t`.
    fn first_after(&self, t: UnixTime) -> usize {
        self.sched
            .slots
            .partition_point(|s| self.sched.start(s) <= t)
    }

    /// Index of the programme airing at `t`.
    pub fn index_at(&self, t: UnixTime) -> Option<usize> {
        let i = self.first_after(t).checked_sub(1)?;
        (self.sched.stop(&self.sched.slots[i]) > t).then_some(i)
    }

    /// The programme airing at `t` (`start <= t < stop`).
    pub fn at(&self, t: UnixTime) -> Option<ProgrammeRef<'a>> {
        self.get(self.index_at(t)?)
    }

    /// The first programme that starts after `t`.
    pub fn next_after(&self, t: UnixTime) -> Option<ProgrammeRef<'a>> {
        self.get(self.first_after(t))
    }

    /// Index range of programmes overlapping `[t0, t1)`.
    pub fn range_indices(&self, t0: UnixTime, t1: UnixTime) -> std::ops::Range<usize> {
        let slots = &self.sched.slots;
        // Non-overlapping and sorted by start, so stops are sorted too.
        let lo = slots.partition_point(|s| self.sched.stop(s) <= t0);
        let hi = slots.partition_point(|s| self.sched.start(s) < t1);
        lo..hi.max(lo)
    }

    /// Programmes overlapping `[t0, t1)`, in time order.
    pub fn range(
        self,
        t0: UnixTime,
        t1: UnixTime,
    ) -> impl ExactSizeIterator<Item = ProgrammeRef<'a>> + 'a {
        self.range_indices(t0, t1)
            .map(move |i| self.get(i).expect("in range"))
    }

    pub fn iter(self) -> impl ExactSizeIterator<Item = ProgrammeRef<'a>> + 'a {
        (0..self.len()).map(move |i| self.get(i).expect("in range"))
    }

    /// Current and next programme at `now`.
    pub fn now_next(&self, now: UnixTime) -> NowNext<'a> {
        let i = self.first_after(now);
        let current = i
            .checked_sub(1)
            .and_then(|j| self.get(j))
            .filter(|p| p.stop() > now);
        NowNext {
            now: current,
            next: self.get(i),
        }
    }
}

/// What is on now and what comes next on one channel.
#[derive(Clone, Copy, Debug, Default)]
pub struct NowNext<'a> {
    pub now: Option<ProgrammeRef<'a>>,
    pub next: Option<ProgrammeRef<'a>>,
}

/// Guide data from several sources, bound to playlist channels.
///
/// Binding: for each playlist channel the candidates from
/// [`crate::MatchResult`] are tried in descending source priority, and the
/// first source with a non-empty schedule for that EPG id wins.
#[derive(Default)]
pub struct GuideStore {
    /// Highest priority first; equal priorities keep insertion order.
    sources: Vec<EpgSource>,
    wanted: HashMap<ChannelKey, Vec<EpgRef>>,
    /// Source index, schedule index, candidate index in `wanted`.
    bound: HashMap<ChannelKey, (u32, u32, u32)>,
}

impl GuideStore {
    pub fn new() -> Self {
        GuideStore::default()
    }

    /// Adds a source, replacing one with the same id, and rebinds.
    pub fn replace_source(&mut self, src: EpgSource) {
        self.sources.retain(|s| s.id != src.id);
        let at = self
            .sources
            .iter()
            .position(|s| s.priority < src.priority)
            .unwrap_or(self.sources.len());
        self.sources.insert(at, src);
        self.rebind();
    }

    pub fn remove_source(&mut self, id: SourceId) {
        self.sources.retain(|s| s.id != id);
        self.rebind();
    }

    pub fn sources(&self) -> &[EpgSource] {
        &self.sources
    }

    pub fn source(&self, id: SourceId) -> Option<&EpgSource> {
        self.sources.iter().find(|s| s.id == id)
    }

    /// Sets the playlist → EPG candidates (usually
    /// [`crate::MatchResult::matches`]) and rebinds.
    pub fn set_matches(&mut self, matches: HashMap<ChannelKey, Vec<EpgRef>>) {
        self.wanted = matches;
        self.rebind();
    }

    /// Drops programmes that ended more than `past_days` before `now`.
    pub fn prune(&mut self, now: UnixTime, past_days: u32) {
        let cutoff = now - i64::from(past_days) * DAY;
        for s in &mut self.sources {
            s.prune(cutoff);
        }
        self.rebind();
    }

    fn rebind(&mut self) {
        self.bound.clear();
        let rank: HashMap<SourceId, usize> = self
            .sources
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id, i))
            .collect();
        for (key, cands) in &self.wanted {
            let best = cands
                .iter()
                .enumerate()
                .filter_map(|(ci, c)| {
                    let si = *rank.get(&c.source)?;
                    let sched = *self.sources[si].by_epg_id.get(c.epg_id.as_str())?;
                    let empty = self.sources[si].schedules[sched as usize].slots.is_empty();
                    (!empty).then_some((si, sched, ci))
                })
                .min_by_key(|&(si, _, _)| si);
            if let Some((si, sched, ci)) = best {
                self.bound
                    .insert(key.clone(), (si as u32, sched, ci as u32));
            }
        }
    }

    /// Number of playlist channels that have guide data.
    pub fn bound_count(&self) -> usize {
        self.bound.len()
    }

    /// The source and EPG id a playlist channel's guide comes from.
    pub fn binding(&self, key: &ChannelKey) -> Option<(SourceId, &str)> {
        let &(si, _, cand) = self.bound.get(key)?;
        let c = &self.wanted.get(key)?[cand as usize];
        Some((self.sources[si as usize].id, c.epg_id.as_str()))
    }

    pub fn schedule(&self, key: &ChannelKey) -> Option<ScheduleRef<'_>> {
        let &(si, sched, _) = self.bound.get(key)?;
        let src = &self.sources[si as usize];
        Some(ScheduleRef {
            src,
            sched: &src.schedules[sched as usize],
        })
    }

    /// Programmes of `key` overlapping `[t0, t1)`; empty when unbound.
    pub fn programmes(
        &self,
        key: &ChannelKey,
        t0: UnixTime,
        t1: UnixTime,
    ) -> impl Iterator<Item = ProgrammeRef<'_>> {
        self.schedule(key)
            .into_iter()
            .flat_map(move |s| s.range(t0, t1))
    }

    pub fn programme_at(&self, key: &ChannelKey, t: UnixTime) -> Option<ProgrammeRef<'_>> {
        self.schedule(key)?.at(t)
    }

    pub fn next_after(&self, key: &ChannelKey, t: UnixTime) -> Option<ProgrammeRef<'_>> {
        self.schedule(key)?.next_after(t)
    }

    /// Now/next for a batch of channels, in input order.
    pub fn now_next<'k>(
        &self,
        keys: impl IntoIterator<Item = &'k ChannelKey>,
        now: UnixTime,
    ) -> Vec<NowNext<'_>> {
        keys.into_iter()
            .map(|k| {
                self.schedule(k)
                    .map(|s| s.now_next(now))
                    .unwrap_or_default()
            })
            .collect()
    }

    /// Rough heap usage of all sources in bytes.
    pub fn heap_bytes(&self) -> usize {
        self.sources.iter().map(EpgSource::heap_bytes).sum()
    }
}
