# Phase 3 — OS File Open, Reveal in Folder & Keyboard Shortcuts

## Goal

Let users open audio files/folders straight from OS file associations, CLI args, or window drag-and-drop, reveal any track in the OS file manager, load lyrics for non-library files, and control playback smoothly via full desktop keyboard shortcuts.

## Changes

- `app/src-tauri/src/commands.rs`: add `player_open_paths`, `reveal_in_folder`, and `lyrics_get_path` (supporting embedded/`.lrc`/cached/online lyrics for external files with `track_id == 0`).
- `app/src-tauri/src/main.rs`: parse startup CLI file/folder args in `.setup`, populate initial queue & play when valid audio files are passed.
- `app/src-tauri/tauri.conf.json` & `capabilities/main-window.json`: declare `bundle.fileAssociations` for lossless/DSD extensions and permit window fullscreen toggle.
- `app/src/lib/platform.ts` & `platform.test.ts`: extend `GlobalShortcutAction` and `ShortcutContext` with `seekBack`/`seekForward` (±5s), `seekBackLong`/`seekForwardLong` (±30s with `Shift`), `prevTrack`/`nextTrack` (`Ctrl/⌘+Left/Right`), `volumeUp`/`volumeDown` (`Ctrl/⌘+Up/Down`, only when software volume is already active), `toggleLyrics` (`L`), `toggleQueue` (`Q`), `toggleNowPlaying` (`N`), and `toggleFullscreen` (`F11` / `Ctrl+⌘+F`).

## Verification

- `pnpm --dir app test` passes expanded `platform.test.ts` covering modifier precedence, editable/slider/tablist guards, and macOS `Option+ArrowLeft` preservation.
- Rust tests covering path expansion/sorting helper and `reveal_in_folder` path validation.
