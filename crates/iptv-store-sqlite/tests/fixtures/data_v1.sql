-- Synthetic data in the schema v1 shape (provider data only).
INSERT INTO sources (id, kind, name, config, position, created_at, updated_at) VALUES
    (1, 'xtream', 'Fixture Panel', '{"server_url":"http://panel.example.com:8080","user_agent":null}', 0, 1700000000, 1700000000),
    (2, 'm3u', 'Fixture List', '{"user_agent":"Fixture/1","referrer":null}', 1, 1700000001, 1700000001);

INSERT INTO source_credentials (source_id, name, value) VALUES
    (1, 'username', 'fixture-user'),
    (1, 'password', 'fixture-pass'),
    (2, 'location', 'http://lists.example.com/fixture.m3u');

INSERT INTO source_imports (source_id, last_attempt_at, last_success_at, group_count,
    channel_count, vod_count, series_count, duplicate_count, last_error_kind, last_error_at) VALUES
    (1, 1700000200, 1700000100, 3, 3, 1, 1, 0, 'timeout', 1700000200);

INSERT INTO groups (source_id, id, kind, position, name, provider_id, adult) VALUES
    (1, 'xt:live-group:1', 'live', 0, 'News', '1', 0),
    (1, 'xt:live-group:2', 'live', 1, 'Sports', '2', 0),
    (1, 'xt:movie-group:9', 'movie', 2, 'Films', '9', 0);

-- catch_up uses the encoding of builds before `correction_secs` existed.
INSERT INTO channels (source_id, id, position, provider_id, name, number, logo, group_ids,
    stream, epg_id, epg_name, epg_shift_minutes, catch_up, radio) VALUES
    (1, 'xt:live:101', 0, '101', 'Fixture News', 1, NULL, '["xt:live-group:1"]',
        '{"t":"xtream","stream_id":101,"ext":null}', 'news.example', NULL, NULL,
        '{"kind":"xtream","days":2,"source":null}', 0),
    (1, 'xt:live:102', 1, '102', 'Fixture Sport', 2, 'http://img.example.com/102.png',
        '["xt:live-group:2"]', '{"t":"xtream","stream_id":102,"ext":null}', NULL, NULL, NULL,
        NULL, 0),
    (1, 'xt:live:103', 2, '103', 'Fixture Radio', NULL, NULL, '["xt:live-group:1"]',
        '{"t":"xtream","stream_id":103,"ext":null}', NULL, NULL, 60, NULL, 1);

INSERT INTO channel_groups (source_id, group_id, channel_id, position) VALUES
    (1, 'xt:live-group:1', 'xt:live:101', 0),
    (1, 'xt:live-group:2', 'xt:live:102', 1),
    (1, 'xt:live-group:1', 'xt:live:103', 2);

INSERT INTO vod (source_id, id, position, provider_id, name, group_ids, poster, plot, year,
    rating_x10, added, duration_secs, stream, is_series) VALUES
    (1, 'xt:movie:5', 0, '5', 'Fixture Film', '["xt:movie-group:9"]', NULL, 'Synthetic.', 2019,
        65, 1690000000, 6000, '{"t":"xtream","stream_id":5,"ext":"mp4"}', 0);

INSERT INTO vod_groups (source_id, group_id, vod_id, position) VALUES
    (1, 'xt:movie-group:9', 'xt:movie:5', 0);

INSERT INTO series (source_id, id, position, provider_id, name, group_ids, poster, plot, year,
    rating_x10, last_modified) VALUES
    (1, 'xt:series:8', 0, '8', 'Fixture Show', '[]', NULL, NULL, 2018, NULL, 1690000500);
