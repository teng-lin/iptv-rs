-- Schema v4: stored guide data (EPG) per playlist source, so the guide
-- shows at start without downloading and parsing XMLTV again. A refresh
-- replaces a source's whole guide in one transaction; nothing here is user
-- data (EPG overrides stay in channel_overlays).
--
-- Never edit a shipped migration: add a new numbered file instead.
-- Comments stay outside statements so sqlite_master holds plain DDL.

-- One row per playlist source with a stored guide or a failed attempt.
-- match_fingerprint: opaque hash of the channels and overrides the stored
-- bindings were matched from (iptv-import). ends_at: latest programme end.
-- last_error_kind: kind only (never a message: it could quote a URL).
CREATE TABLE epg_guides (
    source_id INTEGER PRIMARY KEY NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    replaced_at INTEGER,
    attempted_at INTEGER,
    last_error_kind TEXT,
    match_fingerprint INTEGER,
    bound_channels INTEGER NOT NULL DEFAULT 0,
    ends_at INTEGER
);

-- The XMLTV feeds a stored guide came from, with the settings they were
-- fetched with (so a background refresh needs no playlist download).
-- feed_id: iptv_epg::SourceId within the playlist source. url may embed
-- credentials (Xtream xmltv.php): the file is sensitive, never log rows.
-- etag / last_modified: response validators, when the network layer
-- exposes them. Counts are those written at fetch time.
CREATE TABLE epg_feeds (
    source_id INTEGER NOT NULL REFERENCES epg_guides (source_id) ON DELETE CASCADE,
    feed_id INTEGER NOT NULL,
    url TEXT NOT NULL,
    priority INTEGER NOT NULL,
    user_agent TEXT,
    assumed_offset_minutes INTEGER NOT NULL DEFAULT 0,
    shift_minutes INTEGER NOT NULL DEFAULT 0,
    store_descriptions INTEGER NOT NULL DEFAULT 1,
    past_days_to_keep INTEGER,
    fetched_at INTEGER NOT NULL,
    etag TEXT,
    last_modified TEXT,
    bytes INTEGER NOT NULL DEFAULT 0,
    channel_count INTEGER NOT NULL DEFAULT 0,
    programme_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (source_id, feed_id)
) WITHOUT ROWID;

-- EPG channels whose programmes are stored: those a playlist channel is
-- bound to. Several playlist channels (HD/SD variants) can share one.
CREATE TABLE epg_channels (
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL,
    feed_id INTEGER NOT NULL,
    epg_channel TEXT NOT NULL,
    UNIQUE (source_id, feed_id, epg_channel),
    FOREIGN KEY (source_id, feed_id) REFERENCES epg_feeds (source_id, feed_id) ON DELETE CASCADE
);

-- Playlist channel (stable key: the channel's ContentId) to EPG channel.
-- match_kind: override, tvg_id or name.
CREATE TABLE epg_bindings (
    source_id INTEGER NOT NULL REFERENCES epg_guides (source_id) ON DELETE CASCADE,
    channel_key TEXT NOT NULL,
    epg_channel_id INTEGER NOT NULL REFERENCES epg_channels (id) ON DELETE CASCADE,
    match_kind TEXT NOT NULL,
    PRIMARY KEY (source_id, channel_key)
) WITHOUT ROWID;

CREATE INDEX epg_bindings_by_channel ON epg_bindings (epg_channel_id);

-- Compact rows for the grid: times are unix seconds, start < stop,
-- non-overlapping per channel. flags: reserved bit set (0 for now).
CREATE TABLE epg_programmes (
    channel_id INTEGER NOT NULL REFERENCES epg_channels (id) ON DELETE CASCADE,
    start INTEGER NOT NULL,
    stop INTEGER NOT NULL,
    title TEXT NOT NULL,
    sub_title TEXT,
    category TEXT,
    icon TEXT,
    rating TEXT,
    catchup_id TEXT,
    season INTEGER,
    episode INTEGER,
    flags INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (channel_id, start)
) WITHOUT ROWID;

CREATE INDEX epg_programmes_by_stop ON epg_programmes (stop);

-- Descriptions apart, read on demand (the in-memory guide leaves them out).
CREATE TABLE epg_descriptions (
    channel_id INTEGER NOT NULL REFERENCES epg_channels (id) ON DELETE CASCADE,
    start INTEGER NOT NULL,
    description TEXT NOT NULL,
    PRIMARY KEY (channel_id, start)
) WITHOUT ROWID;
