-- table channel_groups
CREATE TABLE channel_groups (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    group_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (source_id, group_id, channel_id)
) WITHOUT ROWID;
-- table channel_overlays
CREATE TABLE channel_overlays (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL,
    custom_name TEXT,
    custom_number INTEGER,
    hidden INTEGER NOT NULL DEFAULT 0,
    blocked INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (source_id, channel_id)
) WITHOUT ROWID;
-- table channels
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
-- table favourites
CREATE TABLE favourites (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    added_at INTEGER NOT NULL,
    PRIMARY KEY (source_id, channel_id)
) WITHOUT ROWID;
-- table groups
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
-- table schema_migrations
CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    applied_at INTEGER NOT NULL
);
-- table series
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
-- table series_groups
CREATE TABLE series_groups (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    group_id TEXT NOT NULL,
    series_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (source_id, group_id, series_id)
) WITHOUT ROWID;
-- table source_credentials
CREATE TABLE source_credentials (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (source_id, name)
) WITHOUT ROWID;
-- table source_imports
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
-- table sources
CREATE TABLE sources (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('m3u', 'xtream', 'stalker')),
    name TEXT NOT NULL,
    config TEXT NOT NULL,
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
-- table store_meta
CREATE TABLE store_meta (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) WITHOUT ROWID;
-- table vod
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
-- table vod_groups
CREATE TABLE vod_groups (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    group_id TEXT NOT NULL,
    vod_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (source_id, group_id, vod_id)
) WITHOUT ROWID;
-- index channel_groups_by_position
CREATE INDEX channel_groups_by_position ON channel_groups (source_id, group_id, position);
-- index channels_by_position
CREATE INDEX channels_by_position ON channels (source_id, position);
-- index favourites_by_position
CREATE INDEX favourites_by_position ON favourites (source_id, position);
-- index groups_by_position
CREATE INDEX groups_by_position ON groups (source_id, kind, position);
-- index series_by_position
CREATE INDEX series_by_position ON series (source_id, position);
-- index series_groups_by_position
CREATE INDEX series_groups_by_position ON series_groups (source_id, group_id, position);
-- index vod_by_position
CREATE INDEX vod_by_position ON vod (source_id, position);
-- index vod_groups_by_position
CREATE INDEX vod_groups_by_position ON vod_groups (source_id, group_id, position);
