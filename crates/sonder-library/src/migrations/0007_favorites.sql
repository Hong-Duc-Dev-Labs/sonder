-- Path-keyed favorites: entries stay intact across library rescans that reassign track ids.
CREATE TABLE favorites (
    path     TEXT    PRIMARY KEY,
    added_at INTEGER NOT NULL
);
CREATE INDEX favorites_added_at ON favorites (added_at DESC);
