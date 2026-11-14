-- Results of the lossless quality check, keyed by file path like playlist entries so they
-- survive rescans. A row describes the file at (mtime, size); once the file changes it no
-- longer matches the track row and reads as "not checked". report_json is the analyzer's
-- report without its charts (spectrum, cutoff track, spectrogram).
CREATE TABLE quality_checks (
    path             TEXT    PRIMARY KEY,
    mtime            INTEGER NOT NULL,
    size             INTEGER NOT NULL,
    analyzer_version INTEGER NOT NULL,
    verdict          TEXT    NOT NULL,
    confidence       TEXT    NOT NULL,
    report_json      TEXT    NOT NULL,
    checked_at       INTEGER NOT NULL
);
