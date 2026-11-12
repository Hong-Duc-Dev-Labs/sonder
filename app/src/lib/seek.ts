// Keyboard seeking shared by the seek bars: arrows step 5 s, Page keys 30 s, Home/End jump.
// Repeated keys add up even before the engine reports the previous seek, so three quick
// presses move 15 s, not 5.

import { app } from "./app.svelte";
import { commands } from "./bindings";
import { playbackMs } from "./clock.svelte";

const KEY_STEP_MS = 5_000;
const PAGE_STEP_MS = 30_000;
/** How long a sent seek stays the base for the next key press. */
const CHAIN_MS = 700;

export function keySeeker(totalMs: () => number) {
  let target: { ms: number; until: number } | null = null;

  function seekTo(ms: number) {
    const total = totalMs();
    const to = Math.round(Math.min(Math.max(0, total - 500), Math.max(0, ms)));
    target = { ms: to, until: performance.now() + CHAIN_MS };
    app.run(commands.playerSeek(to));
  }

  function seekBy(delta: number) {
    const now = performance.now();
    seekTo((target && target.until > now ? target.ms : playbackMs(now)) + delta);
  }

  /** Handles a key on a seek control; returns whether it was a seek key. */
  function onKey(e: KeyboardEvent): boolean {
    const steps: Record<string, number> = {
      ArrowLeft: -KEY_STEP_MS,
      ArrowDown: -KEY_STEP_MS,
      ArrowRight: KEY_STEP_MS,
      ArrowUp: KEY_STEP_MS,
      PageDown: -PAGE_STEP_MS,
      PageUp: PAGE_STEP_MS,
    };
    if (e.key in steps) seekBy(steps[e.key]);
    else if (e.key === "Home") seekTo(0);
    else if (e.key === "End") seekTo(totalMs());
    else return false;
    e.preventDefault();
    return true;
  }

  return { seekTo, seekBy, onKey };
}
