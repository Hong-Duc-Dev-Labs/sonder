---
title: Songs page
date: 2026-11-14
summary: Library → Songs lists every track by title, with a filter, Play all and Shuffle
---

# Songs page

## What happened
- The sidebar's Library group had Albums and Artists only; there was no way to browse every
  track outside search (capped at 200 results).

## Decision
- `Library::tracks()` returns every track ordered by title (NOCASE), then artist, album, id;
  exposed as the `library_tracks` command. One call, no paging: Play all and Shuffle need
  every id anyway, and a TrackItem is small.
- `SongsView.svelte` reuses `TrackList` (search variant: cover, artist • album, row menu) and
  renders 300 rows at a time, adding a batch when a sentinel 800 px below the last row comes
  into view, so a large library does not put every row in the DOM at once.
- The filter is client-side and diacritic-insensitive (`NFD`, marks stripped, đ → d); every
  word must match title, artist, album artist or album.

## Next steps
- If very large libraries (50k+ tracks) feel slow to open, page `library_tracks` and fetch
  ids separately for Play all.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
