-- Synthetic data added in the schema v4 shape (applied after data_v3.sql):
-- a stored guide for source 1 with two feeds, two playlist channels sharing
-- one EPG channel, and a failed guide attempt for source 2.
INSERT INTO epg_guides (source_id, replaced_at, attempted_at, last_error_kind,
    match_fingerprint, bound_channels, ends_at) VALUES
    (1, 1700001000, 1700001000, NULL, 4242, 3, 1700014400),
    (2, NULL, 1700001100, 'timeout', NULL, 0, NULL);

INSERT INTO epg_feeds (source_id, feed_id, url, priority, user_agent, assumed_offset_minutes,
    shift_minutes, store_descriptions, past_days_to_keep, fetched_at, etag, last_modified,
    bytes, channel_count, programme_count) VALUES
    (1, 1, 'http://panel.example.com:8080/xmltv.php?username=fixture-user&password=fixture-pass',
        2, NULL, 0, 0, 1, 2, 1700001000, '"fixture-etag"', NULL, 4096, 1, 3),
    (1, 2, 'http://epg.example.com/extra.xml.gz', 1, 'Fixture/1', 60, -30, 0, NULL,
        1700001000, NULL, 'Tue, 14 Nov 2023 22:13:20 GMT', 1024, 1, 1);

INSERT INTO epg_channels (id, source_id, feed_id, epg_channel) VALUES
    (1, 1, 1, 'news.example'),
    (2, 1, 2, 'sport.example');

INSERT INTO epg_bindings (source_id, channel_key, epg_channel_id, match_kind) VALUES
    (1, 'xt:live:101', 1, 'tvg_id'),
    (1, 'xt:live:103', 1, 'override'),
    (1, 'xt:live:102', 2, 'name');

INSERT INTO epg_programmes (channel_id, start, stop, title, sub_title, category, icon, rating,
    catchup_id, season, episode, flags) VALUES
    (1, 1700000000, 1700003600, 'Morning Fixture', NULL, 'News', NULL, NULL, NULL, NULL, NULL, 0),
    (1, 1700003600, 1700007200, 'Fixture Midday', 'Part two', 'News', NULL, '12', 'cu-2', 1, 4, 0),
    (1, 1700007200, 1700014400, 'Fixture Evening', NULL, NULL, NULL, NULL, NULL, NULL, NULL, 0),
    (2, 1700000000, 1700010800, 'Fixture Match', NULL, 'Sport', 'http://img.example.com/m.png',
        NULL, NULL, NULL, NULL, 0);

INSERT INTO epg_descriptions (channel_id, start, description) VALUES
    (1, 1700000000, 'An invented morning programme.'),
    (1, 1700003600, 'An invented midday programme.');
