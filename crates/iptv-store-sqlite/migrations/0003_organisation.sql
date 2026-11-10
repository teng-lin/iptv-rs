-- Schema v3: per-channel EPG override, manual order, custom groups and
-- watch history. Like v2 overlays, all keyed by stable ids with no foreign
-- key to the provider tables, so they survive refreshes.

-- epg_mode: NULL = automatic matching, 'none' = no guide for the channel,
-- 'xmltv' = the guide channel epg_channel of EPG source epg_source.
ALTER TABLE channel_overlays ADD COLUMN epg_mode TEXT CHECK (epg_mode IN ('none', 'xmltv'));
ALTER TABLE channel_overlays ADD COLUMN epg_source TEXT;
ALTER TABLE channel_overlays ADD COLUMN epg_channel TEXT;

-- scope: '' for the all-channels list, else a provider group id. Channels
-- without a row sort after the ordered ones, in provider order.
CREATE TABLE manual_order (
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    scope TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (source_id, scope, channel_id)
) WITHOUT ROWID;

CREATE INDEX manual_order_by_position ON manual_order (source_id, scope, position);

CREATE TABLE custom_groups (
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX custom_groups_by_position ON custom_groups (source_id, position);

CREATE TABLE custom_group_members (
    group_id INTEGER NOT NULL REFERENCES custom_groups (id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (group_id, channel_id)
) WITHOUT ROWID;

CREATE INDEX custom_group_members_by_position ON custom_group_members (group_id, position);

-- One row per play; kind is a content kind slug. Pruned per source.
CREATE TABLE watch_history (
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    watched_at INTEGER NOT NULL,
    position_secs INTEGER
);

CREATE INDEX watch_history_by_time ON watch_history (source_id, watched_at);

CREATE INDEX watch_history_by_item ON watch_history (source_id, item_id, watched_at);
