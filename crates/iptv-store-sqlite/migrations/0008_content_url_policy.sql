-- Old feed provenance was not recorded. Conservatively require public hosts
-- until the user-selected source configuration is used to rebuild its guides.
ALTER TABLE epg_feeds ADD COLUMN content_supplied INTEGER NOT NULL DEFAULT 1 CHECK(content_supplied IN (0, 1));
