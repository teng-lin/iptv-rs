-- User policies are independent of imported rows and survive refreshes.
CREATE TABLE source_policy (
    source_id INTEGER PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    refresh_seconds INTEGER NOT NULL DEFAULT 0 CHECK(refresh_seconds >= 0),
    refresh_on_start INTEGER NOT NULL DEFAULT 1 CHECK(refresh_on_start IN (0,1))
);
CREATE TABLE group_overlays (
    source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    group_id TEXT NOT NULL,
    custom_name TEXT,
    hidden INTEGER NOT NULL DEFAULT 0 CHECK(hidden IN (0,1)),
    position INTEGER,
    PRIMARY KEY(source_id, group_id)
);
