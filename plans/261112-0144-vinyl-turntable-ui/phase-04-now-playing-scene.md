---
phase: 4
title: "Phase 4: Now Playing scene, track transitions, blurred background"
status: completed
priority: P1
effort: "1d"
dependencies: [2, 3]
---

# Phase 4: Now Playing scene, track transitions, blurred background

## Overview

Rebuild `NowPlayingView.svelte` around the turntable: a stage holding the disc, the arm and
its base, the metadata and format badges beside it, a blurred cover background, and motion
when the track changes.

## Requirements

- [x] Layout: stage (square, `min(70vh, 60vw)`) on the left, text column on the right; below
      ~1000 px wide the text goes under the stage. Square cover style keeps today's layout,
      with the same background and transitions.
- [x] Background: two stacked layers of the current cover (300 px thumbnail is enough once
      blurred) with `filter: blur(60px) saturate(1.4)`, scaled 1.2 to hide blur edges, dimmed
      by a dark gradient; on track change the new layer fades in over `--dur-slow` and the old
      one is removed after. Paused: background dims a further 15 %.
- [x] Track change (queue index changes while the view is open):
      1. Arm lifts (spring) to rest.
      2. Old disc slides out to the left and fades (`--dur-med`, `--ease-standard`) while still
         spinning at its current velocity.
      3. New disc slides in from the right (`--ease-out`), then the arm drops to the lead-in.
      Previous/next direction reverses the slide. Implemented with a keyed `{#key}` block and
      Svelte `in:`/`out:` transitions driven by CSS transforms only.
- [x] Title/artist crossfade + 8 px rise on change; format badges update without layout jump.
- [x] Nothing playing: empty stage shows a still record with no label art and the arm parked,
      plus the existing hint text.
- [x] Reduced motion: crossfades only.

## Related Code Files

- Modify: `app/src/components/NowPlayingView.svelte`, `app/src/lib/app.svelte.ts` (expose the
  previous index to know slide direction).
- Create: `app/src/components/CoverBackdrop.svelte`, `app/src/components/TurntableStage.svelte`.

## Implementation Steps

1. `TurntableStage` composes `VinylDisc` + `Tonearm` + arm base; exposes `trackKey`.
2. `CoverBackdrop` with two-layer crossfade; reused by the square style.
3. Direction: compare new index with previous in `app` state.
4. Sequence the arm lift → slide → drop with a small async timeline (awaiting transition
   ends), cancellable if the user skips again mid-sequence (latest wins).

## Todo

- [x] Stage + responsive layout
- [x] Cover backdrop crossfade
- [x] Track-change sequence with direction + cancellation
- [x] Metadata transitions
- [x] Empty and reduced-motion states

## Success Criteria

- Pressing Next five times quickly ends on the right track with no stuck/duplicated discs.
- Track change shows no layout shift in the text column (fixed heights).
- `pnpm check` and `pnpm build` clean.

## Risk Assessment

- 60 px blur over a full window costs GPU on low-end machines: blur a 300 px image and scale
  it, never blur the full-size layer; drop to a flat gradient if `matchMedia('(prefers-reduced-transparency)')`.
