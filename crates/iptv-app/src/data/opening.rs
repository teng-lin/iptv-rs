//! Shell effects for an early cached playback start.
use crate::Ui;
pub use crate::runtime::opening::*;
use iptv_epg::GuideStore;
use iptv_guide::EpgSchedule;
use iptv_store_sqlite::StoredSource;
use std::rc::Rc;

/// Cold start with stored source `stored` (the store open): the guide's
/// empty shell, and the last channel when it can start at once.
pub async fn begin(ui: Ui, stored: &StoredSource) {
    let shell = EpgSchedule::new(Vec::new(), GuideStore::default(), super::now_unix());
    crate::runtime().emit(crate::runtime::RuntimeEvent::ScheduleReady(Rc::new(shell)));
    crate::player::resume::early(ui, stored).await;
}
