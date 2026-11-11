mod common;
use common::*;
use iptv_store_sqlite::{SourcePolicy, Store};
#[test]
fn policy_and_group_edits_survive_refresh_restart_and_are_source_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let mut s = Store::open(&path).unwrap();
    let src = s.add_source(&m3u_source("One")).unwrap();
    let other = s.add_source(&m3u_source("Two")).unwrap();
    let cat = catalog(8, 2, XT_PASSWORD);
    replace(&mut s, src, &cat);
    replace(&mut s, other, &cat);
    let p = SourcePolicy {
        enabled: false,
        refresh_seconds: 3600,
        refresh_on_start: true,
    };
    s.set_source_policy(src, p).unwrap();
    s.rename_group(src, &group(0).id, Some(" Local name "))
        .unwrap();
    s.hide_group(src, &group(0).id, true).unwrap();
    s.move_group(src, &group(1).id, 0).unwrap();
    replace(&mut s, src, &cat);
    drop(s);
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.source_policy(src).unwrap(), p);
    assert_eq!(s.source_policy(other).unwrap(), SourcePolicy::default());
    let groups = s.library_groups(src).unwrap();
    assert_eq!(groups[0].group.id, group(1).id);
    assert_eq!(groups[1].name, "Local name");
    assert!(groups[1].hidden);
    assert_eq!(groups[1].group.name, group(0).name);
    assert!(!s.library_groups(other).unwrap()[0].hidden);
    s.rename_group(src, &group(0).id, None).unwrap();
    s.hide_group(src, &group(0).id, false).unwrap();
    s.reset_group_order(src).unwrap();
    let groups = s.library_groups(src).unwrap();
    assert_eq!(groups[0].name, group(0).name);
    assert!(!groups[0].hidden);
    s.delete_source(src).unwrap();
    assert!(s.source_policy(src).is_err());
}

#[test]
fn bulk_visibility_is_source_scoped_and_retains_names_and_favorites() {
    use iptv_store_sqlite::ChannelQuery;
    let mut s = Store::open_in_memory().unwrap();
    let source = s.add_source(&m3u_source("Local")).unwrap();
    let cat = catalog(4, 2, XT_PASSWORD);
    replace(&mut s, source, &cat);
    let id = cat.channels[0].id.clone();
    s.rename_channel(source, &id, Some("Local title")).unwrap();
    s.set_favourite(source, &id, true).unwrap();
    let ids = cat
        .channels
        .iter()
        .map(|c| c.id.as_str().to_owned())
        .collect::<Vec<_>>();
    s.set_library_visibility(source, &ids, false, true).unwrap();
    assert!(
        s.load_channels(source, &ChannelQuery::default())
            .unwrap()
            .is_empty()
    );
    replace(&mut s, source, &cat);
    s.set_library_visibility(source, &ids, false, false)
        .unwrap();
    let rows = s.load_channels(source, &ChannelQuery::default()).unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].name, "Local title");
    assert!(rows[0].favourite);
}

fn live_catalog(channels: usize, groups: usize) -> common::Catalog {
    let mut cat = catalog(channels, groups, XT_PASSWORD);
    cat.groups
        .retain(|g| g.kind == iptv_providers::model::ContentKind::Live);
    cat.vod.clear();
    cat.series.clear();
    cat
}

#[test]
fn group_bulk_edits_keep_order_names_and_other_playlists_isolated() {
    use iptv_providers::GroupId;
    let mut store = Store::open_in_memory().unwrap();
    let source = store.add_source(&m3u_source("A")).unwrap();
    let other = store.add_source(&m3u_source("B")).unwrap();
    let cat = live_catalog(6, 3);
    replace(&mut store, source, &cat);
    replace(&mut store, other, &cat);
    store
        .rename_group(source, &group(1).id, Some("Local group"))
        .unwrap();
    store.move_group(source, &group(1).id, 0).unwrap();
    let ids: Vec<_> = (0..3).map(|i| group(i).id.as_str().to_owned()).collect();
    store
        .set_library_visibility(source, &ids, true, true)
        .unwrap();
    assert!(
        store
            .library_groups(source)
            .unwrap()
            .iter()
            .all(|r| r.hidden)
    );
    assert!(
        store
            .library_groups(other)
            .unwrap()
            .iter()
            .all(|r| !r.hidden)
    );
    store
        .set_library_visibility(source, &ids[1..2], true, false)
        .unwrap();
    let rows = store.library_groups(source).unwrap();
    assert_eq!(rows[0].name, "Local group");
    assert!(!rows[0].hidden);
    assert!(rows[1].hidden);
    store.move_group(source, &group(1).id, usize::MAX).unwrap();
    assert_eq!(
        store.library_groups(source).unwrap()[2].group.id,
        group(1).id
    );
    let before = store.library_groups(source).unwrap();
    store
        .move_group(source, &GroupId::from_stored("missing"), 0)
        .unwrap();
    assert_eq!(store.library_groups(source).unwrap(), before);
    store.reset_group_order(source).unwrap();
    store
        .rename_group(source, &group(1).id, Some("  "))
        .unwrap();
    let rows = store.library_groups(source).unwrap();
    assert_eq!(rows[1].name, group(1).name);
    assert_eq!(rows[0].group.id, group(0).id);
    assert!(rows[0].hidden);
}

#[test]
fn temporarily_absent_groups_regain_overlays_and_empty_bulk_edits_are_noops() {
    use iptv_store_sqlite::ChannelQuery;
    let mut store = Store::open_in_memory().unwrap();
    let source = store.add_source(&m3u_source("A")).unwrap();
    let cat = live_catalog(6, 2);
    replace(&mut store, source, &cat);
    store
        .rename_group(source, &group(1).id, Some("Retained name"))
        .unwrap();
    store.hide_group(source, &group(1).id, true).unwrap();
    store.move_group(source, &group(1).id, 0).unwrap();
    replace(&mut store, source, &live_catalog(3, 1));
    assert_eq!(store.library_groups(source).unwrap().len(), 1);
    replace(&mut store, source, &cat);
    let groups = store.library_groups(source).unwrap();
    assert_eq!(groups[0].group.id, group(1).id);
    assert_eq!(groups[0].name, "Retained name");
    assert!(groups[0].hidden);
    store
        .set_library_visibility(source, &[], true, false)
        .unwrap();
    assert_eq!(store.library_groups(source).unwrap(), groups);
    let ids: Vec<_> = cat
        .channels
        .iter()
        .map(|c| c.id.as_str().to_owned())
        .collect();
    store
        .set_library_visibility(source, &ids, false, true)
        .unwrap();
    store
        .set_library_visibility(source, &ids, false, false)
        .unwrap();
    assert_eq!(
        store
            .load_channels(source, &ChannelQuery::default())
            .unwrap()
            .len(),
        6
    );
    replace(&mut store, source, &live_catalog(0, 0));
    assert!(
        store.orphaned_overlays(Some(source)).unwrap().is_empty(),
        "restoring default visibility leaves no phantom channel customizations"
    );
}

#[test]
fn policies_and_bulk_edits_require_a_source_and_delete_cascades_preferences() {
    use iptv_store_sqlite::SourceId;
    let mut store = Store::open_in_memory().unwrap();
    let missing = SourceId(999);
    assert!(store.source_policy(missing).is_err());
    assert!(
        store
            .set_source_policy(missing, SourcePolicy::default())
            .is_err()
    );
    assert!(
        store
            .set_library_visibility(missing, &["generated".into()], false, true)
            .is_err()
    );
    assert!(
        store
            .rename_group(missing, &group(0).id, Some("Name"))
            .is_err()
    );
    assert!(store.hide_group(missing, &group(0).id, true).is_err());
    let source = store.add_source(&m3u_source("A")).unwrap();
    assert_eq!(
        store.source_policy(source).unwrap(),
        SourcePolicy::default()
    );
    replace(&mut store, source, &catalog(2, 1, XT_PASSWORD));
    store
        .set_source_policy(
            source,
            SourcePolicy {
                enabled: false,
                refresh_seconds: 86400,
                refresh_on_start: false,
            },
        )
        .unwrap();
    store.hide_group(source, &group(0).id, true).unwrap();
    store.delete_source(source).unwrap();
    let fresh = store.add_source(&m3u_source("New")).unwrap();
    replace(&mut store, fresh, &catalog(2, 1, XT_PASSWORD));
    assert_eq!(store.source_policy(fresh).unwrap(), SourcePolicy::default());
    assert!(!store.library_groups(fresh).unwrap()[0].hidden);
}
