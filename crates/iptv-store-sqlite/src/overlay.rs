//! User overlays: favourites, hidden, blocked, custom name and number.
//!
//! Setters accept any channel id, present or not: overlays are keyed by
//! stable id and may be written before the channel is (re)imported, for
//! example when restoring a backup.

use std::collections::HashSet;

use iptv_providers::ContentId;
use rusqlite::types::Value;
use rusqlite::{Connection, params};

use crate::error::Result;
use crate::order::move_to;
use crate::sources::ensure_source;
use crate::{SourceId, Store, search};

/// A `channel_overlays` row with nothing customised left.
const EMPTY_OVERLAY: &str = "custom_name IS NULL AND custom_number IS NULL \
    AND hidden = 0 AND blocked = 0";

/// Customisations whose channel is not in the current catalog. They are
/// kept and apply again when the id returns.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct OrphanedOverlay {
    pub source_id: SourceId,
    pub channel_id: ContentId,
    pub favourite: bool,
    /// Name, number, hidden or blocked.
    pub customised: bool,
}

/// Overlay tables and what each row means for [`OrphanedOverlay`].
const OVERLAY_TABLES: &[(&str, &str)] = &[
    ("favourites", "1 AS favourite, 0 AS customised"),
    ("channel_overlays", "0, 1"),
];

/// Sets one `channel_overlays` column, then drops the row if it became
/// empty. `column` is a fixed identifier from this module.
fn set_overlay_column(
    conn: &Connection,
    source: SourceId,
    id: &ContentId,
    column: &str,
    value: Value,
    now: i64,
) -> Result<()> {
    ensure_source(conn, source)?;
    conn.execute(
        &format!(
            "INSERT INTO channel_overlays (source_id, channel_id, {column}, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (source_id, channel_id) DO UPDATE
             SET {column} = excluded.{column}, updated_at = excluded.updated_at"
        ),
        params![source.0, id.as_str(), value, now],
    )?;
    conn.execute(
        &format!(
            "DELETE FROM channel_overlays WHERE source_id = ?1 AND channel_id = ?2 AND {EMPTY_OVERLAY}"
        ),
        params![source.0, id.as_str()],
    )?;
    Ok(())
}

/// Ids of the source's current channels.
pub(crate) fn present_ids(conn: &Connection, source: SourceId) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare_cached("SELECT id FROM channels WHERE source_id = ?1")?;
    let ids = stmt
        .query_map([source.0], |r| r.get(0))?
        .collect::<rusqlite::Result<HashSet<String>>>()?;
    Ok(ids)
}

impl Store {
    fn set_column(
        &mut self,
        source: SourceId,
        id: &ContentId,
        column: &str,
        value: Value,
    ) -> Result<()> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        set_overlay_column(&tx, source, id, column, value, now)?;
        tx.commit()?;
        Ok(())
    }

    /// Adds to (at the end) or removes from favourites. Idempotent.
    pub fn set_favourite(&mut self, source: SourceId, id: &ContentId, on: bool) -> Result<()> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        ensure_source(&tx, source)?;
        if on {
            tx.execute(
                "INSERT OR IGNORE INTO favourites (source_id, channel_id, position, added_at)
                 VALUES (?1, ?2, (SELECT COALESCE(MAX(position) + 1, 0) FROM favourites
                                  WHERE source_id = ?1), ?3)",
                params![source.0, id.as_str(), now],
            )?;
        } else {
            tx.execute(
                "DELETE FROM favourites WHERE source_id = ?1 AND channel_id = ?2",
                params![source.0, id.as_str()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Flips the favourite flag; returns the new state.
    pub fn toggle_favourite(&mut self, source: SourceId, id: &ContentId) -> Result<bool> {
        let on = !self.is_favourite(source, id)?;
        self.set_favourite(source, id, on)?;
        Ok(on)
    }

    pub fn is_favourite(&self, source: SourceId, id: &ContentId) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM favourites WHERE source_id = ?1 AND channel_id = ?2)",
            params![source.0, id.as_str()],
            |r| r.get(0),
        )?)
    }

    /// Moves a favourite to `index` in the favourites list as shown (present
    /// channels, hidden ones included). Returns `false` if it is not a
    /// favourite.
    pub fn move_favourite(
        &mut self,
        source: SourceId,
        id: &ContentId,
        index: usize,
    ) -> Result<bool> {
        let tx = self.conn.transaction()?;
        let mut list: Vec<String> = {
            let mut stmt = tx.prepare_cached(
                "SELECT channel_id FROM favourites WHERE source_id = ?1 ORDER BY position",
            )?;
            stmt.query_map([source.0], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let present = present_ids(&tx, source)?;
        if !move_to(&mut list, &present, id.as_str(), index) {
            return Ok(false);
        }
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE favourites SET position = ?3 WHERE source_id = ?1 AND channel_id = ?2",
            )?;
            for (position, channel) in list.iter().enumerate() {
                stmt.execute(params![source.0, channel, position as i64])?;
            }
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn set_hidden(&mut self, source: SourceId, id: &ContentId, hidden: bool) -> Result<()> {
        self.set_column(source, id, "hidden", Value::from(hidden))
    }

    /// Parental block: blocked channels stay listed (flagged) by default.
    pub fn set_blocked(&mut self, source: SourceId, id: &ContentId, blocked: bool) -> Result<()> {
        self.set_column(source, id, "blocked", Value::from(blocked))
    }

    /// Sets a custom name, or restores the provider name with `None`.
    /// Blank names restore too.
    pub fn rename_channel(
        &mut self,
        source: SourceId,
        id: &ContentId,
        name: Option<&str>,
    ) -> Result<()> {
        let now = self.now();
        let fts = self.fts_index;
        let value = name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map_or(Value::Null, |n| Value::Text(n.to_owned()));
        let tx = self.conn.transaction()?;
        set_overlay_column(&tx, source, id, "custom_name", value, now)?;
        search::reindex_channel(&tx, fts, source, id)?;
        tx.commit()?;
        Ok(())
    }

    /// Sets a custom number, or restores the provider number with `None`.
    pub fn renumber_channel(
        &mut self,
        source: SourceId,
        id: &ContentId,
        number: Option<u32>,
    ) -> Result<()> {
        let value = number.map_or(Value::Null, |n| Value::Integer(i64::from(n)));
        self.set_column(source, id, "custom_number", value)
    }

    /// Overlay rows whose channel is not in the current catalog, for one
    /// source or all.
    pub fn orphaned_overlays(&self, source: Option<SourceId>) -> Result<Vec<OrphanedOverlay>> {
        let union = OVERLAY_TABLES
            .iter()
            .map(|(table, flags)| format!("SELECT source_id, channel_id, {flags} FROM {table}"))
            .collect::<Vec<_>>()
            .join(" UNION ALL ");
        let sql = format!(
            "SELECT x.source_id, x.channel_id, MAX(x.favourite), MAX(x.customised)
             FROM ({union}) x
             WHERE (?1 IS NULL OR x.source_id = ?1)
               AND NOT EXISTS (SELECT 1 FROM channels c
                               WHERE c.source_id = x.source_id AND c.id = x.channel_id)
             GROUP BY x.source_id, x.channel_id
             ORDER BY x.source_id, x.channel_id"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map([source.map(|s| s.0)], |r| {
            Ok(OrphanedOverlay {
                source_id: SourceId(r.get(0)?),
                channel_id: ContentId::from_stored(r.get::<_, String>(1)?),
                favourite: r.get(2)?,
                customised: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Deletes a source's orphaned overlays. Returns the rows removed.
    pub fn purge_orphaned_overlays(&mut self, source: SourceId) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut removed = 0;
        for (table, _) in OVERLAY_TABLES {
            removed += tx.execute(
                &format!(
                    "DELETE FROM {table} WHERE source_id = ?1 AND NOT EXISTS (
                         SELECT 1 FROM channels c WHERE c.source_id = ?1 AND c.id = {table}.channel_id)"
                ),
                [source.0],
            )?;
        }
        tx.commit()?;
        Ok(removed)
    }
}
