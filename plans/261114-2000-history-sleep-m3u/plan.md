---
title: "Listening history with stats, sleep timer, and M3U playlist import/export"
description: "Record what is actually listened to and show it as a Stats page; stop playback after a set time or at the end of the track; move playlists in and out as M3U/M3U8."
status: completed
priority: P2
effort: "~2 days"
tags: [library, app, ui, playlists, engine-bridge]
created: 2026-11-14
---

# Listening history, sleep timer, M3U

Source idea: Synq 1.5.1 (`listening_log`, "Wrapped", `m3u_playlist_parser`) and Music Check
(sleep timer), see `plans/reports/xia-261114-synq-feature-comparison.md`. Own design.

## Outcome

1. **History + Stats.** A track counts as played once it has actually played for 30 s or half
   its length, whichever is shorter (seeks do not count as listening). A **Stats** page shows,
   for the last 7 days, 30 days, 12 months or all time: listening time, plays and distinct
   tracks, a listening-time bar chart (per day or per month), top tracks, artists and albums,
   and the most recent plays. History is local only and can be cleared.
2. **Sleep timer.** From the transport bar: off, 15/30/45/60/90 min, or the end of the current
   track. It runs in the backend, so it fires with the main window closed (mini player). When
   it fires, playback pauses and a notice says so.
3. **M3U.** The Playlists page imports `.m3u`/`.m3u8` (relative and absolute paths, `file://`
   URLs, UTF-8 or Latin-1, `#EXTINF` ignored) as a new playlist; entries not in the library are
   kept as "not in library", web URLs are skipped and counted. A playlist's ⋯ menu exports
   `.m3u8` (UTF-8, `#EXTM3U`, `#EXTINF:<seconds>,<artist> - <title>`, absolute paths).

## Constraints

- Schema 6 adds `plays` only (path-keyed like playlists, so history survives rescans).
- Engine untouched: the play tracker and the sleep timer live in the Tauri event bridge,
  which already receives `TrackChanged`, `Position`, `State` and `EndOfQueue`.
- "End of track" pauses when the engine reports the next track started (gapless playback has
  no gap to stop in), so up to one position tick (~100 ms) of the next track can sound.
- Parsing/formatting M3U and the tracker are pure functions with unit tests.
- All new strings in `en.ts` and `vi.ts`; bindings regenerated.

## Non-goals

Favourites; scrobbling (Last.fm/ListenBrainz); sharing a "Wrapped" image; fading out before the
sleep timer pauses; M3U with web streams; PLS/XSPF.

## Acceptance criteria

1. Tracker: 30 s or half the track counts; seeks and pauses add nothing; a skip before the
   threshold records nothing; the last track is recorded on stop/end of queue.
2. Stats queries return the right totals, tops and buckets for a period (library tests).
3. Sleep timer fires at its deadline or at the next track change, pauses, and clears itself;
   it can be cancelled.
4. M3U round trip: export then import gives the same paths; Windows and POSIX paths,
   relative paths, BOM, Latin-1 and `file://` URLs parse.
5. `cargo test --workspace`, clippy, fmt, `pnpm check`, `pnpm test`, `pnpm build` pass.

## Result (2026-11-14)

All three parts are done; acceptance rows 1–5 are covered by `cargo test --workspace` (tracker
and sleep timer unit tests in `app/src-tauri/src/{history,sleep}.rs`, M3U parsing in
`crates/sonder-library/src/m3u.rs`, stats and playlist-by-path in
`crates/sonder-library/tests/history.rs`), clippy, fmt, `pnpm check`, `pnpm test` (54) and
`pnpm build`.

Built in a separate worktree (`coral-extras`), because the main worktree's branch had picked up
unpushed commits from another session.

Not done: a click-through in the running app, and listening on hardware to confirm how much of
the next track sounds before "end of this track" pauses.
