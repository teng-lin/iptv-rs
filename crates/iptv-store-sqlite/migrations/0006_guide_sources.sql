-- Schema v6: several guide (XMLTV) sources per playlist source, each with
-- its own switch, place in the priority order and time shift; every
-- channel a stored feed lists (for choosing a channel's guide by hand);
-- and how each feed's last download went.
--
-- Never edit a shipped migration: add a new numbered file instead.
-- Comments stay outside statements so sqlite_master holds plain DDL.

-- The guide sources of a playlist source, as the user arranged them.
-- feed_id: the feed's iptv_epg::SourceId in the stored guide (epg_feeds).
-- provider_index: NULL for a source the user added; n for the provider's
-- n-th own guide (playlist header, Xtream xmltv.php), whose url is updated
-- by every playlist import. position: the user's order, first wins.
-- shift_minutes: added to every programme time of the feed. url may embed
-- credentials: the file is sensitive, never log rows.
CREATE TABLE guide_sources (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    feed_id INTEGER NOT NULL,
    provider_index INTEGER,
    url TEXT NOT NULL,
    name TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    position INTEGER NOT NULL,
    shift_minutes INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (source_id, feed_id),
    UNIQUE (source_id, provider_index)
) WITHOUT ROWID;

-- Every channel a stored feed lists (not only the bound ones in
-- epg_channels): id and first display name, replaced with the guide.
CREATE TABLE epg_feed_channels (
    source_id INTEGER NOT NULL,
    feed_id INTEGER NOT NULL,
    epg_channel TEXT NOT NULL,
    name TEXT,
    PRIMARY KEY (source_id, feed_id, epg_channel),
    FOREIGN KEY (source_id, feed_id) REFERENCES epg_feeds (source_id, feed_id) ON DELETE CASCADE
) WITHOUT ROWID;

-- The last download of each configured feed: when, and the error kind
-- when it failed (kind only, never a message: it could quote a URL).
CREATE TABLE epg_feed_results (
    source_id INTEGER NOT NULL REFERENCES epg_guides (source_id) ON DELETE CASCADE,
    feed_id INTEGER NOT NULL,
    attempted_at INTEGER NOT NULL,
    error_kind TEXT,
    PRIMARY KEY (source_id, feed_id)
) WITHOUT ROWID;

-- The guide address a v5 build kept per source becomes its first source.
INSERT INTO guide_sources (source_id, feed_id, provider_index, url, name, enabled, position,
    shift_minutes, updated_at)
SELECT source_id, 1, NULL, epg_url, NULL, 1, 0, 0, updated_at
FROM source_settings WHERE epg_url IS NOT NULL;

-- A source added before v5 kept a typed address only as feed 1 of its
-- stored guide.
INSERT INTO guide_sources (source_id, feed_id, provider_index, url, name, enabled, position,
    shift_minutes, updated_at)
SELECT f.source_id, 1, NULL, f.url, NULL, 1, 0, f.shift_minutes, f.fetched_at
FROM epg_feeds f
WHERE f.feed_id = 1
    AND NOT EXISTS (SELECT 1 FROM source_settings s WHERE s.source_id = f.source_id);

-- The provider's own guides that were stored (feed 2 + n is its n-th).
INSERT INTO guide_sources (source_id, feed_id, provider_index, url, name, enabled, position,
    shift_minutes, updated_at)
SELECT source_id, feed_id, feed_id - 2, url, NULL, 1, feed_id, shift_minutes, fetched_at
FROM epg_feeds WHERE feed_id >= 2;

-- The stored feeds loaded at their last download.
INSERT INTO epg_feed_results (source_id, feed_id, attempted_at, error_kind)
SELECT source_id, feed_id, fetched_at, NULL FROM epg_feeds;
