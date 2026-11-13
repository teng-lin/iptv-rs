//! Complete SQLite snapshots, including committed pages still in the WAL.

use std::fs::OpenOptions;
use std::path::{Component, Path};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, backup::Backup, params};

use crate::{Result, SCHEMA_VERSION, Store, StoreError, migrate};

fn invalid(message: &str) -> StoreError {
    StoreError::InvalidData(message.to_owned())
}

impl Store {
    /// Writes a complete, consistent snapshot to a new private file. Existing
    /// files are never replaced. The destination has no dependency on the live
    /// database's WAL or on any still-open connection.
    pub fn snapshot_to(&self, path: &Path) -> Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(path)
            .map_err(|_| invalid("cannot create snapshot file"))?;
        drop(file);
        let result = (|| {
            let mut destination = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            destination.busy_timeout(Duration::from_secs(5))?;
            Backup::new(&self.conn, &mut destination)?.run_to_completion(
                256,
                Duration::from_millis(5),
                None,
            )?;
            // A standalone file, even when the source uses WAL.
            destination.pragma_update(None, "journal_mode", "DELETE")?;
            drop(destination);
            Self::validate_snapshot(path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(path);
        }
        result
    }

    /// Checks an untrusted snapshot without migrating or modifying it. Rejects
    /// unknown schema objects as well as damaged, foreign-key-invalid and newer
    /// files. Migrations run only after the caller accepts the validated file.
    pub fn validate_snapshot(path: &Path) -> Result<()> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.pragma_update(None, "trusted_schema", false)?;
        let version = migrate::current_version(&conn)?;
        if version > SCHEMA_VERSION {
            return Err(StoreError::SchemaTooNew {
                found: version,
                supported: SCHEMA_VERSION,
            });
        }
        if version == 0 {
            return Err(invalid("snapshot is not an application database"));
        }
        let user_version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if user_version != version {
            return Err(invalid("snapshot schema version mismatch"));
        }
        let mut expected = Connection::open_in_memory()?;
        migrate::migrate_to(&mut expected, version, 0)?;
        if migrate::schema_dump(&conn)? != migrate::schema_dump(&expected)? {
            return Err(invalid("snapshot has an unexpected database schema"));
        }
        // schema_dump deliberately omits the derived FTS tables. Compare those
        // independently, so an object merely prefixed channel_fts cannot evade
        // validation. A snapshot from a host without FTS is also valid.
        let derived = |db: &Connection| -> Result<Vec<(String, String, String)>> {
            let mut statement = db.prepare(
                "SELECT type, name, sql FROM sqlite_master
                 WHERE sql IS NOT NULL AND name LIKE 'channel_fts%' ORDER BY name",
            )?;
            Ok(statement
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?)
        };
        let actual_fts = derived(&conn)?;
        if !actual_fts.is_empty() {
            crate::search::prepare(&mut expected)?;
            if actual_fts != derived(&expected)? {
                return Err(invalid("snapshot has an unexpected search schema"));
            }
        }
        for migration in migrate::MIGRATIONS.iter().take(version as usize) {
            let name: String = conn.query_row(
                "SELECT name FROM schema_migrations WHERE version = ?1",
                [migration.version],
                |r| r.get(0),
            )?;
            if name != migration.name {
                return Err(invalid("snapshot migration history mismatch"));
            }
        }
        let mut check = conn.prepare("PRAGMA integrity_check")?;
        let results = check
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if results.as_slice() != ["ok"] {
            return Err(invalid("snapshot integrity check failed"));
        }
        let mut check = conn.prepare("PRAGMA foreign_key_check")?;
        if check.query([])?.next()?.is_some() {
            return Err(invalid("snapshot foreign key check failed"));
        }
        Ok(())
    }

    /// Rewrites imported local playlist locations when moving an entire data
    /// directory to another device/home. All local playlists must be under the
    /// archived root; unrelated filesystem paths cannot silently become valid.
    /// The caller copies those files separately before activating the snapshot.
    pub fn rebase_local_playlists(&mut self, old_root: &Path, new_root: &Path) -> Result<()> {
        let mut explicit_guides = std::collections::HashMap::new();
        for summary in self.sources()? {
            if let Some(source) = self.source(summary.id)?
                && let iptv_providers::model::SourceConfig::Xtream(credentials) =
                    source.source.config
                && let Ok(client) =
                    iptv_providers::xtream::XtreamClient::from_credentials(&credentials)
            {
                explicit_guides.insert(summary.id.0, client.xmltv_url());
            }
        }
        let tx = self.conn.transaction()?;
        let locations = {
            let mut statement = tx.prepare(
                "SELECT c.source_id, c.value FROM source_credentials c
                 JOIN sources s ON s.id = c.source_id
                 WHERE s.kind = 'm3u' AND c.name = 'location'",
            )?;
            statement
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (source, location) in locations {
            let Some(local) = iptv_providers::content_url::local_path(&location)
                .map_err(|_| invalid("invalid local playlist URL"))?
            else {
                continue;
            };
            let relative = local
                .as_path()
                .strip_prefix(old_root)
                .map_err(|_| invalid("local playlist is outside the backup directory"))?;
            if relative.as_os_str().is_empty()
                || relative
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(invalid("invalid local playlist location"));
            }
            let rebased = new_root.join(relative);
            let rebased = rebased
                .to_str()
                .ok_or_else(|| invalid("local playlist location is not UTF-8"))?;
            tx.execute(
                "UPDATE source_credentials SET value = ?1
                 WHERE source_id = ?2 AND name = 'location'",
                params![rebased, source],
            )?;
        }
        // Guide URLs are just as capable of naming local resources as a
        // playlist location. Every local reference must stay inside the archive.
        for table in ["guide_sources", "epg_feeds"] {
            let guides = {
                let provenance = if table == "guide_sources" {
                    "provider_index IS NOT NULL"
                } else {
                    "content_supplied"
                };
                let mut statement = tx.prepare(&format!(
                    "SELECT source_id, feed_id, url, {provenance} FROM {table}"
                ))?;
                statement
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, u32>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, bool>(3)?,
                        ))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            for (source, feed, location, content_supplied) in guides {
                let local = iptv_providers::content_url::local_path(&location)
                    .map_err(|_| invalid("invalid guide URL"))?;
                if let Some(local) = local {
                    let relative = local
                        .strip_prefix(old_root)
                        .map_err(|_| invalid("local guide is outside backup directory"))?;
                    if relative.as_os_str().is_empty()
                        || relative
                            .components()
                            .any(|p| !matches!(p, Component::Normal(_)))
                    {
                        return Err(invalid("invalid local guide location"));
                    }
                    let rebased = new_root.join(relative);
                    tx.execute(
                        &format!("UPDATE {table} SET url=?1 WHERE source_id=?2 AND feed_id=?3"),
                        params![
                            rebased
                                .to_str()
                                .ok_or_else(|| invalid("guide path is not UTF-8"))?,
                            source,
                            feed
                        ],
                    )?;
                } else {
                    let explicit = !content_supplied
                        || explicit_guides
                            .get(&source)
                            .is_some_and(|url| url == &location);
                    let validate = if explicit {
                        iptv_providers::content_url::validate_picked
                    } else {
                        iptv_providers::content_url::validate
                    };
                    validate(&location)
                        .map_err(|_| invalid("backup guide URL violates its source policy"))?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }
}
