---
phase: 1
title: "Phase 1: Motion foundation and cover style"
status: completed
priority: P1
effort: "4h"
dependencies: []
---

# Phase 1: Motion foundation and cover style

## Overview

Shared pieces every later phase uses: motion tokens, a reduced-motion switch, a smooth
playback clock (the engine sends position at ~10 Hz; the disc and the arm need it every
frame), and the `coverStyle` setting persisted through Rust.

## Requirements

- [x] Motion tokens in `app/src/app.css`: durations (`--dur-fast` 160 ms, `--dur-med` 280 ms,
      `--dur-slow` 520 ms) and easings (`--ease-out` `cubic-bezier(0.22, 1, 0.36, 1)`,
      `--ease-standard` `cubic-bezier(0.4, 0, 0.2, 1)`, `--ease-spring` via CSS `linear()`
      approximating a damped spring). One place to tune the whole app's feel.
- [x] `prefers-reduced-motion: reduce` turns transitions to near-instant fades and stops
      continuous motion (spin, arm swing animations); a `motion` helper in TS exposes the same.
- [x] `app/src/lib/clock.svelte.ts`: one `requestAnimationFrame` loop that extrapolates
      `elapsedMs` between engine ticks while playing (`elapsed = lastTick + (now - tickTime)`),
      clamps to duration, snaps on seek/track change, and stops when nothing subscribes or the
      window is hidden (`visibilitychange`). Subscribers get `(elapsedMs, dtSeconds)` per frame.
- [x] `Settings.coverStyle: "vinyl" | "square"` (default `vinyl`) in
      `app/src-tauri/src/settings.rs` + `dto.rs` `CoverStyle` enum; bindings regenerated
      (`SONDER_UPDATE_BINDINGS=1 cargo test -p sonder-app typescript`).
- [x] Settings view: "Now Playing cover" choice (Vinyl / Square) next to DSP.

## Related Code Files

- Modify: `app/src/app.css`, `app/src-tauri/src/settings.rs`, `app/src-tauri/src/dto.rs`,
  `app/src/lib/bindings.ts` (generated), `app/src/components/SettingsView.svelte`.
- Create: `app/src/lib/clock.svelte.ts`, `app/src/lib/motion.ts`.

## Implementation Steps

1. Add tokens and a global `@media (prefers-reduced-motion: reduce)` block.
2. `motion.ts`: `reducedMotion()` (matchMedia, live), `spring(from, to, {stiffness, damping})`
   integrator used by later phases for velocity easing — semi-implicit Euler at a fixed 1/120 s
   step so frame rate does not change the feel.
3. `clock.svelte.ts`: subscribe/unsubscribe API; per-frame callbacks write to the DOM directly
   (no Svelte state churn at 60 Hz); a `$state` value updated at most 10 Hz for text.
4. Rust: add enum + field with `#[serde(default)]`, extend the settings round-trip test with
   `coverStyle`; regenerate bindings.
5. Settings UI radio group.

## Todo

- [x] Motion tokens + reduced-motion block
- [x] `motion.ts` spring integrator with unit-free fixed step
- [x] Playback clock with visibility pause
- [x] `coverStyle` setting end to end + test
- [x] Settings UI

## Success Criteria

- `cargo test -p sonder-app` passes (settings round-trip covers `coverStyle`; bindings fresh).
- `pnpm check` clean.
- With the clock subscribed and playback running, the extrapolated time never runs ahead of
  the next engine tick by more than one frame, and pauses exactly on pause (checked by logging
  both during a manual run).

## Risk Assessment

- Extrapolation drift if the engine stalls (device lost): stop extrapolating when state is not
  `playing`, and clamp to the last tick + 250 ms when ticks stop arriving.
