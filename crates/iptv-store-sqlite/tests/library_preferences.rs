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
