//! The programme index of the search screen, built on its own thread from
//! every day the store keeps (`Store::for_each_epg_title`): the guide in
//! memory holds only the last day (`data/pages.rs`), and search reaches
//! back as far as the store does. A guide that is not stored (none on
//! screen) is indexed from memory.

use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use iptv_core::guide::Schedule;
use iptv_epg::{ChannelKey, DAY, UnixTime};
use iptv_guide::{EpgSchedule, combined::CombinedSchedule};
use iptv_search::{ProgrammeIndex, ProgrammeIndexBuilder};
use nsh_core::prelude::*;

const POLL: Duration = Duration::from_millis(50);

/// Builds the index of `epg`'s channels (by row) and hands it to `done` on
/// the UI thread (not at all when the store cannot be read).
pub fn build(schedule: &dyn Schedule, rows: usize, done: impl FnOnce(ProgrammeIndex) + 'static) {
    let mut sources: Vec<(iptv_store_sqlite::SourceId, Vec<ChannelKey>, usize)> = Vec::new();
    if let Some(all) = schedule
        .as_any()
        .and_then(|s| s.downcast_ref::<CombinedSchedule>())
    {
        let mut offset = 0;
        for part in all.sources() {
            let keys = (0..part.schedule.channel_count())
                .filter_map(|c| part.schedule.channel(c).map(|c| c.key.clone()))
                .collect();
            sources.push((iptv_store_sqlite::SourceId(part.id), keys, offset));
            offset += part.schedule.channel_count();
        }
    } else if let Some(epg) = schedule
        .as_any()
        .and_then(|s| s.downcast_ref::<EpgSchedule>())
    {
        let keys: Vec<_> = (0..rows)
            .map_while(|c| epg.channel(c).map(|g| g.key.clone()))
            .collect();
        let Some(source) = crate::data::stored::shown().filter(|_| epg.loaded_from().is_some())
        else {
            done(ProgrammeIndex::build(epg.store(), &keys));
            return;
        };
        sources.push((source, keys, 0));
    } else {
        return;
    }
    let keep = crate::data::stored::keep_past_days(&crate::settings::get());
    let from: UnixTime =
        crate::time::wall_unix().saturating_sub(i64::from(keep).saturating_mul(DAY));
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let started = Instant::now();
        let mut b = ProgrammeIndexBuilder::new(rows);
        let read = crate::data::open_store().and_then(|s| {
            for (source, keys, offset) in sources {
                s.for_each_epg_title(
                    source,
                    &keys,
                    from,
                    UnixTime::MAX,
                    |c, start, stop, title| b.push(c + offset, start, stop, title),
                )?;
            }
            Ok(())
        });
        let index = match read {
            Ok(_) => Some(b.finish()),
            Err(e) => {
                log::warn!("iptv search: guide titles: {e}");
                None
            }
        };
        let _ = tx.send((index, started.elapsed()));
    });
    crate::data::spawn_in_app(async move {
        loop {
            match rx.try_recv() {
                Ok((Some(index), took)) => {
                    log::info!(
                        "iptv search: read {} stored programmes in {} ms",
                        index.programme_count(),
                        took.as_millis()
                    );
                    return done(index);
                }
                Ok((None, _)) | Err(TryRecvError::Disconnected) => return,
                Err(TryRecvError::Empty) => timer(POLL).await,
            }
        }
    });
}
