-- Listening history: one row per play that reached the listening threshold (see the app's
-- play tracker). Keyed by path like playlist entries, so history survives rescans; a play
-- whose file left the library still counts in totals but drops out of the top lists.
CREATE TABLE plays (
    id          INTEGER PRIMARY KEY,
    path        TEXT    NOT NULL,
    -- Unix seconds when the play started.
    played_at   INTEGER NOT NULL,
    -- Milliseconds actually heard (seeks and pauses excluded).
    listened_ms INTEGER NOT NULL
);
CREATE INDEX plays_played_at ON plays (played_at);
CREATE INDEX plays_path ON plays (path);
