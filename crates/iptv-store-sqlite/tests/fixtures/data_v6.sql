-- Synthetic data added in the schema v6 shape (applied after data_v5.sql):
-- source 1's guide sources as migrating its v5 data arranges them, its
-- feeds' results and the channels one feed lists; source 2 has a guide
-- source the user added, named, shifted and turned off, whose last
-- download failed.
INSERT INTO guide_sources (source_id, feed_id, provider_index, url, name, enabled, position,
    shift_minutes, updated_at) VALUES
    (1, 1, NULL, 'http://epg.example.com/typed.xml', NULL, 1, 0, 0, 1700002000),
    (1, 2, 0, 'http://epg.example.com/extra.xml.gz', NULL, 1, 2, -30, 1700001000),
    (2, 1001, NULL, 'http://alt.example.com/guide.xml', 'Fixture Alt', 0, 0, -60, 1700003000);

INSERT INTO epg_feed_results (source_id, feed_id, attempted_at, error_kind) VALUES
    (1, 1, 1700001000, NULL),
    (1, 2, 1700001000, NULL),
    (2, 1001, 1700001100, 'timeout');

INSERT INTO epg_feed_channels (source_id, feed_id, epg_channel, name) VALUES
    (1, 2, 'sport.example', 'Fixture Sport'),
    (1, 2, 'spare.example', 'Fixture Spare');
