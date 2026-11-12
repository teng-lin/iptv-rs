//! Which source the app shows (a `store_meta` entry).
//!
//! The `source_settings` table (schema v5) held one guide address per
//! source; schema v6 moved it into `guide_sources` (`guide_sources.rs`) and
//! nothing reads the column any more.

use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::sources::ensure_source;
use crate::{SourceId, Store};

/// The `store_meta` key of the source the app shows.
const ACTIVE_SOURCE: &str = "active_source";

impl Store {
    /// Persisted aggregation preference, independent of the last individual source.
    pub fn all_playlists(&self) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM store_meta WHERE key='all_playlists'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .as_deref()
            == Some("1"))
    }
    pub fn set_all_playlists(&mut self, all: bool) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO store_meta(key,value) VALUES('all_playlists',?1)",
            [if all { "1" } else { "0" }],
        )?;
        Ok(())
    }
    pub fn combined_last_channel(&self) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM store_meta WHERE key='combined_last_channel'",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub fn set_combined_last_channel(&mut self, key: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO store_meta(key,value) VALUES('combined_last_channel',?1)",
            [key],
        )?;
        Ok(())
    }
    /// The source the app shows, if one was chosen and still exists.
    pub fn active_source(&self) -> Result<Option<SourceId>> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM store_meta WHERE key = ?1",
                [ACTIVE_SOURCE],
                |r| r.get(0),
            )
            .optional()?;
        let Some(id) = value.and_then(|v| v.parse::<i64>().ok()).map(SourceId) else {
            return Ok(None);
        };
        Ok(ensure_source(&self.conn, id).ok().map(|()| id))
    }

    /// Remembers `id` as the source the app shows.
    pub fn set_active_source(&mut self, id: SourceId) -> Result<()> {
        ensure_source(&self.conn, id)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO store_meta (key, value) VALUES (?1, ?2)",
            params![ACTIVE_SOURCE, id.0.to_string()],
        )?;
        Ok(())
    }
}

/// Forgets `id` as the shown source (it is being deleted).
pub(crate) fn forget_active(conn: &rusqlite::Connection, id: SourceId) -> Result<()> {
    conn.execute(
        "DELETE FROM store_meta WHERE key = ?1 AND value = ?2",
        params![ACTIVE_SOURCE, id.0.to_string()],
    )?;
    Ok(())
}

#[cfg(test)]
mod combined_tests {
    use super::*;
    #[test]
    fn aggregation_preference_and_resume_key_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.sqlite");
        let mut store = Store::open(&path).unwrap();
        assert!(!store.all_playlists().unwrap());
        assert!(store.combined_last_channel().unwrap().is_none());
        store.set_all_playlists(true).unwrap();
        store
            .set_combined_last_channel("source:2:synthetic-channel")
            .unwrap();
        drop(store);
        let mut store = Store::open(&path).unwrap();
        assert!(store.all_playlists().unwrap());
        assert_eq!(
            store.combined_last_channel().unwrap().as_deref(),
            Some("source:2:synthetic-channel")
        );
        store.set_all_playlists(false).unwrap();
        assert!(!store.all_playlists().unwrap());
    }
}
