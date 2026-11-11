---
phase: 4
title: "Phase 4: Tauri UI"
status: todo
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
- UI framework (React vs Svelte) is an open question; plan assumes the user picks before this phase starts.

## Requirements

- [ ] `app/` Tauri 2 project; `src-tauri` depends on `sonder-engine`, `sonder-library`.
- [ ] Commands: library (albums, album, artists, search, add/remove root, rescan), player (play album/track, enqueue, transport, seek, volume), devices (list, select, formats), settings get/set.
- [ ] Events: player state ~10 Hz while playing, scan progress, device changes.
- [ ] Screens: Albums grid, Album detail, Artists, Search, Now Playing (large art, format badge), Queue panel, Settings.
- [ ] Settings persisted as TOML in the OS config dir (`directories` crate).
- [ ] Media keys / OS now-playing integration (`souvlaki`).

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

- [ ] Tauri scaffold in workspace
- [ ] Settings + state
- [ ] Commands + events + typed bindings
- [ ] Screens
- [ ] Art URI scheme
- [ ] Media keys
- [ ] Manual click-through

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
