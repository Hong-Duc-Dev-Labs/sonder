---
phase: 4
title: "Phase 4: Playlists"
status: todo
priority: P1
effort: "4-5d"
dependencies: []
---

# Phase 4: Playlists

## Overview

User playlists stored in the library database, with commands and a Playlists view.

## Key Insights

- Migrations are ordered SQL files in `crates/sonder-library/src/migrations/`, registered in
  `MIGRATIONS` (`crates/sonder-library/src/schema.rs:14`); an existing database is backed up
  before migrating.
- Tracks can disappear on rescan; playlist items must survive a track's removal and
  reappearance at the same path, so items reference the track **path**, joined to `tracks`
  on read. `tracks.path` is `UNIQUE` (`0001_initial.sql:41`) while `tracks.id` is a rowid
  that a delete-and-rescan may reassign.
- Commands are collected with tauri-specta in `app/src-tauri/src/main.rs`; `bindings.ts` is
  generated.
- Views are a `View` union in `app/src/lib/app.svelte.ts`; row actions live in `RowMenu.svelte`.

## Requirements

- [ ] Migration `0004_playlists.sql`: `playlists(id, name, created_at, updated_at)`,
      `playlist_items(playlist_id, position, path)` with `(playlist_id, position)` unique and
      `ON DELETE CASCADE`.
- [ ] Library API: create, rename, delete, list (with item count and total duration of
      present tracks), items (tracks present in the library, plus a count of missing
      entries), add tracks (append), remove item, move item (reorder).
- [ ] Commands for each operation; play / play next / enqueue a playlist reuse the existing
      player commands with the resolved track ids.
- [ ] UI: a Playlists entry in navigation, a list view, a playlist view (play, shuffle,
      rename, delete, drag to reorder, remove from playlist), "Add to playlist ▸" in
      `RowMenu` with "New playlist…".
- [ ] Deleting a playlist asks for confirmation.

## Related Code Files

- Create: `crates/sonder-library/src/migrations/0004_playlists.sql`,
  `crates/sonder-library/src/playlist.rs`, `app/src/components/PlaylistsView.svelte`,
  `app/src/components/PlaylistView.svelte`.
- Modify: `crates/sonder-library/src/{schema,lib}.rs`, `app/src-tauri/src/{commands,dto,main}.rs`,
  `app/src/lib/bindings.ts` (generated), `app/src/lib/app.svelte.ts`, `app/src/App.svelte`,
  `app/src/components/RowMenu.svelte`, `README.md`.

## Implementation Steps

1. Migration + schema test (fresh database and upgrade from version 3 with backup, as the
   existing schema tests do).
2. `playlist.rs` + tests: CRUD; positions stay dense 0..n after remove/move; a removed track
   disappears from `items` and counts as missing, then returns after rescan at the same path;
   deleting a playlist removes its items.
3. Commands + bindings.
4. Views, navigation, row menu; `pnpm check`.

## Todo

- [ ] Migration + schema test
- [ ] Library playlist API + tests
- [ ] Commands + bindings
- [ ] Views, navigation, row menu
- [ ] README

## Success Criteria

- `cargo test -p sonder-library` passes with the new tests; upgrade from version 3 keeps all
  existing data.
- Playlists can be created, filled from album/artist/search rows, reordered, played and
  deleted in the running app.

## Risk Assessment

- Path-based items break when a user moves files; that matches how the library itself keys
  tracks and is reported as "missing", not silently dropped.
