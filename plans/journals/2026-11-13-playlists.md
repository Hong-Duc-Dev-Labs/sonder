---
title: "Playlists: path-keyed entries, dense positions, one shared dialog"
date: 2026-11-13
summary: "Playlists in the library database and UI; entries survive rescans by keying on path, and the UI got its first modal dialog and drag reordering."
---

# Playlists

## What happened

Added schema version 4 (`playlists`, `playlist_items`) and a `playlist.rs` API in
`sonder-library`, eight Tauri commands, a Playlists page, a playlist page and "Add to
playlist ▸" in every track row menu. Library tests cover CRUD, ordering, duplicates and an
entry whose file leaves and comes back through a rescan; the schema test upgrades a version-3
database and checks the `.bak-3` copy.

## Decisions

- Entries store the track path, not `tracks.id`: a rescan may delete and re-add a row under a
  new id, while `tracks.path` is unique and stable. Missing files stay as entries, hidden from
  the list and counted as "not in library".
- Remove and move rewrite the playlist's rows in one transaction instead of shifting positions
  in place. Shifting under the `(playlist_id, position)` key can collide mid-statement in
  SQLite; a rewrite keeps positions dense with no ordering trick, and playlists are small.
- Remove and move take the entry's position among all entries (missing ones included), so the
  UI can edit around entries it does not show.
- Reordering uses a pointer-driven grip, not HTML5 drag and drop, which a Tauri window's file
  drop handling can swallow on Windows; Up/Down on the grip moves a row by keyboard.
- The UI moves rows and renumbers positions at once and queues the commands one after
  another, reloading only when the last has finished, so quick repeated moves never act on
  stale positions.
- One `Dialog.svelte` behind `app.ask()` serves "New playlist…", rename and the delete
  confirmation; the app had no modal before.

## Found along the way

- `Popover` closed when anything scrolled, including a list inside itself; it now ignores
  scrolls inside the panel and places itself again when its content changes size.
- `TrackList` keyed rows by track id, which breaks with a track listed twice; rows are now
  keyed by id plus occurrence.

## Open

- The in-app walkthrough (create, fill, reorder, play, delete) still has to be done by hand.
