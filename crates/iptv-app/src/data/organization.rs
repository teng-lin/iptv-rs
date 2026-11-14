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
/// Every caller loads after the change it shows was committed (an edit's
/// reply, a refresh's publish), so the reader sees it.
async fn load(source: SourceId) -> Result<Snapshot, StoreError> {
    crate::store_service::read(move |store| {
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

/// The schedule of an imported catalog with `source`'s organisation, built
/// off the UI thread.
///
/// Measured with a generated 100,000-channel, 1,000-group playlist, both
/// steps then on the UI thread: grouping the catalog took 70-100 ms and
/// organising it 50-65 ms on a desktop (release build), and 136-171 ms and
/// 126-173 ms on the Android TV emulator, twice per cold start (the stored
/// guide, then the refreshed one). So the data is made on the shared
/// runtime's blocking threads (`EpgSchedule::prepare`) and only turned into
/// the schedule here, which moves it.
pub async fn build(
    source: SourceId,
    groups: Vec<iptv_providers::model::Group>,
    channels: Vec<iptv_providers::model::Channel>,
    guide: iptv_epg::GuideStore,
    now: iptv_epg::UnixTime,
) -> EpgSchedule {
    let organization = match load(source).await {
        Ok(snapshot) => Some((snapshot.organization, snapshot.lists)),
        Err(error) => {
            log::warn!("iptv organization: store unavailable: {error}");
            None
        }
    };
    let count = channels.len();
    let started = std::time::Instant::now();
    let prepared = crate::executor::spawn_blocking(move || {
        let started = std::time::Instant::now();
        let prepared = EpgSchedule::prepare(&groups, &channels, guide, now, organization);
        (prepared, started.elapsed())
    })
    .await;
    match prepared {
        Ok((prepared, took)) => {
            let schedule = EpgSchedule::from_prepared(prepared);
            log::info!(
                "iptv data: schedule of {count} channels built in {} ms off the UI thread \
                 ({} ms until here)",
                took.as_millis(),
                started.elapsed().as_millis()
            );
            schedule
        }
        Err(_) => {
            log::error!("iptv data: building a schedule failed; showing it empty");
            EpgSchedule::new(Vec::new(), iptv_epg::GuideStore::default(), now)
        }
    }
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
