//! Channel reads.

use std::fmt;

use iptv_providers::model::Channel;
use iptv_providers::{ContentId, GroupId};
use rusqlite::params_from_iter;
use rusqlite::types::Value;

use crate::codec::{catch_up_from_json, group_ids_from_json, narrow, stream_from_json};
use crate::error::Result;
use crate::search::{fts_query, like_pattern, words};
use crate::{SourceId, Store};

/// A channel as the UI shows it.
#[derive(Clone, PartialEq, Eq)]
pub struct ChannelRow {
    pub source_id: SourceId,
    /// Provider data as imported.
    pub channel: Channel,
}

/// Leaves out the stream locator: its URL and headers may carry
/// credentials.
impl fmt::Debug for ChannelRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelRow")
            .field("source_id", &self.source_id)
            .field("id", &self.channel.id)
            .field("name", &self.channel.name)
            .field("number", &self.channel.number)
            .field("groups", &self.channel.groups)
            .finish_non_exhaustive()
    }
}

/// Which channels to list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ChannelScope {
    #[default]
    All,
    /// One provider group.
    Group(GroupId),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelQuery {
    pub scope: ChannelScope,
    pub offset: u32,
    /// `None` returns every row from `offset`.
    pub limit: Option<u32>,
}

const ROW_COLUMNS: &str = "c.source_id, c.id, c.provider_id, c.name, c.number, c.logo, \
    c.group_ids, c.stream, c.epg_id, c.epg_name, c.epg_shift_minutes, c.catch_up, c.radio";

fn decode(row: &rusqlite::Row<'_>) -> Result<ChannelRow> {
    let catch_up: Option<String> = row.get(11)?;
    let channel = Channel {
        id: ContentId::from_stored(row.get::<_, String>(1)?),
        provider_id: row.get(2)?,
        name: row.get(3)?,
        number: narrow(row.get(4)?, "channels.number")?,
        logo: row.get(5)?,
        groups: group_ids_from_json(&row.get::<_, String>(6)?)?,
        stream: stream_from_json(&row.get::<_, String>(7)?)?,
        epg_id: row.get(8)?,
        epg_name: row.get(9)?,
        epg_shift_minutes: narrow(row.get(10)?, "channels.epg_shift_minutes")?,
        catch_up: catch_up.as_deref().map(catch_up_from_json).transpose()?,
        radio: row.get(12)?,
    };
    Ok(ChannelRow {
        source_id: SourceId(row.get(0)?),
        channel,
    })
}

/// FROM/WHERE for a query; parameters are appended to `args`.
fn scope_sql(source: SourceId, query: &ChannelQuery, args: &mut Vec<Value>) -> String {
    args.push(Value::Integer(source.0));
    match &query.scope {
        ChannelScope::All => "FROM channels c WHERE c.source_id = ?1".to_owned(),
        ChannelScope::Group(g) => {
            args.push(Value::Text(g.as_str().to_owned()));
            "FROM channel_groups m JOIN channels c ON c.source_id = m.source_id AND c.id = m.channel_id
             WHERE m.source_id = ?1 AND m.group_id = ?2"
                .to_owned()
        }
    }
}

impl Store {
    /// Channels of one source in provider order.
    pub fn load_channels(&self, source: SourceId, query: &ChannelQuery) -> Result<Vec<ChannelRow>> {
        let mut args = Vec::new();
        let from = scope_sql(source, query, &mut args);
        let sql = format!(
            "SELECT {ROW_COLUMNS} {from} ORDER BY c.position LIMIT {} OFFSET {}",
            query.limit.map_or(-1, i64::from),
            query.offset
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let mut rows = stmt.query(params_from_iter(args))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(decode(row)?);
        }
        Ok(out)
    }

    /// Rows `load_channels` would return without paging.
    pub fn channel_count(&self, source: SourceId, query: &ChannelQuery) -> Result<u64> {
        let mut args = Vec::new();
        let from = scope_sql(source, query, &mut args);
        let mut stmt = self
            .conn
            .prepare_cached(&format!("SELECT COUNT(*) {from}"))?;
        let n: i64 = stmt.query_row(params_from_iter(args), |r| r.get(0))?;
        Ok(n.max(0) as u64)
    }

    /// One channel by stable id.
    pub fn channel(&self, source: SourceId, id: &ContentId) -> Result<Option<ChannelRow>> {
        let sql =
            format!("SELECT {ROW_COLUMNS} FROM channels c WHERE c.source_id = ?1 AND c.id = ?2");
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let mut rows = stmt.query(rusqlite::params![source.0, id.as_str()])?;
        rows.next()?.map(decode).transpose()
    }

    /// Channels whose name contains every word of `text`, best match first.
    /// `source = None` searches every source.
    pub fn search_channels(
        &self,
        source: Option<SourceId>,
        text: &str,
        limit: u32,
    ) -> Result<Vec<ChannelRow>> {
        let words = words(text);
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let mut args = vec![Value::Integer(i64::from(limit))];
        let source_filter = match source {
            Some(s) => {
                args.push(Value::Integer(s.0));
                "AND c.source_id = ?2"
            }
            None => "",
        };
        let sql = if self.fts {
            args.push(Value::Text(fts_query(&words)));
            let at = args.len();
            format!(
                "SELECT {ROW_COLUMNS} FROM channel_fts f JOIN channels c ON c.rowid = f.rowid
                 WHERE channel_fts MATCH ?{at} {source_filter}
                 ORDER BY f.rank, c.source_id, c.position LIMIT ?1"
            )
        } else {
            let mut filters = String::new();
            for w in &words {
                args.push(Value::Text(like_pattern(w)));
                filters.push_str(&format!(" AND c.name LIKE ?{} ESCAPE '\\'", args.len()));
            }
            format!(
                "SELECT {ROW_COLUMNS} FROM channels c WHERE 1 = 1 {source_filter} {filters}
                 ORDER BY c.source_id, c.position LIMIT ?1"
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(args))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(decode(row)?);
        }
        Ok(out)
    }
}
