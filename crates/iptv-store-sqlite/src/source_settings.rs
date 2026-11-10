//! What the user chose about the sources: each source's own settings
//! (`source_settings`, schema v5) and which source the app shows (a
//! `store_meta` entry).

use iptv_providers::SecretUrl;
use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::sources::ensure_source;
use crate::{SourceId, Store};

/// The `store_meta` key of the source the app shows.
const ACTIVE_SOURCE: &str = "active_source";

/// A source's settings. `Debug` is safe: the guide address is a redacting
/// [`SecretUrl`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceSettings {
    /// The guide (XMLTV) address the user gave the source, loaded before
    /// its own guides; `None` when there is none (or it was removed).
    pub epg_url: Option<SecretUrl>,
}

impl Store {
    /// `id`'s settings; `None` when none were ever saved (a source added
    /// before schema v5, or by a caller that saved none).
    pub fn source_settings(&self, id: SourceId) -> Result<Option<SourceSettings>> {
        ensure_source(&self.conn, id)?;
        Ok(self
            .conn
            .query_row(
                "SELECT epg_url FROM source_settings WHERE source_id = ?1",
                [id.0],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .map(|url| SourceSettings {
                epg_url: url.map(SecretUrl::new),
            }))
    }

    /// Sets (or with `None` removes) the guide address the user gave `id`.
    pub fn set_source_epg_url(&mut self, id: SourceId, url: Option<&SecretUrl>) -> Result<()> {
        ensure_source(&self.conn, id)?;
        let now = self.now();
        self.conn.execute(
            "INSERT INTO source_settings (source_id, epg_url, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (source_id) DO UPDATE SET epg_url = excluded.epg_url,
                 updated_at = excluded.updated_at",
            params![id.0, url.map(SecretUrl::expose), now],
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
