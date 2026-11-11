---
phase: 3
title: "Phase 3: Library scanner"
status: in-progress
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

- [x] `sonder-library` crate with `rusqlite` (bundled) + versioned migrations.
- [x] Schema: `roots`, `tracks(path, mtime, size, rate, bits, channels, codec, duration, rg_track_gain, rg_album_gain, ...)`, `albums`, `artists`, `track_artists`, `art`.
- [x] Scanner: parallel walk (`walkdir` + `rayon`), skip unchanged files by (mtime, size), remove deleted files.
- [x] Watcher: `notify` debounced events trigger incremental rescan.
- [x] Query API: list albums (sort by artist/title/added/year), album detail, artist detail, search by substring across title/album/artist (SQLite FTS5).

## Related Code Files

- Create: `crates/sonder-library/src/{lib.rs,schema.rs,migrations/*.sql,scan.rs,tags.rs,art.rs,query.rs,watch.rs,group.rs}`, `crates/sonder-library/tests/{library.rs,watch.rs,scale.rs,fixtures/mod.rs}`.
- Modify: `crates/sonder-cli/src/main.rs` (`scan` command for timing a real library).

## Implementation Steps

1. Schema + migrations + DB open with WAL mode. Back up the DB file before any migration runs (copy to `library.db.bak-<version>`).
2. Tag reader over lofty with a DFF fallback; unit tests on small fixtures.
3. Scanner with incremental logic; integration test on a temp dir fixture tree (add, modify, delete).
4. Art extraction + thumbnail cache (`image` crate).
5. Query API + FTS5 search; tests.
6. Watcher.

## Todo

- [x] Schema + migrations + backup-before-migrate
- [x] Tag reading incl. DSF/DFF
- [x] Incremental scanner + tests
- [x] Cover art + thumbnails
- [x] Query + FTS search
- [x] File watcher
- [ ] CI green on all three OSes (watcher test is the one most likely to differ on macOS/Linux)
- [ ] Manual: `sonder-cli scan <music folder>` on a real ~10k-track library; record full and no-change times below

## Success Criteria

- Integration test: scanning a fixture tree of mixed formats yields the expected albums/tracks; rescan after edits applies only the delta.
- Scanning a real library of ~10k tracks completes and a rescan with no changes finishes in seconds (measure and record).

## Risk Assessment

- Messy tags produce split albums: keep grouping logic in one function with table-driven tests so heuristics are easy to adjust.

## Security Considerations

- Paths come from user-chosen roots only; never follow symlinks outside roots without opt-in. DB stores no personal data beyond file paths.

## Implementation Notes

- lofty 0.25 has no DSD support at all (the Key Insight above was wrong for DSF too). DSF and
  DFF headers are parsed in `tags.rs`; their ID3v2 blob goes through lofty's MPEG reader with
  property reading off, the only public entry to its ID3v2 parser, so all formats share one
  tag mapping. A DFF without ID3 falls back to `DIIN`/`DITI`/`DIAR`; any file without a title
  or track number falls back to the file name (`03 - Title`, `1-03 Title`).
- MP4 is indexed only when the codec is ALAC; AAC files are reported as unsupported. ALAC has
  no generated fixture (needs an encoder) and relies on lofty's MP4 support.
- Album identity (`group.rs`): MusicBrainz release id → (album artist, title) → (title, album
  folder) when there is no album artist → folder when there is no album tag. Disc folders
  (`CD1`, `Disc 2`) collapse to their parent; disc suffixes (`(Disc 1)`, `[CD2]`) are stripped
  from titles. Album artist display: most common tagged album artist, else the single track
  artist, else "Various Artists".
- Roots may not overlap, so every track belongs to one root. A root that is missing (unplugged
  drive) or a folder that cannot be read keeps its tracks; only a successful walk removes.
- Search uses an FTS5 table with the trigram tokenizer (`remove_diacritics 1`): every term of
  three or more characters must match (in any column); shorter terms fall back to `LIKE`.
- Write transactions are `IMMEDIATE`, so the UI's connection and the watcher's wait for each
  other (10 s busy timeout) instead of failing on a stale read snapshot.
- Watcher: 2 s debounce, events coalesced on one worker thread that owns its own `Library`.
  Event paths are mapped back to the root's spelling (macOS reports `/private/var/...` for
  `/var/...`).
- Known gap: a changed folder image (`cover.jpg`) is picked up only when a track in that folder
  changes or is rescanned after an edit.
- Measured on Windows 11, release build, 10 000 generated FLAC tracks (1 000 albums, each with
  its own 600 px embedded cover), via `cargo test --release -p sonder-library --test scale --
  --ignored --nocapture`: full scan 3.86 s (includes 1 000 cover decodes and 2 000 thumbnails),
  no-change rescan 85 ms, album list 4 ms, search 2 ms. Generated files are header-only, so a
  real library's numbers (larger files, slower disks) are still to be recorded.

## Next Steps

- Phase 4 consumes the query API.
