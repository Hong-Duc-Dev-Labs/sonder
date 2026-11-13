-- User playlists. Items name tracks by path, not by tracks.id: a rescan may delete and
-- re-add a track under a new id, while its path (UNIQUE in tracks) stays the same. Items
-- whose file is not indexed are kept and reported as missing until it returns.
CREATE TABLE playlists (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Positions are dense, 0..n in play order.
CREATE TABLE playlist_items (
    playlist_id INTEGER NOT NULL REFERENCES playlists (id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    path        TEXT    NOT NULL,
    PRIMARY KEY (playlist_id, position)
) WITHOUT ROWID;
