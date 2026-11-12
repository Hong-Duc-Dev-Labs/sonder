---
title: "Glass UI refresh, queue editing, cover colours"
date: 2026-11-12
summary: "Sonder moved to a black canvas with frosted-glass chrome lit by cover colours; engine gained queue insert/remove; visual and perf checks still open"
---

# Glass UI refresh, queue editing, cover colours

## What happened
Implemented `plans/261112-0215-mochy-style-ui-refresh`:
- **Backend:** covers now carry up to four dominant colours (migration 0002, backfilled from
  thumbnails). Artists have a cover and an all-tracks query. The engine gained `Insert` and
  `Remove` queue commands. The app gained stable queue entry ids, Play next, queue remove and
  a glass quality setting.
- **UI:** rebuilt on a black canvas with drifting cover-colour light and frosted-glass chrome
  (sidebar, player pill, queue, popovers, toasts). The library got heroes melting into black,
  grouped track cards and row menus. Now Playing got its own large controls, and the record
  and tonearm were redrawn.
- **Checks:** all automated checks pass.

## Findings
- The five earlier UI reviews looked at Mochy's older Material screenshots. The user wanted
  its current dark-glass look, so the reviews' visual proposals were dropped and their
  accessibility and performance findings kept.
- Queue edits in a gapless engine need every stored index remapped through one function. An
  edit inside audio already decoded ahead restarts from the current position (seek-like). The
  engine must re-announce the current index after an edit, or a paused UI highlights the
  wrong row.
- Blur stays cheap only when it is static. Hero and mosaic backdrops are painted once into a
  96 px canvas with a canvas filter. The Now Playing cover blur (re-blurred every frame under
  the spinning label) was replaced by the stepped ambient canvas.
- Shell heredoc and `node -e` edits mangle backticks and `$`; source edits now go through the
  editor tools only.

## Next steps
- Run the app (`pnpm tauri dev`) and check every screen at 900×600 and 1280×820.
- Record a DevTools trace while scrolling under the glass at `full` and `reduced`.
- Check WebKitGTK blur cost on Linux.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
