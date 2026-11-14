---
title: Mini player
date: 2026-11-14
summary: Closing the main window while a track is loaded opens a small always-on-top player
---

# Mini player

## What happened
- Closing Sonder's window always quit, so music stopped with it. Wanted: a small picture-in-
  picture style player (polaroid cover, record peeking out, transport controls) shown on
  close, switchable in Settings.

## Decision
- A second Tauri window, label `mini`, built on demand from Rust (`src-tauri/src/mini.rs`):
  frameless, transparent, no shadow, always on top, 340×300, placed bottom-right of the main
  window's screen. It is hidden and reshown, never rebuilt.
- `on_window_event` intercepts the main window's close only when `settings.miniPlayer` is on
  (default) and the player has a current entry; the main window is hidden, not destroyed, so
  its state survives. Otherwise a leftover hidden mini window is destroyed so the app still
  exits normally. Closing the mini window calls `app.exit`.
- No `app.exit` on the main window's `Destroyed` event: it would race the updater's restart.
- Same bundle for both windows; `main.ts` mounts `MiniPlayer.svelte` when the window label is
  `mini`. It feeds the shared `app` store from the player and queue events, so `MiniDisc`'s
  spin and `playbackMs` work unchanged. Language is re-read on focus.
- Commands `window_show_main` and `app_quit`; the capability now covers `mini` and adds only
  `core:window:allow-start-dragging` (dragging is started from `pointerdown`, not
  `data-tauri-drag-region`, so the whole card drags except its buttons).

## Next steps
- Not verified by hand in the real windowed app yet: transparency and corner placement on
  macOS (transparent windows there need `macOSPrivateApi`) and Linux compositors.
