---
phase: 3
title: "Phase 3: Library scanner"
status: todo
priority: P1
effort: "1w"
dependencies: [2]
---

# Phase 3: Library scanner

## Overview

Index local music folders into SQLite: tracks, albums, artists, cover art, ReplayGain
tags, technical format. Incremental rescans and a file watcher keep it current.

## Key Insights

- `lofty` reads tags for FLAC/ALAC(MP4)/WAV/AIFF/DSF; DFF (DSDIFF) uses ID3 chunks that
  may need a small custom reader — verify in this phase and fall back to filename parsing.
- Album identity: group by (album artist or artist, album title, disc-folder heuristics);
  prefer `MUSICBRAINZ_ALBUMID` when present. Online matching is a non-goal.
- Cover art: embedded picture first, then `cover|folder|front.(jpg|png)` in the folder;
  store resized thumbnails (e.g. 300 px, 1000 px) in an on-disk cache keyed by hash.

## Requirements

- [ ] `sonder-library` crate with `rusqlite` (bundled) + versioned migrations.
- [ ] Schema: `roots`, `tracks(path, mtime, size, rate, bits, channels, codec, duration, rg_track_gain, rg_album_gain, ...)`, `albums`, `artists`, `track_artists`, `art`.
- [ ] Scanner: parallel walk (`walkdir` + `rayon`), skip unchanged files by (mtime, size), remove deleted files.
- [ ] Watcher: `notify` debounced events trigger incremental rescan.
- [ ] Query API: list albums (sort by artist/title/added/year), album detail, artist detail, search by substring across title/album/artist (SQLite FTS5).

## Related Code Files

- Create: `crates/sonder-library/src/{lib.rs,schema.rs,migrations/*.sql,scan.rs,tags.rs,art.rs,query.rs,watch.rs}`.

## Implementation Steps

1. Schema + migrations + DB open with WAL mode. Back up the DB file before any migration runs (copy to `library.db.bak-<version>`).
2. Tag reader over lofty with a DFF fallback; unit tests on small fixtures.
3. Scanner with incremental logic; integration test on a temp dir fixture tree (add, modify, delete).
4. Art extraction + thumbnail cache (`image` crate).
5. Query API + FTS5 search; tests.
6. Watcher.

## Todo

- [ ] Schema + migrations + backup-before-migrate
- [ ] Tag reading incl. DSF/DFF
- [ ] Incremental scanner + tests
- [ ] Cover art + thumbnails
- [ ] Query + FTS search
- [ ] File watcher

## Success Criteria

- Integration test: scanning a fixture tree of mixed formats yields the expected albums/tracks; rescan after edits applies only the delta.
- Scanning a real library of ~10k tracks completes and a rescan with no changes finishes in seconds (measure and record).

## Risk Assessment

- Messy tags produce split albums: keep grouping logic in one function with table-driven tests so heuristics are easy to adjust.

## Security Considerations

- Paths come from user-chosen roots only; never follow symlinks outside roots without opt-in. DB stores no personal data beyond file paths.

## Next Steps

- Phase 4 consumes the query API.
