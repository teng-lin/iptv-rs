-- Schema v2: user overlays. Keyed by (source, stable channel id) from
-- iptv-providers, with no foreign key to the provider tables: a refresh
-- replaces channels but never these rows, and a row whose channel is gone
-- stays (an orphan) until the channel returns or the user purges it.

-- One row per customised channel; deleted when every field is back to
-- its default.
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

CREATE TABLE favourites (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    added_at INTEGER NOT NULL,
    PRIMARY KEY (source_id, channel_id)
) WITHOUT ROWID;

CREATE INDEX favourites_by_position ON favourites (source_id, position);
