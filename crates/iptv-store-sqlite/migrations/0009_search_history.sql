-- Recent searches, newest at position 0. Kept in the encrypted store instead
-- of the plaintext file older builds wrote beside it.
CREATE TABLE search_history (
    position INTEGER PRIMARY KEY NOT NULL CHECK(position >= 0),
    query TEXT NOT NULL
);
