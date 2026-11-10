-- Synthetic data added in the schema v2 shape (applied after data_v1.sql):
-- overlays, including ones for a channel that is not in the catalog.
INSERT INTO channel_overlays (source_id, channel_id, custom_name, custom_number, hidden,
    blocked, updated_at) VALUES
    (1, 'xt:live:101', 'My News', NULL, 0, 0, 1700000300),
    (1, 'xt:live:102', NULL, 10, 0, 1, 1700000300),
    (1, 'xt:live:103', NULL, NULL, 1, 0, 1700000300),
    (1, 'xt:live:999', 'Gone For Now', NULL, 0, 0, 1700000300);

INSERT INTO favourites (source_id, channel_id, position, added_at) VALUES
    (1, 'xt:live:102', 0, 1700000400),
    (1, 'xt:live:999', 1, 1700000400),
    (1, 'xt:live:101', 2, 1700000400);
