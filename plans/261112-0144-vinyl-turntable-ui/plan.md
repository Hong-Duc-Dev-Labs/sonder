---
title: "Vinyl turntable Now Playing and smooth UI motion"
description: "A spinning vinyl record with a tonearm seek control in Now Playing, plus app-wide motion, ported at the idea level from Mochy-Music."
status: in-progress
priority: P1
effort: "~4 days"
tags: [frontend, ui, animation]
created: 2026-11-12
---

# Vinyl turntable Now Playing and smooth UI motion

## Overview

Now Playing shows the album cover as the label of a vinyl record that spins slowly while
playing (eased up and down), under a tonearm that tracks progress and can be dragged to
seek. Track changes slide records in and out over a blurred cover background; the rest of
the app gets consistent, reduced-motion-aware transitions.

## Source manifest

- Source: `realidkroo/Mochy-Music` @ `01b06ba4842e10c3d048d2ca6709b3b65d679786` (Kotlin,
  Jetpack Compose, Android). Files studied: `ui/player/customize/{DiscCover,DiscCoverStack,
  TurntableArm,TurntableArt,PlayerCoverStyles}.kt`.
- **License: GPL-3.0.** Sonder has no license yet. Port **ideas and behaviour only**: write
  all code, SVG and gradients fresh for Svelte. Do not translate or paste source code or
  colour/geometry tables from it.

## Source anatomy (behaviour, re-expressed)

| Piece | What it does |
|---|---|
| Still surface | Grooves, sheen and rim drawn once under a fixed light; never rotates |
| Rotating label | Only the round cover (+ ring text) rotates, at a constant ~26 °/s |
| Paused look | Pictures desaturate while paused |
| Tonearm | Angle = song progress (law of cosines to stylus radius); drag to scrub; time arc while held; snaps small steps, springs on jumps |
| Arm shadow | Silhouette drawn once, only rotated, offset grows when lifted |

## Dependency matrix

| Need | Local | Status |
|---|---|---|
| Cover images | `sonder-art` scheme, 300/1000 px | EXISTS |
| Progress | `PlayerSnapshot.elapsedMs` at ~10 Hz | EXISTS (needs per-frame extrapolation → Phase 1) |
| Seek | `commands.playerSeek(ms)` | EXISTS |
| Cover style setting | `Settings` | NEW field `coverStyle` |
| Frame loop / springs | none | NEW (`clock`, `motion`, `spin`) |
| Unit tests for TS | none | NEW (`vitest`) |

## Decision matrix

| Decision | Source's way | Ours |
|---|---|---|
| Rendering | Compose Canvas, offscreen layer | SVG + CSS gradients, one composited rotating layer |
| Spin speed | Constant 26 °/s, abrupt stop | ~26 °/s with spring spin-up (~0.8 s) / coast-down (~1.6 s) — user choice |
| Tonearm | Seek control | Seek control + keyboard slider; bottom slider kept — user choice |
| Placement | Many cover styles | Vinyl default in Now Playing, Vinyl/Square setting, mini disc in transport bar — user choice |
| Extra motion | Per-screen | Track-change slide, blurred backdrop, view transitions, micro-interactions — user choice |
| Reduced motion | Toggle in prefs | Follow OS `prefers-reduced-motion` |

## Phases

| # | Phase | Status |
|---|-------|--------|
| 1 | [Motion foundation and cover style](./phase-01-motion-foundation.md) | Completed |
| 2 | [Vinyl disc](./phase-02-vinyl-disc.md) | Completed |
| 3 | [Tonearm as seek control](./phase-03-tonearm-seek.md) | Completed |
| 4 | [Now Playing scene, transitions, backdrop](./phase-04-now-playing-scene.md) | Completed |
| 5 | [App-wide motion and verification](./phase-05-app-motion.md) | In progress (manual verification open) |

Order: 1 → 2 → 3 → 4; 5 needs only 1 and can run after 1 in parallel with 2–4.

## Risk score: Medium

- Licence contamination (high impact, controlled): clean-room rule above; review diffs for
  copied constants or code before merging.
- Per-frame cost on weak GPUs / WebKitGTK (medium): only transforms animate per frame; blur
  a small image; measure in Phase 2 and Phase 5.
- Interaction conflicts on the stage (low): only the arm is interactive.

## Success Criteria

- [ ] Playing an album in Now Playing: record spins up smoothly, arm tracks progress, pause
      coasts to a stop and greys the label, Next slides a new record in.
- [ ] Dragging the arm seeks; keyboard seeking on the arm works.
- [ ] Settings switches Vinyl/Square and it persists across restarts.
- [ ] Reduced motion removes continuous motion everywhere.
- [x] `pnpm check`, `pnpm test`, `pnpm build`, `cargo test --workspace`, clippy clean.

## Implementation Notes

- Vinyl surface is layered CSS gradients on one element (not an SVG): same still-surface
  idea, fewer nodes. Only the label layer rotates.
- The playback clock is a pull function (`playbackMs()`) read inside the shared frame loop
  (`onFrame` in `motion.ts`) instead of a subscribe API; the loop already sleeps with no
  subscribers or a hidden window.
- Stylus range is 0.93 → label + 0.07 of the record radius (Sonder's own values).
- After a drag-seek the arm holds where it was dropped until the clock reaches the target (or
  1.5 s), so it does not swing back to the old position while the seek is in flight.
- Album pages draw their header from the clicked card's summary, so the cover can morph via
  View Transitions before the track list loads.
- CI runs `pnpm test` (vitest geometry tests) in the UI step.

## Rollback

All work is in `app/src` plus one additive settings field (`#[serde(default)]`, older files
still load). Revert the branch; a saved `coverStyle` is ignored by older builds.

<!-- slug: vinyl-turntable-ui -->
