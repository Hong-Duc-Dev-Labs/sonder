---
phase: 3
title: "Phase 3: Tonearm as seek control"
status: completed
priority: P1
effort: "1d"
dependencies: [1, 2]
---

# Phase 3: Tonearm as seek control

## Overview

A tonearm over the record whose stylus position is the song's progress: at 0 % it rests in
the lead-in near the rim, at 100 % just outside the label. Dragging the arm scrubs; a time arc
along the stylus path shows the target while dragging; releasing seeks. The transport-bar
seek slider stays.

## Requirements

- [x] `Tonearm.svelte`: SVG arm (pivot base, tube, headshell, counterweight) drawn for Sonder,
      pivot at the top-right of the stage; shading from the same up-left light as the disc.
      The arm and a blurred silhouette shadow are drawn once and **only rotated** about the
      pivot (`transform-origin` at the pivot).
- [x] Geometry in `app/src/lib/turntable.ts` (pure functions, unit tested):
      - `armAngleFor(fraction)`: law of cosines on pivot → record centre distance, stylus
        reach and target radius `r = lerp(0.93, labelRadius + 0.07, fraction)` × record radius.
      - `fractionForAngle(angle)` inverse (clamped), used while dragging.
      - Angles in degrees, screen space (0 = right, 90 = down).
- [x] Following playback: target angle from the Phase 1 clock each frame. Small changes snap;
      a jump > 1.5° (seek, new track) swings there with a spring (damping ≈ 0.9, slow).
- [x] Not playing / no track: arm lifts back to its rest post outside the record (spring).
- [x] Drag: pointer down on the arm's hit area (headshell + tube, ≥ 24 px wide) → pointer
      capture, arm lifts (scale 1.02, shadow offset grows), turns by the pointer's angle change
      around the pivot, clamped to [start, end]. While dragging: time arc + label
      ("1:23 / 4:56") along the stylus path. On release: `commands.playerSeek(ms)`; arm lowers
      after 300 ms. `Esc` cancels the drag.
- [x] Keyboard: the arm is a focusable `role="slider"` with `aria-valuetext`; ←/→ seek ∓5 s,
      PageUp/PageDown ∓30 s, Home/End.
- [x] Disabled (no duration): arm stays parked, not focusable.

## Related Code Files

- Create: `app/src/components/Tonearm.svelte`, `app/src/lib/turntable.ts`,
  `app/src/lib/turntable.test.ts`.
- Modify: `app/package.json` (add `vitest` dev dependency and `test` script).

## Implementation Steps

1. Write `turntable.ts` + tests first: angle(0) and angle(1) land the stylus on the expected
   radii within 0.5 % of the record radius; `fractionForAngle(armAngleFor(f)) ≈ f`.
2. SVG arm in a `viewBox` sharing units with the disc stage.
3. Follow loop on the clock; spring for jumps.
4. Pointer handling with `setPointerCapture`; compute angle via `atan2` around the pivot.
5. Time arc: SVG arc path from the start angle to the current angle at the stylus radius.

## Todo

- [x] Geometry + vitest tests
- [x] Arm SVG + shadow
- [x] Follow / park animations
- [x] Drag-to-scrub + time arc + Esc cancel
- [x] Keyboard slider semantics

## Success Criteria

- `pnpm test` (vitest) passes the geometry tests.
- Dragging the arm to the middle of its travel and releasing seeks to ~50 % (within 1 s on a
  4-minute track); the transport slider moves to match.
- Arm movement during normal playback is continuous (no 10 Hz stepping visible).

## Risk Assessment

- The arm overlaps the disc's click target — the disc has none; keep only the arm interactive.
- Pointer events on SVG `<g>` with transforms: hit-test a transparent wide stroke along the
  tube instead of the thin visible shapes.
