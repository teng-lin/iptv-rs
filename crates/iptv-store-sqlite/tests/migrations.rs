//! Migrations from every historical schema, with data in it.
//!
//! `tests/fixtures/schema_vN.sql` is the exact DDL a build at version N
//! wrote (a `schema_dump`); `tests/fixtures/data_vN.sql` is synthetic data
//! in that shape. Each test builds an old file from those fixtures, opens it
//! with the current build and checks the data through the public API.
//!
//! Adding migration N+1: run
//! `cargo test -p iptv-store-sqlite --test migrations -- --ignored --nocapture print_current_schema`,
//! save the output as `schema_v{N+1}.sql`, write `data_vN.sql` for the now
//! historical version N, and add a case below.

mod common;

use iptv_store_sqlite::migrate::schema_dump;
use iptv_store_sqlite::{SCHEMA_VERSION, Store};

#[test]
#[ignore = "tool: prints the current schema snapshot"]
fn print_current_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.db");
    drop(Store::open(&path).unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    print!("{}", schema_dump(&conn).unwrap());
}

#[test]
fn latest_snapshot_is_the_fresh_schema() {
    let snapshot = std::fs::read_to_string(format!(
        "{}/tests/fixtures/schema_v{SCHEMA_VERSION}.sql",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("snapshot the latest schema (see module docs)");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.db");
    drop(Store::open(&path).unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(schema_dump(&conn).unwrap(), snapshot);
}
