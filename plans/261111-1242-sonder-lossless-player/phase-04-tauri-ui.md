---
phase: 4
title: "Phase 4: Tauri UI"
status: completed
priority: P1
effort: "2w"
dependencies: [2, 3]
---

# Phase 4: Tauri UI

## Overview

A Tauri 2 desktop app that hosts the engine and library in-process and exposes them to a
TypeScript web UI: library grid, album page, now playing, queue, settings (roots, output
device, DSP).

## Key Insights

- Keep engine and library as plain Rust crates; `app/src-tauri` only adapts them to Tauri
  commands and events. This keeps a future core/remote split possible without moving code.
- Cover art served through a custom URI scheme (`sonder-art://<hash>/<size>`) from the
  thumbnail cache, not base64 over IPC.
- Large libraries: virtualised grid; paginate album queries.
- UI framework: Svelte 5 + Vite (chosen by the user on 2026-11-12).

## Requirements

- [x] `app/` Tauri 2 project; `src-tauri` depends on `sonder-engine`, `sonder-library`.
- [x] Commands: library (albums, album, artists, search, add/remove root, rescan), player (play album/track, enqueue, transport, seek, volume), devices (list, select, formats), settings get/set.
- [x] Events: player state ~10 Hz while playing, scan progress, device changes.
- [x] Screens: Albums grid, Album detail, Artists, Search, Now Playing (large art, format badge), Queue panel, Settings.
- [x] Settings persisted as TOML in the OS config dir (`directories` crate).
- [x] Media keys / OS now-playing integration (`souvlaki`).

## Related Code Files

- Create: `app/package.json`, `app/src/**`, `app/src-tauri/{Cargo.toml,tauri.conf.json,src/main.rs,src/commands.rs,src/events.rs,src/settings.rs}`.
- Modify: workspace `Cargo.toml` (add `app/src-tauri` member).

## Implementation Steps

1. Scaffold Tauri 2 app with the chosen frontend framework; add to workspace.
2. Implement settings store and app state (engine + library handles).
3. Implement commands/events; typed bindings via `specta`/`tauri-specta` to avoid drift.
4. Build screens; virtualised album grid; art URI scheme.
5. Media keys integration.
6. Run the app and click through: add root → scan → play album → seek → next → switch device.

## Todo

- [x] Tauri scaffold in workspace
- [x] Settings + state
- [x] Commands + events + typed bindings
- [x] Screens
- [x] Art URI scheme
- [x] Media keys
- [ ] Manual click-through on a real library and DAC (add root → scan → play album → seek → next → switch device). The app was run and used in development, but this full script, the device switch in particular, has not been recorded as passing. Carried forward: repeat it before the first release.
- [x] CI green with the UI build steps (Windows, macOS, Ubuntu on PR #4)

Closed with PR #4, which also brought the vinyl turntable, the glass refresh and synced
lyrics; see [vinyl turntable](../261112-0144-vinyl-turntable-ui/plan.md),
[glass refresh](../261112-0215-mochy-style-ui-refresh/plan.md) and
[synced lyrics](../261112-1100-synced-lyrics/plan.md).

## Success Criteria

- `pnpm tauri build` produces an installer on Windows.
- Click-through above works on a real library with no UI freeze during scan.
- Now Playing shows the source format and the actual device format.

## Risk Assessment

- IPC event flood: throttle position updates; diff state.

## Security Considerations

- Tauri capabilities limited to required commands; no shell/fs plugin exposure to the webview; CSP enabled.

## Next Steps

- Phase 8 adds the signal-path panel and DSP settings UI.

## Implementation Notes

- Engine: new `Command::PlayIndex` (play a queue entry from any state) and `Command::SetDevice`
  (move playback to another device). A device switch stops the old stream and reads the
  position only after its render thread exits, so no frame is lost or repeated. A test checks
  the bytes are exact across the switch.
- Library: new `albums_page` / `album_count`. The grid fetches 200 albums per page and renders
  only the rows in view. New `scan_with_progress` reports walking, then reading (done/total),
  then writing.
- Scans run on a worker thread with their own SQLite connection, so browsing does not wait on a
  scan. A scan requested during another scan runs once that one finishes.
- The app mirrors the engine queue (paths → track metadata). Every enqueue and clear goes
  through one lock, so the mirror and the engine stay in the same order.
- Player events are folded into one snapshot. State, track and format changes reach the UI
  immediately. Position-only changes go at most every 100 ms, and only when the snapshot changed.
- Bindings: `tauri-specta =2.0.0-rc.28` writes `app/src/lib/bindings.ts` on debug runs. The test
  `typescript_bindings_are_up_to_date` fails when the committed file is out of date, and
  `SONDER_UPDATE_BINDINGS=1 cargo test -p sonder-app typescript` rewrites it.
- Security: the window capability grants only `core:event:default`. The folder picker runs in
  Rust (`tauri-plugin-dialog` with no webview permission). The CSP allows images only from `self`,
  `data:` and the art scheme. The art handler accepts only hex hashes and the known thumbnail
  sizes, so a request cannot read files outside the cache.
- CI builds the UI (`pnpm check`, `pnpm build`) before running cargo, because the app crate
  embeds `app/dist`. It also installs WebKitGTK on Linux.
- Settings live in `settings.toml` in the OS config dir: output device, volume in dB or
  bit-perfect, album sort. A file that fails to parse is renamed to `settings.toml.broken` and
  the defaults are used.
