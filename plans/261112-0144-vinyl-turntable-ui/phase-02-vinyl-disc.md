---
phase: 2
title: "Phase 2: Vinyl disc"
status: completed
priority: P1
effort: "1d"
dependencies: [1]
---

# Phase 2: Vinyl disc

## Overview

A `VinylDisc.svelte` component: a still vinyl surface with a rotating label (the album
cover, round) on top. Rotation eases up when playback starts and coasts down when it stops.
Also a small spinning disc for the transport bar.

## Key Insights (from the source, re-expressed for the web)

- Real records' grooves are circles and their sheen comes from a fixed light, so the
  **surface never needs to rotate** — only the label does. The surface is drawn once; only one
  composited layer turns each frame (cheap on any GPU).
- Pausing desaturates the label slightly so "paused" is readable at a glance.

## Requirements

- [x] Layers, bottom to top, all in one square box sized by the parent:
  1. Soft drop shadow (CSS `box-shadow`/radial gradient), still.
  2. Vinyl surface, still: an SVG written for Sonder — black disc, groove texture
     (`repeating-radial-gradient` fine rings + a few wider "track gap" rings), two
     `conic-gradient` sheens at left/right of centre (fixed light, up-left), bright rim line.
     Rendered once; container has `contain: strict`.
  3. Label, **rotating**: the `sonder-art` 1000 px image clipped round at ~44 % of the
     diameter, plus album title set on a circular `<textPath>` in the ring between label and
     grooves. `will-change: transform`; rotation written as `transform: rotate(Xdeg)` from the
     frame loop, never through Svelte state.
  4. Label lighting, still: a radial highlight up-left and an inner edge shadow so the label
     looks printed onto the record, plus the centre spindle hole.
- [x] Spin engine (`app/src/lib/spin.ts`): angular velocity eases toward a target —
      26 °/s while playing, 0 otherwise — with a critically damped spring (spin-up ≈ 0.8 s,
      spin-down ≈ 1.6 s, slower down than up like a real platter). Angle integrates velocity;
      the loop sleeps once velocity reaches 0 and wakes on play.
- [x] Paused look: label `filter: saturate()` transitions 1 → 0.55 over `--dur-slow`.
- [x] Reduced motion: no spin, no transitions; still disc.
- [x] Missing art: label shows the existing placeholder gradient + disc icon.
- [x] `MiniDisc.svelte` (transport bar, 56 px): same layers without text and grooves
      simplified to two rings; shares the spin engine instance so both discs stay in phase.

## Related Code Files

- Create: `app/src/components/VinylDisc.svelte`, `app/src/components/MiniDisc.svelte`,
  `app/src/lib/spin.ts`.
- Modify: `app/src/components/TransportBar.svelte` (use `MiniDisc` when cover style is vinyl).

## Implementation Steps

1. Write the SVG surface with `viewBox="0 0 1000 1000"` so every measure is a fraction of the
   diameter; compare against the screenshot of a real LP at 300 and 600 px for groove density.
2. Spin engine: singleton `createSpin()` with `start()`, `stop()`, `angle`, `onFrame(cb)`;
   subscribes to the playback state from `app.player.state`.
3. Label component binds a ref and registers `el.style.transform = rotate(...)` on frame.
4. Ring text: `<textPath>` with `textLength` set to the circumference so long titles squeeze
   and short ones repeat with " · " separators.
5. MiniDisc reuses the spin engine; TransportBar swaps it in for the square thumb.

## Todo

- [x] Vinyl surface SVG
- [x] Spin engine with eased velocity + sleep
- [x] Rotating label + ring text
- [x] Paused desaturation
- [x] MiniDisc in transport bar
- [x] Reduced-motion path

## Success Criteria

- Chrome DevTools Performance (WebView2 remote debugging) while playing: one compositing
  layer animates; no layout or paint per frame on the surface; frame time < 4 ms on the dev PC.
- Play → disc reaches full speed smoothly in under 1 s; pause → coasts to a stop in ~1.6 s;
  no jump in angle at any point (angle continuous across play/pause/track change).
- `pnpm check` clean.

## Risk Assessment

- Large blurred/filtered layers on WebKitGTK (Linux) can be slow: keep filters off the
  rotating layer except `saturate`, and measure on Linux before Phase 5 ships.

## Security Considerations

- Images still come only from `sonder-art`; no new CSP sources.
