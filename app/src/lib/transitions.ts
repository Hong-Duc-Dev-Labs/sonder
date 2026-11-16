// Svelte transitions shared by the animated views. Every one falls back to a short fade when the
// OS asks for reduced motion, and moves only transform and opacity.

import { cubicIn, cubicOut } from "svelte/easing";
import { viewTransitionActive } from "./app.svelte";
import { reducedMotion } from "./motion";

/** The old record leaves once the arm has started lifting off it. */
export const RECORD_LEAVE_DELAY = 120;
export const RECORD_LEAVE_MS = 380;
/** The new record lands once the old one is mostly gone, so they never collide. */
export const RECORD_LAND_DELAY = 220;
export const RECORD_LAND_MS = 560;

/** Ease-out with a small overshoot (about 3 %), so a landing record settles like a platter. */
function landing(t: number): number {
  const s = 0.9;
  const f = t - 1;
  return f * f * ((s + 1) * f + s) + 1;
}

/** A record sliding along the queue: in from `dir`'s side, out to the other side. */
export function slideRecord(
  _node: Element,
  { dir, leaving }: { dir: 1 | -1; leaving: boolean },
) {
  if (reducedMotion()) return { duration: 150, css: (t: number) => `opacity: ${t}` };
  const side = leaving ? -dir : dir;
  return {
    delay: leaving ? RECORD_LEAVE_DELAY : RECORD_LAND_DELAY,
    duration: leaving ? RECORD_LEAVE_MS : RECORD_LAND_MS,
    easing: leaving ? cubicIn : landing,
    css: (t: number, u: number) =>
      `transform: translateX(${side * u * 115}%) rotate(${side * u * 5}deg); opacity: ${leaving ? Math.max(0, 1 - u * 1.4) : Math.min(1, t * 1.6)}`,
  };
}

/** Text arriving: rises a few pixels while it fades in. */
export function rise(_node: Element, { delay = 0 }: { delay?: number } = {}) {
  if (reducedMotion()) return { duration: 120, css: (t: number) => `opacity: ${t}` };
  return {
    delay,
    duration: 240,
    easing: cubicOut,
    css: (t: number, u: number) => `transform: translateY(${u * 6}px); opacity: ${t}`,
  };
}

/**
 * A view arriving: fades in while rising a few pixels. Only opacity and transform animate, on
 * the content alone, so the glass chrome around it keeps running. Still while a View
 * Transition is morphing a cover, which animates the page itself.
 */
export function pageIn(_node: Element) {
  if (reducedMotion() || viewTransitionActive()) return { duration: 0 };
  return {
    duration: 240,
    easing: cubicOut,
    css: (t: number, u: number) => `opacity: ${t}; transform: translateY(${u * 6}px)`,
  };
}

/** A quick fade. */
export function fadeQuick(_node: Element, { duration = 160 }: { duration?: number } = {}) {
  return { duration: reducedMotion() ? 1 : duration, css: (t: number) => `opacity: ${t}` };
}
