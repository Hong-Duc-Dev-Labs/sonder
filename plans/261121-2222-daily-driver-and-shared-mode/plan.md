---
title: "Daily Driver Polish & Opt-in Shared Mode Output (Wave 1)"
description: "Ship M2 Shared Mode output (WASAPI/CoreAudio/ALSA) and M1 Daily Driver Polish (session/queue persistence, queue reorder/shuffle/save, OS file open & drag-drop, reveal in folder, favorites schema v7, Home recent plays, full desktop shortcuts)."
status: completed
priority: P1
effort: "~4 days"
tags: [output, engine, library, app, ui, shortcuts]
created: 2026-11-21
---

# Daily Driver Polish & Opt-in Shared Mode Output (Wave 1)

Derived from [`plans/reports/brainstorm-261121-2222-v020-roadmap.md`](../reports/brainstorm-261121-2222-v020-roadmap.md).

## Outcome

1. **M2 — Shared Mode Output (opt-in):** Users can toggle between `Exclusive` (default, bit-perfect) and `Shared` (OS audio mixer so Discord/Zoom/browsers play concurrently) in `Settings → Output`. Shared mode safely converts DSD → PCM, resamples to the OS mixer rate if needed, adds an `"OS Mixer"` stage to Signal Path, and surfaces amber non-bit-perfect status across UI badges.
2. **M1 — Daily Driver Polish:**
   - Queue & playback position survive restarts (`session.json`).
   - Queue supports drag/keyboard reordering (`queue_move`), upcoming shuffle (`queue_shuffle`), and Save as Playlist (`playlist_create_from_queue`).
   - Opening audio files/folders from the OS, CLI arguments, or window drag-and-drop plays immediately (even outside indexed library folders), and tracks can be revealed in Explorer/Finder/file manager (`reveal_in_folder`).
   - Path-keyed Favorites (`0007_favorites.sql`, schema v7) with a dedicated Favorites view, row/transport/Now Playing hearts, plus a Recently Played albums shelf and Favorites shortcut on Home.
   - Complete desktop keyboard shortcuts (seek, prev/next, software volume step when active, Lyrics/Queue/Now Playing toggles, fullscreen).

## Constraints

- Zero new Cargo or npm dependencies.
- `Exclusive` mode + DSP off stays 100% byte-exact bit-perfect (`sonder-verify` untouched/passing).
- All animations check `reducedMotion()`; all strings translated in both `en.ts` and `vi.ts`.
- SQLite migration runs atomically with automatic `library.db.bak-6` backup.

## Non-goals

- CUE sheets, WavPack, lossy codecs (Wave 2 / M3).
- Discord Rich Presence, Last.fm / ListenBrainz scrobbling (Wave 3 / M4).

## Phases

1. [Phase 1 — Shared Mode Output](phase-01-shared-mode-output.md)
2. [Phase 2 — Session Persistence, Queue Mutations & Favorites](phase-02-session-queue-and-favorites.md)
3. [Phase 3 — OS File Open, Reveal in Folder & Keyboard Shortcuts](phase-03-os-file-open-reveal-and-shortcuts.md)
4. [Phase 4 — Frontend UI, Bilingual i18n & Full Verification](phase-04-ui-and-verification.md)

## Result

Implemented Wave 1 (M2 Opt-in Shared Mode + M1 Daily Driver Polish) across `sonder-output`, `sonder-engine`, `sonder-verify`, `sonder-library`, `sonder-app`, and the Svelte 5 desktop UI (`app/src`), while preserving Exclusive default bit-perfect output.

## Acceptance Criteria

- `cargo test --workspace` passes (including `sonder-verify` bit-perfect checks + shared mode, queue reorder, favorites, and session tests).
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check` pass.
- `pnpm --dir app check`, `pnpm --dir app test`, and `pnpm --dir app build` exit 0.
- `bash scripts/check-forbidden-files.sh` passes.
