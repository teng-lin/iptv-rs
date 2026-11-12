//! A private import cannot disturb the saved catalog until publication.
use crate::common;
use iptv_import::{CatalogRow, CatalogSink};
use iptv_store_sqlite::{CatalogStage, ChannelQuery, Store};

#[test]
fn incomplete_and_failed_steps_preserve_saved_rows_and_overlays() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("store.sqlite")).unwrap();
    let source = store.add_source(&common::m3u_source("Synthetic")).unwrap();
    let old = common::m3u_channel(1, 2, common::XT_PASSWORD);
    store
        .replace_catalog(source, &[], std::slice::from_ref(&old), &[], &[])
        .unwrap();
    store.set_favourite(source, &old.id, true).unwrap();
    let mut incomplete = CatalogStage::create_in(dir.path()).unwrap();
    incomplete
        .push(CatalogRow::Channel(common::m3u_channel(
            2,
            2,
            common::XT_PASSWORD,
        )))
        .unwrap();
    assert!(store.replace_catalog_stage(source, incomplete).is_err());
    assert_eq!(
        store
            .load_channels(source, &ChannelQuery::default())
            .unwrap()[0]
            .channel,
        old
    );

    let mut stage = CatalogStage::create_in(dir.path()).unwrap();
    stage.begin_step().unwrap();
    // Cross multiple flush boundaries, then roll back the entire failed step.
    for i in 2..1300 {
        stage
            .push(CatalogRow::Channel(common::m3u_channel(
                i,
                2,
                common::XT_PASSWORD,
            )))
            .unwrap();
    }
    stage.end_step(false).unwrap();
    stage.begin_step().unwrap();
    stage.push(CatalogRow::Channel(old.clone())).unwrap();
    stage.push(CatalogRow::Channel(old.clone())).unwrap();
    stage.end_step(true).unwrap();
    stage.finish().unwrap();
    assert_eq!(stage.counts().channels, 1);
    assert_eq!(stage.counts().duplicates, 1);
    assert_eq!(stage.load_channels().unwrap(), vec![old.clone()]);
    assert_eq!(
        store.replace_catalog_stage(source, stage).unwrap().channels,
        1
    );
    let rows = store
        .load_channels(source, &ChannelQuery::default())
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].channel, old);
    assert!(rows[0].favourite);
    assert_eq!(
        std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("catalog-stage"))
            .count(),
        0
    );
}

#[test]
fn startup_removes_only_abandoned_catalog_stage_files() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "catalog-stage-123-0.sqlite",
        "catalog-stage-123-0.sqlite-journal",
        "iptv.sqlite",
        "playlist.m3u",
    ] {
        std::fs::write(dir.path().join(name), b"synthetic").unwrap();
    }
    CatalogStage::remove_stale(dir.path());
    assert!(!dir.path().join("catalog-stage-123-0.sqlite").exists());
    assert!(
        !dir.path()
            .join("catalog-stage-123-0.sqlite-journal")
            .exists()
    );
    assert!(dir.path().join("iptv.sqlite").exists());
    assert!(dir.path().join("playlist.m3u").exists());
}
