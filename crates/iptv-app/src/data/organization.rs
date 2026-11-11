//! Apply user organization to the guide without replacing playback channels.
use super::open_store;
use crate::Ui;
use iptv_guide::{EpgSchedule, Organization};
use iptv_store_sqlite::{ChannelQuery, SourceId, StoreError};

pub fn read(source: SourceId) -> Result<Organization, StoreError> {
    let store = open_store()?;
    let channels = store
        .load_channels(
            source,
            &ChannelQuery {
                include_hidden: true,
                ..Default::default()
            },
        )?
        .into_iter()
        .map(|r| (r.channel.id.as_str().to_owned(), r.name, r.hidden))
        .collect();
    let groups = store
        .library_groups(source)?
        .into_iter()
        .filter(|r| r.group.kind == iptv_providers::model::ContentKind::Live)
        .map(|r| (r.group.id.as_str().to_owned(), r.name, r.hidden))
        .collect();
    Ok(Organization { channels, groups })
}
pub fn apply(schedule: &EpgSchedule, source: Option<SourceId>) {
    if let Some(source) = source {
        match read(source) {
            Ok(org) => schedule.set_organization(org),
            Err(e) => log::warn!("iptv organization: store unavailable: {e}"),
        }
    }
}
pub fn changed(ui: Ui, source: SourceId) {
    ui.library.version.update(|v| *v += 1);
    if ui.library.shown.get_untracked() != Some(source) {
        return;
    }
    let kept = crate::lists::keep(ui);
    let schedule = crate::schedule();
    if let Some(epg) = schedule
        .as_any()
        .and_then(|s| s.downcast_ref::<EpgSchedule>())
    {
        apply(epg, Some(source));
        crate::lists::restore(ui, kept);
        ui.data_version.update(|v| *v += 1);
    }
}
