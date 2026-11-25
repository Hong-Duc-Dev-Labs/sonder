# Phase 2 — Session Persistence, Queue Mutations & Favorites

## Goal

Persist active queue/track/position/repeat across restarts, add engine-safe queue reordering/shuffling and Save as Playlist, and add path-keyed Favorites + Recent Albums queries in SQLite (schema v7).

## Changes

- `crates/sonder-engine/src/queue.rs` & `player.rs`: add `Queue::move_entry`, `Queue::reorder`, `Command::Select { index, position }`, `Command::Move { from, to }`, and `Command::Reorder(Vec<usize>)` with index remapping and lookahead stream restart when active queue entries change.
- `crates/sonder-library/src/migrations/0007_favorites.sql` & `schema.rs`: add `favorites` table (`path TEXT PRIMARY KEY, added_at INTEGER NOT NULL`), bumping `SCHEMA_VERSION` to 7.
- `crates/sonder-library/src/favorites.rs` & `query.rs` & `history.rs` & `lib.rs`: add `Track.favorite`, `track_by_path`, `toggle_favorite`, `set_favorite`, `favorite_tracks`, `favorite_paths`, and `recent_albums`.
- `app/src-tauri/src/{session.rs,settings.rs,dto.rs,state.rs,events.rs,commands.rs,main.rs}`:
  - Add `OutputShareMode` to `Settings`, `favorite` on `TrackItem`, `path` + `from_external` on `QueueItem`, `shared` on `SignalPathInfo`.
  - Add atomic `SessionStore` (`session.json`) restored on `AppState::open` and saved on mutations, track changes, pauses/stops, periodic ticks, and clean app exit.
  - Add commands `queue_move`, `queue_shuffle`, `playlist_create_from_queue`, `library_toggle_favorite`, `library_favorites`, `library_favorite_paths`, `library_recent_albums`, plus `FavoritesChangedEvent`.

## Verification

- `sonder-engine` tests for `move_entry`, `reorder`, and `select` resume at timestamp.
- `sonder-library` migration test (v6 → v7 backup + persistence across rescans), `toggle_favorite`, `track_by_path`, and `recent_albums`.
- `sonder-app` unit tests for `SessionStore` round-trip, `restore_index` with missing files, and queue shuffle permutation.
