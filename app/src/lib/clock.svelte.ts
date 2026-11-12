// Playback time for animation. The engine reports the position about ten times a second; the
// record and the tonearm need it every frame. Between reports, time is extrapolated from the
// last one while playing, never past the duration and never for longer than a report is late.

import { app } from "./app.svelte";

/** How far time may run past the last report when reports stop (device lost, stall). */
const MAX_AHEAD_MS = 250;
/** A change bigger than this between reports is a seek or a new track: follow it at once. */
const JUMP_MS = 300;

let lastTick = 0;
let tickAt = 0;
let shown = 0;
let index: number | null = null;
let playing = false;
let duration: number | null = null;

$effect.root(() => {
  $effect(() => {
    const snap = app.player;
    const now = performance.now();
    const jumped = snap.index !== index || Math.abs(snap.elapsedMs - extrapolate(now)) > JUMP_MS;
    index = snap.index;
    playing = snap.state === "playing";
    duration = snap.durationMs;
    lastTick = snap.elapsedMs;
    tickAt = now;
    if (jumped || !playing) shown = snap.elapsedMs;
  });
});

function extrapolate(now: number): number {
  if (!playing) return lastTick;
  const ahead = Math.min(now - tickAt, MAX_AHEAD_MS);
  const t = lastTick + Math.max(0, ahead);
  return duration != null ? Math.min(t, duration) : t;
}

/**
 * Milliseconds into the current track at `now` (a `performance.now()` time). While playing on
 * the same track it never moves backwards, so animations driven by it do not jitter.
 */
export function playbackMs(now = performance.now()): number {
  const t = extrapolate(now);
  if (playing && t < shown && shown - t < JUMP_MS) return shown;
  shown = t;
  return t;
}
