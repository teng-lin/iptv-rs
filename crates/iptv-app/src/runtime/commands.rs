//! Named library operations. Views send values and await results; only this
//! module creates database work items.
use super::store_service;
use iptv_core::membership::Member;
use iptv_library::{CustomGroupId, SourceId};
use iptv_providers::ContentId;
use iptv_store_sqlite::{ChannelQuery, ChannelScope, CustomGroup, Store, StoreError};
use std::future::Future;

pub(crate) fn custom_groups(
    source: SourceId,
) -> impl Future<Output = Result<Vec<CustomGroup>, StoreError>> {
    store_service::read(move |store| store.custom_groups(source))
}

fn member_rows(
    store: &Store,
    source: SourceId,
    scope: ChannelScope,
) -> Result<Vec<Member>, StoreError> {
    Ok(store
        .load_channels(
            source,
            &ChannelQuery {
                scope,
                include_hidden: true,
                ..Default::default()
            },
        )?
        .into_iter()
        .map(|row| Member {
            id: row.channel.id.as_str().into(),
            name: row.name,
        })
        .collect())
}

pub(crate) fn members(
    source: SourceId,
    scope: ChannelScope,
) -> impl Future<Output = Result<Vec<Member>, StoreError>> {
    store_service::read(move |store| member_rows(store, source, scope))
}

pub(crate) fn group_members(
    source: SourceId,
    scope: ChannelScope,
) -> impl Future<Output = Result<(Vec<CustomGroup>, Vec<Member>), StoreError>> {
    store_service::read(move |store| {
        Ok((
            store.custom_groups(source)?,
            member_rows(store, source, scope)?,
        ))
    })
}

pub(crate) fn save_custom_group(
    source: SourceId,
    group: Option<CustomGroupId>,
    name: String,
) -> impl Future<Output = Result<(), StoreError>> {
    store_service::call(move |store| match group {
        Some(group) => store.rename_custom_group(group, &name),
        None => store.create_custom_group(source, &name).map(|_| ()),
    })
}

pub(crate) fn remove_custom_members(
    group: CustomGroupId,
    members: Vec<ContentId>,
) -> impl Future<Output = Result<(), StoreError>> {
    store_service::call(move |store| store.remove_custom_members(group, &members))
}

pub(crate) fn transfer_custom_members(
    source: SourceId,
    from: Option<CustomGroupId>,
    to: CustomGroupId,
    members: Vec<ContentId>,
    moving: bool,
) -> impl Future<Output = Result<usize, StoreError>> {
    store_service::call(move |store| {
        store.transfer_custom_members(source, from, to, &members, moving)
    })
}

pub(crate) fn delete_custom_group(
    group: CustomGroupId,
) -> impl Future<Output = Result<(), StoreError>> {
    store_service::call(move |store| store.delete_custom_group(group).map(|_| ()))
}

pub(crate) fn hide_channel(
    source: SourceId,
    channel: String,
) -> impl Future<Output = Result<(), StoreError>> {
    store_service::call(move |store| {
        store.set_hidden(source, &ContentId::from_stored(channel), true)
    })
}

pub(crate) fn remember_combined_channel(
    key: String,
) -> impl Future<Output = Result<(), StoreError>> {
    store_service::call(move |store| store.set_combined_last_channel(&key))
}

pub(crate) fn channel(
    source: SourceId,
    channel: ContentId,
) -> impl Future<Output = Result<Option<iptv_store_sqlite::ChannelRow>, StoreError>> {
    store_service::read(move |store| store.channel(source, &channel))
}

pub(crate) fn save_search_history(
    history: Vec<String>,
) -> impl Future<Output = Result<(), StoreError>> {
    store_service::call(move |store| store.set_search_history(&history))
}

use iptv_core::organization::{Action as Edit, Editor, Item};
use iptv_guide::Organization;
use iptv_library::OrganizationScope as Kind;
use iptv_providers::GroupId;
use iptv_store_sqlite::{ChannelOrder, OrderScope};

pub(crate) async fn organization_items(
    source: SourceId,
    kind: Kind,
    favorites: Vec<String>,
) -> Result<Vec<Item>, StoreError> {
    // Edits reload their rows only after the edit's reply: the reader sees it.
    store_service::read(move |store| {
        let org = organization_in(store, source)?;
        let rows = match kind {
            Kind::Channels => org.channels,
            Kind::Groups => org.groups,
            Kind::Favorites => {
                let names: std::collections::HashMap<_, _> = org
                    .channels
                    .into_iter()
                    .map(|(id, name, hidden)| (id, (name, hidden)))
                    .collect();
                favorites
                    .into_iter()
                    .filter_map(|id| {
                        names
                            .get(&id)
                            .map(|(name, hidden)| (id.clone(), name.clone(), *hidden))
                    })
                    .collect()
            }
            Kind::GroupChannels(group) => store
                .load_channels(
                    source,
                    &ChannelQuery {
                        scope: ChannelScope::Group(group),
                        include_hidden: true,
                        ..Default::default()
                    },
                )?
                .into_iter()
                .map(|r| (r.channel.id.as_str().to_owned(), r.name, r.hidden))
                .collect(),
            Kind::CustomChannels(group) => store
                .load_channels(
                    source,
                    &ChannelQuery {
                        scope: ChannelScope::Custom(group),
                        include_hidden: true,
                        ..Default::default()
                    },
                )?
                .into_iter()
                .map(|r| (r.channel.id.as_str().to_owned(), r.name, r.hidden))
                .collect(),
        };
        Ok(rows
            .into_iter()
            .map(|(id, name, hidden)| Item { id, name, hidden })
            .collect())
    })
    .await
}

pub(crate) fn organization_in(store: &Store, source: SourceId) -> Result<Organization, StoreError> {
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
pub(crate) fn edit_organization(
    source: SourceId,
    kind: Kind,
    editor: Editor,
    item: Item,
    edit: Edit,
) -> impl Future<Output = Result<(), StoreError>> {
    store_service::call(move |store| {
        let id = ContentId::from_stored(&item.id);
        let group = GroupId::from_stored(&item.id);
        let at = editor
            .items
            .iter()
            .position(|i| i.id == item.id)
            .unwrap_or(0);
        let to = match edit {
            Edit::Earlier => at.saturating_sub(1),
            Edit::Later => (at + 1).min(editor.items.len().saturating_sub(1)),
            Edit::First => 0,
            Edit::Last => editor.items.len().saturating_sub(1),
            _ => at,
        };
        match edit {
            Edit::Visibility => match &kind {
                Kind::Groups => store.hide_group(source, &group, !item.hidden)?,
                _ => store.set_hidden(source, &id, !item.hidden)?,
            },
            Edit::ResetName => match &kind {
                Kind::Groups => store.rename_group(source, &group, None)?,
                _ => store.rename_channel(source, &id, None)?,
            },
            Edit::Earlier | Edit::Later | Edit::First | Edit::Last => match &kind {
                Kind::Channels => {
                    store.move_channel(source, &OrderScope::All, &id, to)?;
                }
                Kind::GroupChannels(g) => {
                    store.move_channel(source, &OrderScope::Group(g.clone()), &id, to)?;
                }
                Kind::CustomChannels(g) => {
                    store.move_in_custom_group(*g, &id, to)?;
                }
                Kind::Favorites => unreachable!("handled before queuing"),
                Kind::Groups => store.move_group(source, &group, to)?,
            },
            Edit::ResetOrder => match &kind {
                Kind::Channels => store.reset_manual_order(source, &OrderScope::All)?,
                Kind::GroupChannels(g) => {
                    store.reset_manual_order(source, &OrderScope::Group(g.clone()))?
                }
                Kind::CustomChannels(g) => store.reset_custom_group_order(*g)?,
                Kind::Favorites => unreachable!("handled before queuing"),
                Kind::Groups => store.reset_group_order(source)?,
            },
            Edit::HideSelected | Edit::ShowSelected | Edit::HideAll | Edit::ShowAll => {
                let all = matches!(edit, Edit::HideAll | Edit::ShowAll);
                let hidden = matches!(edit, Edit::HideSelected | Edit::HideAll);
                let ids: Vec<_> = editor
                    .items
                    .iter()
                    .filter(|i| all || editor.selected.contains(&i.id))
                    .map(|i| i.id.clone())
                    .collect();
                store.set_library_visibility(source, &ids, kind == Kind::Groups, hidden)?;
            }
            _ => {}
        }
        Ok(())
    })
}

pub(crate) fn favorite_provider_order(
    source: SourceId,
    keys: Vec<String>,
) -> impl Future<Output = Result<Vec<String>, StoreError>> {
    store_service::call(move |store| {
        Ok(store
            .load_channels(
                source,
                &ChannelQuery {
                    include_hidden: true,
                    order: ChannelOrder::Provider,
                    ..Default::default()
                },
            )?
            .into_iter()
            .filter(|row| keys.iter().any(|key| key == row.channel.id.as_str()))
            .map(|row| row.channel.id.as_str().to_owned())
            .collect::<Vec<_>>())
    })
}

pub(crate) fn rename_organization_item(
    source: SourceId,
    kind: Kind,
    id: String,
    text: String,
) -> impl Future<Output = Result<(), StoreError>> {
    store_service::call(move |store| {
        let text = (!text.trim().is_empty()).then_some(text.trim());
        match kind {
            Kind::Groups => store.rename_group(source, &GroupId::from_stored(&id), text),
            _ => store.rename_channel(source, &ContentId::from_stored(&id), text),
        }
    })
}

pub(crate) fn import_legacy_history(
    store: &mut iptv_store_sqlite::Store,
    path: &std::path::Path,
) -> Result<(), iptv_store_sqlite::StoreError> {
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(());
    };
    let list: Vec<String> = serde_json::from_slice(&bytes).unwrap_or_default();
    if !list.is_empty() && store.search_history()?.is_empty() {
        let list = SearchHistory::from_list(list, SearchHistory::DEFAULT_CAPACITY).to_list();
        store.set_search_history(&list)?;
        log::info!(
            "iptv search: moved {} recent searches into the store",
            list.len()
        );
    }
    if let Err(error) = std::fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!(
            "iptv search: could not delete the old recent searches file ({})",
            error.kind()
        );
    }
    Ok(())
}

pub(crate) fn load_search_history(
    legacy: std::path::PathBuf,
) -> impl Future<Output = Result<Vec<String>, StoreError>> {
    store_service::call(move |store| {
        import_legacy_history(store, &legacy)?;
        store.search_history()
    })
}

use iptv_search::SearchHistory;

use super::state::OrganizationSnapshot;
pub(crate) async fn load_organization(
    source: SourceId,
) -> Result<OrganizationSnapshot, StoreError> {
    store_service::read(move |store| {
        Ok(OrganizationSnapshot {
            organization: organization_in(store, source)?,
            lists: group_lists_in(store, source)?,
        })
    })
    .await
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
