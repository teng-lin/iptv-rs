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
