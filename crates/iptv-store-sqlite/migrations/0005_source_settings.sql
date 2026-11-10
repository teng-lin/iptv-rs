-- Schema v5: settings the user chose for a playlist source, apart from the
-- provider's data and from the source's secrets.
--
-- Never edit a shipped migration: add a new numbered file instead.
-- Comments stay outside statements so sqlite_master holds plain DDL.

-- epg_url: the guide (XMLTV) address the user gave the source, loaded before
-- the source's own guides; NULL when the user gave none or removed it. A
-- source without a row was added by a build before v5, which kept a typed
-- address only with the source's stored guide (epg_feeds). The address may
-- embed credentials: the file is sensitive, never log rows.
CREATE TABLE source_settings (
    source_id INTEGER PRIMARY KEY NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    epg_url TEXT,
    updated_at INTEGER NOT NULL
);
