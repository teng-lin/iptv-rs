-- Synthetic data added in the schema v5 shape (applied after data_v4.sql):
-- source 1 has a guide address the user typed, source 2 had one removed,
-- and source 2 is the one the app shows.
INSERT INTO source_settings (source_id, epg_url, updated_at) VALUES
    (1, 'http://epg.example.com/typed.xml', 1700002000),
    (2, NULL, 1700002100);

INSERT INTO store_meta (key, value) VALUES ('active_source', '2');
