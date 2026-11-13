//! Apply user organization to the guide without replacing playback channels.
use crate::Ui;
use iptv_guide::{EpgSchedule, Organization};
use iptv_store_sqlite::{ChannelQuery, SourceId, Store, StoreError};

thread_local! { static REVISION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
pub fn revision() -> u64 {
    REVISION.get()
}

pub fn read_in(store: &Store, source: SourceId) -> Result<Organization, StoreError> {
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
struct Snapshot {
    organization: Organization,
    lists: Vec<(String, String, Vec<String>)>,
}
async fn load(source: SourceId) -> Result<Snapshot, StoreError> {
    crate::store_service::call(move |store| {
        Ok(Snapshot {
            organization: read_in(store, source)?,
            lists: group_lists_in(store, source)?,
        })
    })
    .await
}
fn install(schedule: &EpgSchedule, snapshot: Snapshot) {
    schedule.set_organization(snapshot.organization);
    schedule.set_group_lists(snapshot.lists);
}
pub async fn apply(schedule: &EpgSchedule, source: SourceId) {
    match load(source).await {
        Ok(snapshot) => install(schedule, snapshot),
        Err(error) => log::warn!("iptv organization: store unavailable: {error}"),
    }
}
pub fn changed(ui: Ui, source: SourceId) {
    REVISION.set(REVISION.get().wrapping_add(1));
    ui.library.version.update(|v| *v += 1);
    if super::combined::active(ui) {
        super::combined::organization_changed(ui, source);
        return;
    }
    if ui.library.shown.get_untracked() != Some(source) {
        return;
    }
    let schedule = crate::schedule();
    super::spawn_in_app(async move {
        if let Some(epg) = schedule
            .as_any()
            .and_then(|s| s.downcast_ref::<EpgSchedule>())
        {
            let Ok(snapshot) = load(source).await else {
                return;
            };
            if std::rc::Rc::ptr_eq(&schedule, &crate::schedule()) {
                let kept = crate::lists::keep(ui);
                install(epg, snapshot);
                crate::lists::restore(ui, kept);
                ui.data_version.update(|v| *v += 1);
            }
        }
    });
}

/// Includes empty custom groups so their membership can be populated later.
fn group_lists_in(
    store: &Store,
    source: SourceId,
) -> Result<Vec<(String, String, Vec<String>)>, StoreError> {
    use iptv_store_sqlite::ChannelScope;
    let mut lists = Vec::new();
    for g in store
        .library_groups(source)?
        .into_iter()
        .filter(|g| !g.hidden && g.group.kind == iptv_providers::model::ContentKind::Live)
    {
        let rows = store.load_channels(
            source,
            &ChannelQuery {
                scope: ChannelScope::Group(g.group.id.clone()),
                include_hidden: true,
                ..Default::default()
            },
        )?;
        lists.push((
            g.group.id.as_str().to_owned(),
            g.name,
            rows.into_iter()
                .map(|r| r.channel.id.as_str().to_owned())
                .collect(),
        ));
    }
    for g in store.custom_groups(source)? {
        let rows = store.load_channels(
            source,
            &ChannelQuery {
                scope: ChannelScope::Custom(g.id),
                include_hidden: true,
                ..Default::default()
            },
        )?;
        lists.push((
            format!("custom:{}", g.id.0),
            g.name,
            rows.into_iter()
                .map(|r| r.channel.id.as_str().to_owned())
                .collect(),
        ));
    }
    Ok(lists)
}
