// The record's spin, shared by every disc on screen so they stay in step. Angular velocity eases
// toward its target with a critically damped spring: quicker up to speed when play starts,
// a longer coast down when it stops, like a platter. The loop sleeps once the record is still.

import { app } from "./app.svelte";
import { Spring, onFrame, onReducedMotionChange, reducedMotion } from "./motion";

/** Degrees per second while playing: slow enough to keep the cover readable. */
export const SPIN_SPEED = 26;
/** Spring stiffness (ω² for unit mass) speeding up: settles in about 0.8 s. */
const SPIN_UP = 64;
/** Spring stiffness slowing down: settles in about 1.9 s. */
const SPIN_DOWN = 12;

type Target = (angle: number) => void;

const velocity = new Spring(0, { stiffness: SPIN_UP, damping: 1 });
let angle = 0;
const targets = new Set<Target>();
let stopLoop: (() => void) | null = null;

function frame(dt: number) {
  velocity.options.stiffness = velocity.target > velocity.value ? SPIN_UP : SPIN_DOWN;
  const moving = velocity.step(dt);
  angle = (angle + Math.max(0, velocity.value) * dt) % 360;
  for (const t of targets) t(angle);
  if (!moving && velocity.value === 0) sleep();
}

function wake() {
  if (!stopLoop && targets.size > 0) stopLoop = onFrame(frame);
}

function sleep() {
  stopLoop?.();
  stopLoop = null;
}

function setPlaying(playing: boolean) {
  if (reducedMotion()) {
    velocity.snap(0);
    sleep();
    return;
  }
  velocity.target = playing ? SPIN_SPEED : 0;
  wake();
}

$effect.root(() => {
  $effect(() => setPlaying(app.player.state === "playing"));
});
onReducedMotionChange(() => setPlaying(app.player.state === "playing"));

/**
 * Turns `el` with the record from now on. Returns a function that stops it. The transform is
 * written straight to the element every frame, outside Svelte's reactivity.
 */
export function attachSpin(el: HTMLElement): () => void {
  const target: Target = (a) => {
    el.style.transform = `rotate(${a.toFixed(2)}deg)`;
  };
  targets.add(target);
  target(angle);
  if (!velocity.settled || velocity.value !== 0) wake();
  return () => {
    targets.delete(target);
    if (targets.size === 0) sleep();
  };
}
