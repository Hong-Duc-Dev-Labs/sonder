# Quality check becomes its own sidebar page

**Date:** 2026-11-14

## What changed

The quality report moved out of a modal dialog into a **Quality check** page in the sidebar
(`app/src/components/QualityView.svelte`). It checks any song: a searchable library list, the
track that is playing, or any audio file picked from disk. The ⋯ menu and the track badges now
open this page on their track; `QualityReportDialog.svelte` was removed.

## Decisions

- **Files outside the library** go through two commands, `quality_pick_file` (backend dialog,
  remembers the path) and `quality_check_file` (checks the remembered path). The webview never
  names a path to read, which keeps the "no file access from the webview" constraint.
- A picked file that is already in the library (`Library::track_id_at`) is checked as that track,
  so its result is cached and badged; other files are checked without saving.
- Progress events for a picked file carry track id 0 (library ids start at 1) instead of
  changing the event's shape.
- Single checks share one cancel flag that a new check resets, so switching tracks mid-check
  could leave two analyses running. The page now cancels and waits for the running check before
  starting the next.
- The average spectrum uses a linear kHz axis with a filled area (512 log-spaced points are dense
  enough at the top), which shows a lossy lid or a hi-res roll-off more plainly than a log axis.

## Not verified

Not clicked through in the running Tauri app; checked with `pnpm check`, `pnpm test`,
`pnpm build`, clippy and the Rust tests.
