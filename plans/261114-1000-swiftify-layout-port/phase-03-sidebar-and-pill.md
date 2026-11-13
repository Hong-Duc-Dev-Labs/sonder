---
phase: 3
title: "Phase 3: Sidebar and player pill"
status: in-progress
priority: P2
effort: "0.5d"
dependencies: [1, 2]
---

# Phase 3: Sidebar and player pill

## Context

Sidebar (`App.svelte`): brand, "Library" (Albums, Artists, Playlists), Search (`Ctrl F`),
Now Playing (live mini disc), scan status, Settings; 72 px icon rail below 1100 px.
Pill (`TransportBar.svelte`): disc + text + status chip, prev/play/next/repeat, seek, and a
right cluster with only the queue toggle. Volume: `settings.volumeDb` (`null` = bit-perfect),
`commands.playerSetVolume(db | null)`.

## Requirements

### Sidebar

- [x] Order: Home, Search; "Library" label → Albums, Artists; "Playlists" label with a `+`
      button → up to 8 playlists inline (small cover or playlist glyph + name, active state
      when that playlist is open) → "All playlists"; spacer; Now Playing, scan status,
      Settings.
- [x] `+` uses the existing create-playlist flow in `app` (the one around `playlistCreate`)
      and opens the new playlist.
- [x] Playlist list refreshes after create / rename / delete (reuse whatever `app` already
      refreshes for `PlaylistsView`; lift the list into `app` if it is component-local).
- [x] The middle section scrolls on short windows; brand, Now Playing and Settings stay put.
- [x] Icon rail (< 1100 px): group labels and inline playlists hidden, `+` hidden, "All
      playlists" shows as the playlist icon — same targets as before.

### Pill

- [x] Right cluster: Lyrics toggle (`aria-pressed`, uses `toggleLyrics` from phase 1), Queue
      toggle (`aria-expanded`, `toggleQueue`), then volume.
- [x] Volume: when `volumeDb !== null`, a compact horizontal slider (dB range matching the
      Settings volume control, same step) with `aria-valuetext` in dB; when `null`, no
      slider — the existing bit-perfect status chip remains the signal. The pill never turns
      software volume on by itself.
- [x] Narrow pill (< 1100 px): volume collapses to an icon that opens the slider in the
      existing `Popover`.

## Files

- Modify: `app/src/App.svelte`, `app/src/components/TransportBar.svelte`,
  `app/src/lib/app.svelte.ts` (playlist list state, if lifted).

## Validation

- `pnpm check`, `pnpm test`, `pnpm build`.
- `pnpm tauri dev`: create a playlist from the sidebar and see it appear and open; 10+
  playlists show 8 + "All playlists"; rail at 1000 px; pill toggles open the docked inspector
  (1500 px) and overlay (1200 px); volume slider appears only after enabling software volume
  in Settings, and moving it changes `volumeDb`; with volume off the chip still says
  bit-perfect.

## Risks

- Pill right cluster crowding at mid widths → hide the slider into the popover earlier
  (tune the breakpoint in the visual check).

## Rollback

Revert the phase commit; sidebar and pill return to their current layout.
