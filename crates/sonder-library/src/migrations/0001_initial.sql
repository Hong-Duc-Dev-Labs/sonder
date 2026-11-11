-- Folders the user chose to index. Every track belongs to exactly one root.
CREATE TABLE roots (
    id              INTEGER PRIMARY KEY,
    path            TEXT    NOT NULL UNIQUE,
    follow_symlinks INTEGER NOT NULL DEFAULT 0,
    added_at        INTEGER NOT NULL
);

CREATE TABLE artists (
    id   INTEGER PRIMARY KEY,
    name TEXT    NOT NULL UNIQUE COLLATE NOCASE
);

-- One row per distinct cover image (SHA-256 of the original bytes). Thumbnails live in the
-- on-disk art cache under the same hash.
CREATE TABLE art (
    id     INTEGER PRIMARY KEY,
    hash   TEXT    NOT NULL UNIQUE,
    width  INTEGER NOT NULL,
    height INTEGER NOT NULL
);

CREATE TABLE albums (
    id          INTEGER PRIMARY KEY,
    -- Grouping identity computed from tags and folder layout; see group.rs.
    group_key   TEXT    NOT NULL UNIQUE,
    title       TEXT    NOT NULL,
    -- Display album artist ("A; B", "Various Artists"); artist_id is its first artist.
    artist      TEXT,
    artist_id   INTEGER REFERENCES artists (id) ON DELETE SET NULL,
    year        INTEGER,
    mb_album_id TEXT,
    art_id      INTEGER REFERENCES art (id) ON DELETE SET NULL,
    added_at    INTEGER NOT NULL
);
CREATE INDEX albums_artist ON albums (artist_id);

CREATE TABLE tracks (
    id            INTEGER PRIMARY KEY,
    root_id       INTEGER NOT NULL REFERENCES roots (id) ON DELETE CASCADE,
    path          TEXT    NOT NULL UNIQUE,
    -- Change detection: a file is re-read only when (mtime, size) differ.
    mtime         INTEGER NOT NULL,
    size          INTEGER NOT NULL,
    album_id      INTEGER NOT NULL REFERENCES albums (id),
    title         TEXT    NOT NULL,
    artist        TEXT,
    album_artist  TEXT,
    compilation   INTEGER NOT NULL DEFAULT 0,
    track_no      INTEGER,
    disc_no       INTEGER,
    year          INTEGER,
    genre         TEXT,
    codec         TEXT    NOT NULL,
    rate          INTEGER NOT NULL,
    bits          INTEGER,
    channels      INTEGER NOT NULL,
    duration_ms   INTEGER NOT NULL,
    rg_track_gain REAL,
    rg_track_peak REAL,
    rg_album_gain REAL,
    rg_album_peak REAL,
    art_id        INTEGER REFERENCES art (id) ON DELETE SET NULL,
    added_at      INTEGER NOT NULL
);
CREATE INDEX tracks_album ON tracks (album_id);
CREATE INDEX tracks_root ON tracks (root_id);

-- role 0 = track artist, 1 = album artist; position keeps tag order.
CREATE TABLE track_artists (
    track_id  INTEGER NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    artist_id INTEGER NOT NULL REFERENCES artists (id),
    role      INTEGER NOT NULL,
    position  INTEGER NOT NULL,
    PRIMARY KEY (track_id, role, position)
);
CREATE INDEX track_artists_artist ON track_artists (artist_id);

-- Substring search over tracks; rowid = tracks.id. The trigram tokenizer matches any
-- substring of three or more characters, ignoring case and diacritics.
CREATE VIRTUAL TABLE track_search USING fts5 (
    title, album, artist,
    tokenize = 'trigram remove_diacritics 1'
);

CREATE TRIGGER tracks_search_delete AFTER DELETE ON tracks BEGIN
    DELETE FROM track_search WHERE rowid = old.id;
END;
