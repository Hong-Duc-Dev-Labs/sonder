---
title: "Vinyl turntable Now Playing built"
date: 2026-11-12
summary: "Spinning vinyl, tonearm seek, track-change slides and app motion implemented in Svelte; automated checks green, visual pass pending"
---

# Vinyl turntable Now Playing built

## What happened
Implemented `plans/261112-0144-vinyl-turntable-ui` phases 1–5 in `app/src`: motion tokens and
a shared frame loop, an extrapolated playback clock, a vinyl record whose label alone rotates
(spring spin-up/coast-down), a tonearm that tracks progress and seeks by drag or keyboard,
track-change slides over a blurred cover backdrop, View Transitions with a cover morph,
play/pause morph and spring hovers. `coverStyle` (vinyl/square) is a persisted setting.
`pnpm check`, `pnpm test` (6 geometry tests), `pnpm build`, `cargo test --workspace` and clippy
pass.

## Findings
- 10 Hz position reports step visibly on a turning arm; a pull-based clock that extrapolates
  between reports (capped 250 ms, monotonic within a track) fixes it without extra IPC.
- After a drag-seek, the arm swung back to the old time until the seek report arrived; it now
  holds at the drop point until the clock catches up.
- A View Transition cover morph needs the target element on the first frame; album pages now
  draw their header from the clicked card's summary.
- Shell-quoted Node edit scripts broke twice on `$state` and backticks; Edit is safer for
  Svelte/Markdown.

## Next steps
- Visual and performance pass in the running app (WebView2 DevTools), keyboard-only run, and
  a WebKitGTK check for the blurred backdrop.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
