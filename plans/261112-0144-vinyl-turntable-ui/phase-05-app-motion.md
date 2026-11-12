---
phase: 5
title: "Phase 5: App-wide motion and verification"
status: in-progress
priority: P2
effort: "6h"
dependencies: [1]
---

# Phase 5: App-wide motion and verification

## Overview

Smooth the rest of the app with the Phase 1 tokens, then verify performance and
accessibility across the whole feature.

## Requirements

- [x] View changes: `document.startViewTransition` when available (WebView2 has it; feature-
      detect for WebKit) with a short fade + 12 px slide; the album card's cover morphs into
      the album page header via `view-transition-name` tied to the album id. Fallback: plain
      swap.
- [x] Play/pause button morphs between the two icons (two SVG paths with the same point count,
      `d` transitioned) and gives a spring press (scale 0.92 → 1).
- [x] Album card hover: lift 4 px + shadow grow with `--ease-spring`; play button keeps its
      existing reveal but on the spring easing.
- [x] Queue panel slides in/out from the right; toasts slide up and fade.
- [x] Transport bar: the elapsed fill of the seek slider follows the Phase 1 clock (no 10 Hz
      stepping).
- [x] Every transition respects reduced motion.
- [ ] Verification pass (whole plan): performance profile, keyboard-only run, Linux WebKitGTK
      check if available, `pnpm check`, `pnpm build`, `cargo test --workspace`,
      `cargo clippy --workspace --all-targets -- -D warnings`.

## Related Code Files

- Modify: `app/src/App.svelte`, `app/src/components/AlbumCard.svelte`,
  `app/src/components/AlbumView.svelte`, `app/src/components/TransportBar.svelte`,
  `app/src/components/QueuePanel.svelte`, `app/src/components/Icon.svelte`,
  `app/src/lib/app.svelte.ts` (wrap `go`/`back` in view transitions).

## Todo

- [x] View transitions + shared cover morph
- [x] Play/pause morph + press spring
- [x] Card hover spring
- [x] Queue panel + toast motion
- [x] Smooth seek fill
- [ ] Full verification pass

## Success Criteria

- Navigating albums → album → back shows the cover morph on Windows; nothing breaks where
  view transitions are missing.
- DevTools Performance during scrolling the album grid with hover: no long tasks > 50 ms.
- All checks listed above pass.
