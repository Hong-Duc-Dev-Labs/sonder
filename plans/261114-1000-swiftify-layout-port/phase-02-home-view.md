---
phase: 2
title: "Phase 2: Home view"
status: in-progress
priority: P2
effort: "1d"
dependencies: [1]
---

# Phase 2: Home view

## Context

`View` in `lib/app.svelte.ts` has no home; the app opens on `{ kind: "albums" }`. Data for
Home already exists: `commands.libraryAlbums(sort)` with `Added` (most recently added first),
`libraryPlaylists`, `libraryArtists`, `app.current`, `app.player`, and the queue. There is no
play history, pinning or favourites — do not add backend for them.

## Requirements

- [x] `View` gains `{ kind: "home" }`; it becomes the initial view and the target of
      "back" when history is empty. `App.svelte` renders `HomeView`.
- [x] **Greeting**: "Good morning / afternoon / evening" or "Late night" from the local hour,
      as the page title (no glyph: the Icon set has no fitting time-of-day shapes).
      Re-evaluated when the view mounts (no timer).
- [x] **Hero card** (soft surface, not glass): cover, kicker "Now playing" when playing,
      "Paused" when paused, title, artist, album link, a play/pause button and a "Now Playing"
      button. When nothing is loaded: an empty state with "Browse albums". Cover colours may
      wash the card via the existing `app.wash`/art colours.
- [x] **Shortcuts**: two tiles side by side — Playlists (count) → playlists view,
      Artists (count) → artists view. Stack below 700 px of content width.
- [x] **Shelves**: a reusable `Shelf.svelte` = section header (title + "See all" pill button)
      and one horizontally scrolling row with arrow-key and wheel-to-horizontal support,
      edge fade, and keyboard-reachable cards. Shelves: Recently added (12 albums, reuse
      `AlbumCard`) → Albums sorted by Added; Playlists (12) → Playlists; Artists (12, reuse the
      artist card/avatar used by `ArtistsView`) → Artists. Hide a shelf when it is empty.
- [x] "See all" for Recently added opens Albums with sort = Added (reuse whatever sort state
      `AlbumsView` already has; if sort is local to that component, lift it into `app`).
- [x] Empty library (first run): Home shows one call to action to add a music folder in
      Settings instead of empty shelves.
- [x] Padding uses `--page-x` and `--dock-reserve` at the bottom (the sheet already sits beside
      the sidebar and inspector, so pages do not re-add either inset). Page heading has
      `data-view-heading` like other views.

## Files

- Create: `app/src/components/HomeView.svelte`, `app/src/components/Shelf.svelte`.
- Modify: `app/src/lib/app.svelte.ts`, `app/src/App.svelte`,
  `app/src/components/AlbumsView.svelte` (sort entry point, if needed).

## Steps

1. Add the view kind and default; update `isActive` and `back()` fallbacks.
2. Build `Shelf.svelte` (header + scroller) using existing tokens and `AlbumCard`.
3. Build `HomeView.svelte`: load the three lists in parallel with the existing `app.run`
   error handling; render greeting, hero, shortcuts, shelves.
4. Wire "See all" targets.

## Validation

- `pnpm check`, `pnpm test`, `pnpm build`.
- Unit test (vitest) for the greeting-by-hour function (boundaries 5, 12, 17, 21).
- `pnpm tauri dev`: cold start lands on Home; playing / paused / idle hero states; empty
  library CTA; shelves scroll by keyboard and wheel; "See all" targets; inspector docked
  narrows Home correctly.

## Risks

- Three list loads on every Home visit on big libraries → request only what the shelves show
  if the commands allow a limit; otherwise slice client-side and cache in `app` for the
  session, refreshed after a rescan.

## Rollback

Revert the phase commit; the default view returns to Albums.
