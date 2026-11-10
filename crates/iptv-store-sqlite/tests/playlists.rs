//! What the playlists screen keeps: the shown source, each source's guide
//! address, and what deleting a source takes with it.

mod common;

use common::*;
use iptv_providers::SecretUrl;
use iptv_store_sqlite::{ChannelQuery, SourceId, SourceSettings, Store, StoreError};

fn store() -> Store {
    let mut s = Store::open_in_memory().unwrap();
    s.set_clock(|| 1_800_000_000);
    s
}

fn typed() -> SecretUrl {
    SecretUrl::new("http://guide.example.test/xmltv.php?username=u&password=synthetic-pass-7f3a")
}

#[test]
fn the_shown_source_is_remembered_while_it_exists() {
    let mut s = store();
    assert_eq!(s.active_source().unwrap(), None);
    let a = s.add_source(&m3u_source("A")).unwrap();
    let b = s.add_source(&xtream_source("B")).unwrap();
    s.set_active_source(b).unwrap();
    assert_eq!(s.active_source().unwrap(), Some(b));
    s.set_active_source(a).unwrap();
    assert_eq!(s.active_source().unwrap(), Some(a));
    assert!(matches!(
        s.set_active_source(SourceId(99)),
        Err(StoreError::SourceNotFound(_))
    ));
    // Deleting another source keeps the choice; deleting it forgets it.
    s.delete_source(b).unwrap();
    assert_eq!(s.active_source().unwrap(), Some(a));
    s.delete_source(a).unwrap();
    assert_eq!(s.active_source().unwrap(), None);
    // A new source is not shown until chosen.
    let c = s.add_source(&m3u_source("C")).unwrap();
    assert_eq!(s.active_source().unwrap(), None);
    s.set_active_source(c).unwrap();
    assert_eq!(s.active_source().unwrap(), Some(c));
}

#[test]
fn a_guide_address_is_unset_set_or_removed() {
    let mut s = store();
    let a = s.add_source(&xtream_source("A")).unwrap();
    // Never saved: the app falls back to what an older build kept.
    assert_eq!(s.source_settings(a).unwrap(), None);
    s.set_source_epg_url(a, Some(&typed())).unwrap();
    let saved = s.source_settings(a).unwrap().unwrap();
    assert_eq!(saved.epg_url, Some(typed()));
    // Debug never shows the address's credentials.
    assert!(!format!("{saved:?}").contains(XT_PASSWORD));
    s.set_source_epg_url(a, None).unwrap();
    assert_eq!(
        s.source_settings(a).unwrap(),
        Some(SourceSettings { epg_url: None })
    );
    assert!(matches!(
        s.source_settings(SourceId(42)),
        Err(StoreError::SourceNotFound(_))
    ));
    assert!(matches!(
        s.set_source_epg_url(SourceId(42), None),
        Err(StoreError::SourceNotFound(_))
    ));
}

#[test]
fn editing_a_source_keeps_its_settings() {
    let mut s = store();
    let a = s.add_source(&xtream_source("A")).unwrap();
    s.set_source_epg_url(a, Some(&typed())).unwrap();
    s.rename_source(a, "Renamed").unwrap();
    s.update_source(a, &xtream_source("Again")).unwrap();
    assert_eq!(
        s.source_settings(a).unwrap().unwrap().epg_url,
        Some(typed())
    );
    assert_eq!(s.sources().unwrap()[0].name, "Again");
}

#[test]
fn deleting_a_source_takes_its_data_and_leaves_the_others() {
    let mut s = store();
    let a = s.add_source(&m3u_source("A")).unwrap();
    let b = s.add_source(&m3u_source("B")).unwrap();
    for id in [a, b] {
        replace(&mut s, id, &catalog(4, 2, "x"));
        s.set_source_epg_url(id, Some(&typed())).unwrap();
        let first = s.load_channels(id, &ChannelQuery::default()).unwrap()[0]
            .channel
            .id
            .clone();
        s.set_favourite(id, &first, true).unwrap();
    }
    assert!(s.delete_source(a).unwrap());
    assert_eq!(s.source(a).unwrap(), None);
    assert!(s.source_settings(a).is_err());
    assert!(
        s.load_channels(a, &ChannelQuery::default())
            .unwrap()
            .is_empty()
    );
    let only = s.sources().unwrap();
    assert_eq!(only.len(), 1);
    assert_eq!(only[0].id, b);
    assert_eq!(
        s.load_channels(b, &ChannelQuery::default()).unwrap().len(),
        4
    );
    let favourites = ChannelQuery {
        favourites_only: true,
        ..ChannelQuery::default()
    };
    assert_eq!(s.load_channels(b, &favourites).unwrap().len(), 1);
    assert!(s.source_settings(b).unwrap().unwrap().epg_url.is_some());
}
