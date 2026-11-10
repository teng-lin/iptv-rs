-- Schema v1: sources, their credentials and import status, and the provider
-- catalog (groups, channels, VOD, series). Provider tables are replaced
-- wholesale on every refresh; they hold nothing the user edited.
--
-- Never edit a shipped migration: add a new numbered file instead.
-- Comments stay outside statements so sqlite_master holds plain DDL.

CREATE TABLE store_meta (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) WITHOUT ROWID;

-- config: kind-specific JSON with no secrets (server URL, User-Agent).
CREATE TABLE sources (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('m3u', 'xtream', 'stalker')),
    name TEXT NOT NULL,
    config TEXT NOT NULL,
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Secrets (passwords, MACs, playlist URLs) by field name. Plain text for
-- now; Android Keystore encryption of `value` is a later step.
CREATE TABLE source_credentials (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (source_id, name)
) WITHOUT ROWID;

CREATE TABLE source_imports (
    source_id INTEGER PRIMARY KEY NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    last_attempt_at INTEGER,
    last_success_at INTEGER,
    group_count INTEGER NOT NULL DEFAULT 0,
    channel_count INTEGER NOT NULL DEFAULT 0,
    vod_count INTEGER NOT NULL DEFAULT 0,
    series_count INTEGER NOT NULL DEFAULT 0,
    duplicate_count INTEGER NOT NULL DEFAULT 0,
    last_error_kind TEXT,
    last_error_at INTEGER
);

CREATE TABLE groups (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    id TEXT NOT NULL,
    kind TEXT NOT NULL,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    provider_id TEXT,
    adult INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (source_id, id)
) WITHOUT ROWID;

CREATE INDEX groups_by_position ON groups (source_id, kind, position);

-- A rowid table so the search index can address rows by rowid.
-- group_ids: JSON array (membership is also in channel_groups);
-- stream and catch_up: JSON, see codec.rs.
CREATE TABLE channels (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    id TEXT NOT NULL,
    position INTEGER NOT NULL,
    provider_id TEXT,
    name TEXT NOT NULL,
    number INTEGER,
    logo TEXT,
    group_ids TEXT NOT NULL,
    stream TEXT NOT NULL,
    epg_id TEXT,
    epg_name TEXT,
    epg_shift_minutes INTEGER,
    catch_up TEXT,
    radio INTEGER NOT NULL DEFAULT 0,
    UNIQUE (source_id, id)
);

CREATE INDEX channels_by_position ON channels (source_id, position);

-- position: the channel's provider position, copied so a group scan is
-- index-ordered.
CREATE TABLE channel_groups (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    group_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (source_id, group_id, channel_id)
) WITHOUT ROWID;

CREATE INDEX channel_groups_by_position ON channel_groups (source_id, group_id, position);

CREATE TABLE vod (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    id TEXT NOT NULL,
    position INTEGER NOT NULL,
    provider_id TEXT,
    name TEXT NOT NULL,
    group_ids TEXT NOT NULL,
    poster TEXT,
    plot TEXT,
    year INTEGER,
    rating_x10 INTEGER,
    added INTEGER,
    duration_secs INTEGER,
    stream TEXT NOT NULL,
    is_series INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (source_id, id)
) WITHOUT ROWID;

CREATE INDEX vod_by_position ON vod (source_id, position);

CREATE TABLE vod_groups (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    group_id TEXT NOT NULL,
    vod_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (source_id, group_id, vod_id)
) WITHOUT ROWID;

CREATE INDEX vod_groups_by_position ON vod_groups (source_id, group_id, position);

CREATE TABLE series (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    id TEXT NOT NULL,
    position INTEGER NOT NULL,
    provider_id TEXT,
    name TEXT NOT NULL,
    group_ids TEXT NOT NULL,
    poster TEXT,
    plot TEXT,
    year INTEGER,
    rating_x10 INTEGER,
    last_modified INTEGER,
    PRIMARY KEY (source_id, id)
) WITHOUT ROWID;

CREATE INDEX series_by_position ON series (source_id, position);

CREATE TABLE series_groups (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    group_id TEXT NOT NULL,
    series_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (source_id, group_id, series_id)
) WITHOUT ROWID;

CREATE INDEX series_groups_by_position ON series_groups (source_id, group_id, position);
