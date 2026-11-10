-- Synthetic data added in the schema v3 shape (applied after data_v2.sql):
-- EPG overrides, a manual order, a custom group and watch history.
UPDATE channel_overlays SET epg_mode = 'xmltv', epg_source = 'epg-fixture',
    epg_channel = 'alt.news.example' WHERE source_id = 1 AND channel_id = 'xt:live:101';

INSERT INTO channel_overlays (source_id, channel_id, hidden, blocked, updated_at, epg_mode)
    VALUES (1, 'xt:live:104', 0, 0, 1700000500, 'none');

INSERT INTO manual_order (source_id, scope, channel_id, position) VALUES
    (1, '', 'xt:live:102', 0),
    (1, '', 'xt:live:999', 1),
    (1, '', 'xt:live:101', 2);

INSERT INTO custom_groups (id, source_id, name, position, created_at) VALUES
    (1, 1, 'Evening', 0, 1700000600);

INSERT INTO custom_group_members (group_id, channel_id, position) VALUES
    (1, 'xt:live:103', 0),
    (1, 'xt:live:999', 1),
    (1, 'xt:live:101', 2);

INSERT INTO watch_history (source_id, item_id, kind, watched_at, position_secs) VALUES
    (1, 'xt:live:101', 'live', 1700000700, NULL),
    (1, 'xt:movie:5', 'movie', 1700000800, 1200),
    (1, 'xt:live:101', 'live', 1700000900, NULL);
