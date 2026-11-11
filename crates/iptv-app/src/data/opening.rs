//! A stored source opening (a cold start, or a switch to another stored
//! playlist): what the store has is on screen before any network, in the
//! order a viewer needs it.
//!
//! On the UI thread, as soon as the store is open ([`begin`], cold start
//! only): the guide's shell replaces the synthetic guide (no channels, so
//! nothing made up flashes and nothing is laid out twice), and the last
//! channel starts from its stored row (`player/resume.rs`).
//!
//! On the import thread ([`send_stored`]): the channels with the first
//! screen's programmes ([`first_rows`] around the channel the guide opens
//! on, [`first_screen`]: the hour before now to [`FIRST_SCREEN_HOURS`] on),
//! shown with every other channel and time loading
//! (`EpgSchedule::held_only`, `held_channels`); then the whole window
//! (`stored::load_guide`), which replaces it. On a TV box the
//! window read takes over a second; the first screen's a fraction of it
//! (`docs/perf.md`).

use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::Instant;

use iptv_epg::{ChannelKey, GuideStore, UnixTime};
use iptv_guide::EpgSchedule;
use iptv_providers::model::Channel;
use iptv_store_sqlite::{EpgWindow, SourceId, Store, StoredSource};

use super::{Imported, Job, Msg, Streams, stored};
use crate::Ui;

/// Hours of programmes after the hour of now read first.
pub const FIRST_SCREEN_HOURS: i64 = 6;
/// Channels read first either side of the one the guide opens on: a
/// screen's rows and more.
pub const FIRST_SCREEN_ROWS: usize = 30;

/// What a first screen holds: the programmes of `keys` overlapping
/// `[from, to)`.
pub struct Held {
    pub from: UnixTime,
    pub to: UnixTime,
    pub keys: Vec<ChannelKey>,
}

/// The times the first screen's read holds: from the hour before now's to
/// [`FIRST_SCREEN_HOURS`] after now's hour.
pub fn first_screen(now: UnixTime) -> (UnixTime, UnixTime) {
    let hour = now - now.rem_euclid(3600);
    (hour - 3600, hour + FIRST_SCREEN_HOURS * 3600)
}

/// The channels around the one the guide opens on, `focus` (a channel key;
/// the first channel without one or when it is gone), in playlist order
/// (the guide's list at a cold start: all channels).
pub fn first_rows(channels: &[Channel], focus: Option<&str>) -> Vec<ChannelKey> {
    let at = focus
        .and_then(|key| channels.iter().position(|c| c.id.as_str() == key))
        .unwrap_or(0);
    let from = at.saturating_sub(FIRST_SCREEN_ROWS);
    let to = (at + FIRST_SCREEN_ROWS + 1).min(channels.len());
    channels[from..to]
        .iter()
        .map(|c| ChannelKey::new(c.id.as_str()))
        .collect()
}

/// Cold start with stored source `stored` (the store open): the guide's
/// empty shell, and the last channel when it can start at once.
pub fn begin(ui: Ui, store: &Store, stored: &StoredSource) {
    let shell = EpgSchedule::new(Vec::new(), GuideStore::default(), super::now_unix());
    crate::set_schedule(&ui, Rc::new(shell));
    crate::player::resume::early(ui, store, stored);
}

/// Sends `id`'s stored channels with the first screen's programmes, then
/// with the whole guide window. Nothing when it has no channels.
pub(super) fn send_stored(job: &Job, id: SourceId, now: UnixTime, tx: &Sender<Msg>) {
    let started = Instant::now();
    let ms = |t: Instant| t.elapsed().as_millis();
    let (groups, channels) = match super::open_store().and_then(|s| stored::catalog_in(&s, id)) {
        Ok(catalog) => catalog,
        Err(e) => {
            log::warn!("iptv data: stored catalog: {e}");
            return;
        }
    };
    if channels.is_empty() {
        return;
    }
    let n = channels.len();
    log::info!("iptv data: showing {n} stored channels before the refresh");
    crate::startup::mark_with("channels read", format_args!("{n} in {} ms", ms(started)));
    let imported = |groups, channels, guide| {
        let context = crate::player::stored_context(&job.source.config);
        Box::new(Imported {
            groups,
            channels,
            resume: context.is_some(),
            playback: Streams::New(context, None),
            guide,
            now,
            source: Some(id),
        })
    };
    let started = Instant::now();
    let (from, to) = first_screen(now);
    let resumes = crate::settings::flag(iptv_settings::keys::RESUME_LAST_CHANNEL);
    let focus = crate::player::resume::last_key(id).filter(|_| resumes);
    let keys = first_rows(&channels, focus.as_deref());
    let window = EpgWindow {
        from,
        to,
        keys: Some(&keys),
        ..EpgWindow::from(now, 0)
    };
    if let Some(guide) = stored::read_guide(id, &window) {
        let kib = guide.heap_bytes() / 1024;
        crate::startup::mark_with(
            "first screen read",
            format_args!("{} channels, {kib} KiB in {} ms", keys.len(), ms(started)),
        );
        let first = imported(groups.clone(), channels.clone(), guide);
        let held = Held { from, to, keys };
        let _ = tx.send(Msg::FirstScreen(first, held));
    }
    let started = Instant::now();
    let guide = stored::load_guide(id, now).unwrap_or_default();
    let (bound, kib) = (guide.bound_count(), guide.heap_bytes() / 1024);
    crate::startup::mark_with(
        "guide window read",
        format_args!("{bound} channels, {kib} KiB in {} ms", ms(started)),
    );
    let _ = tx.send(Msg::Guide(imported(groups, channels, guide)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channels(n: usize) -> Vec<Channel> {
        let mut m3u = String::from("#EXTM3U\n");
        for i in 0..n {
            m3u.push_str(&format!(
                "#EXTINF:-1 tvg-id=\"c{i}\",C{i}\nhttp://s.example.test/{i}.m3u8\n"
            ));
        }
        iptv_providers::m3u::parse_m3u(&m3u).channels.items
    }

    #[test]
    fn the_first_rows_are_around_the_last_channel() {
        let all = channels(100);
        let key = |i: usize| ChannelKey::new(all[i].id.as_str());
        let rows = first_rows(&all, Some(all[50].id.as_str()));
        assert_eq!(rows.len(), 2 * FIRST_SCREEN_ROWS + 1);
        assert_eq!((rows[0].clone(), rows[60].clone()), (key(20), key(80)));
        // Near the start, without one, or with one gone: from the top.
        let top = first_rows(&all, Some(all[3].id.as_str()));
        assert_eq!((top[0].clone(), top.len()), (key(0), 34));
        assert_eq!(first_rows(&all, None)[0], key(0));
        assert_eq!(first_rows(&all, Some("gone")).len(), FIRST_SCREEN_ROWS + 1);
        assert_eq!(first_rows(&all[..5], None).len(), 5);
    }

    #[test]
    fn the_first_screen_is_the_hour_before_now_to_six_hours_on() {
        let hour = 1_800_000_000 - 1_800_000_000 % 3600;
        assert_eq!(first_screen(hour + 1234), (hour - 3600, hour + 6 * 3600));
        assert_eq!(first_screen(hour), (hour - 3600, hour + 6 * 3600));
    }
}
