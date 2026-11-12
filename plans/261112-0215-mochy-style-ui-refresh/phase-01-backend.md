---
phase: 1
title: "Phase 1: Backend — art colours, artist art and tracks, queue edits"
status: completed
priority: P1
effort: "1d"
dependencies: []
---

# Phase 1: Backend — art colours, artist art and tracks, queue edits

## State

Done. Colours are computed by `cover_colors()` when a cover is stored, persisted in `art.colors`
(migration 0002), backfilled at startup on a background thread, and carried to the UI as
`artColors`. Backfill reads the JPEG thumbnail, so its colours can differ from a fresh scan by a
few units per channel (tested within 8).

Evidence: library tests 16/16 (4 new), engine tests 26/26 (8 new, repeated 5× without a
failure), app tests 7/7, bindings regenerated, clippy and fmt clean.

## Requirements

- [x] Library: migration `0002` adds `art.colors` (text, nullable): up to four `#rrggbb`
      colours, most dominant first, comma-separated. The first is the seed for the glass wash;
      all four feed the ambient pools. Extend `seed_color` into `cover_colors` (top hue
      groups by population × chroma; greys give an empty list). Write it with the art row;
      `known_art()` reads it. `Library::backfill_art_colors()` fills older rows from the
      300 px thumbnail.
- [x] `AlbumSummary.art_colors`, `Track.art_colors`, `ArtistSummary.art` (newest album cover).
- [x] `Library::artist_tracks(id)`: album-artist albums in year order, then guest tracks.
- [x] Engine: `Command::Remove(index)` and `Command::Insert(index, paths)` with one
      index-remap function over every stored index; an edit inside audio already decoded ahead
      restarts from the current position; removing the playing track moves to the next.
- [x] App: queue entries get a stable `entryId`; commands `queue_remove(entryId)`,
      `player_play_next_tracks(ids)`, `player_play_next_album(id)`, `library_artist_tracks(id)`;
      DTOs carry `artColors`; the colour backfill runs on the scan worker at startup;
      `Settings.glass: full | reduced | off` (default `full`).

## Success Criteria

- Engine tests: remove before / at / after the current entry and insert-next while playing
  are byte-exact and report the right indices.
- Library tests: colours stored for a chromatic cover, none for a grey one, survive a rescan;
  artist tracks order; migration from schema 1 keeps data.
- Bindings regenerated; `cargo test --workspace` and clippy pass.
