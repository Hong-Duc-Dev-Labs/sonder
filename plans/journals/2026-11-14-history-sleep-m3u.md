---
title: Listening stats, sleep timer and M3U playlists
date: 2026-11-14
summary: Plays are recorded from engine events into a Stats page; a backend sleep timer pauses playback; playlists import and export as M3U/M3U8
---

# Listening stats, sleep timer and M3U playlists

## What happened
- The last items from the Synq comparison report: listening history with stats, a sleep timer,
  and M3U import/export. Favourites were left out because they were not asked for this round.
- The main worktree's branch had two new commits from another session, so this work was done in
  its own worktree off `origin/main` to keep out of its way.

## Decision
- The play tracker lives in the Tauri event bridge and counts only small forward position steps
  while playing. Seeks (jumps over 1.5 s), pauses and skips therefore add nothing, and the
  engine stays untouched. The threshold is 30 s, or half the track when it is shorter.
- Plays are stored by path, like playlist entries (schema 6, table `plays`). Stats join the
  library at query time; a file that left the library still counts in the totals but drops out
  of the top lists. The chart's day and month bars are made in SQL with local time, empty ones
  included.
- The sleep timer is checked on every bridge tick and on every engine event, so it works with
  only the mini player open. "End of track" fires on the next `TrackChanged` or `EndOfQueue`:
  gapless playback leaves no gap to stop in.
- M3U import keeps entries that are not in the library (the playlist shows them as missing) and
  skips web streams. Paths are normalised to native separators so they match the library's.

## Open
- Not clicked through in the running app; how much of the next track sounds before "end of
  this track" pauses has not been measured on hardware.
