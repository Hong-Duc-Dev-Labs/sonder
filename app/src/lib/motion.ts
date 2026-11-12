// Motion primitives shared by the animated parts of the UI: the OS reduced-motion setting, one
// requestAnimationFrame loop for everything that moves every frame, and a spring integrator.

const reducedQuery =
  typeof matchMedia === "function" ? matchMedia("(prefers-reduced-motion: reduce)") : null;

/** Whether the OS asks for less motion. Read it when deciding, not once at startup. */
export function reducedMotion(): boolean {
  return reducedQuery?.matches ?? false;
}

/** Calls `fn` whenever the reduced-motion setting changes; returns an unsubscribe function. */
export function onReducedMotionChange(fn: (reduced: boolean) => void): () => void {
  if (!reducedQuery) return () => {};
  const listener = (e: MediaQueryListEvent) => fn(e.matches);
  reducedQuery.addEventListener("change", listener);
  return () => reducedQuery.removeEventListener("change", listener);
}

/** Called every frame with the seconds since the previous frame (capped) and the frame time. */
export type FrameFn = (dt: number, now: number) => void;

const subscribers = new Set<FrameFn>();
let handle = 0;
let last = 0;
/** Longest step a frame may report, so a stall (hidden window, debugger) does not jump. */
const MAX_DT = 1 / 20;

function tick(now: number) {
  handle = 0;
  const dt = last ? Math.min((now - last) / 1000, MAX_DT) : 0;
  last = now;
  for (const fn of [...subscribers]) fn(dt, now);
  schedule();
}

function schedule() {
  if (handle || subscribers.size === 0 || document.hidden) {
    if (subscribers.size === 0 || document.hidden) last = 0;
    return;
  }
  handle = requestAnimationFrame(tick);
}

document.addEventListener("visibilitychange", () => {
  last = 0;
  schedule();
});

/**
 * Runs `fn` every frame until the returned function is called. The shared loop stops when no
 * one is subscribed and while the window is hidden.
 */
export function onFrame(fn: FrameFn): () => void {
  subscribers.add(fn);
  schedule();
  return () => {
    subscribers.delete(fn);
  };
}

export type SpringOptions = {
  /** Stiffness k (per second squared, unit mass). Higher settles faster. */
  stiffness: number;
  /** Damping ratio: 1 is critically damped (no overshoot), below 1 overshoots. */
  damping: number;
};

/** Integration step: fixed, so the feel does not depend on the display's frame rate. */
const STEP = 1 / 120;
const REST = 1e-3;

/** A value pulled toward `target` by a damped spring. Advance it with `step(dt)`. */
export class Spring {
  value: number;
  velocity = 0;
  target: number;
  options: SpringOptions;
  private carry = 0;

  constructor(value: number, options: SpringOptions) {
    this.value = value;
    this.target = value;
    this.options = options;
  }

  get settled(): boolean {
    return Math.abs(this.target - this.value) < REST && Math.abs(this.velocity) < REST;
  }

  /** Jumps to `value` with no motion. */
  snap(value: number) {
    this.value = value;
    this.target = value;
    this.velocity = 0;
    this.carry = 0;
  }

  /** Advances by `dt` seconds. Returns whether it is still moving. */
  step(dt: number): boolean {
    if (this.settled) {
      this.value = this.target;
      this.velocity = 0;
      return false;
    }
    const k = this.options.stiffness;
    const c = 2 * this.options.damping * Math.sqrt(k);
    this.carry += dt;
    while (this.carry >= STEP) {
      this.carry -= STEP;
      // Semi-implicit Euler: stable for the stiffness values used in the UI.
      const accel = k * (this.target - this.value) - c * this.velocity;
      this.velocity += accel * STEP;
      this.value += this.velocity * STEP;
    }
    return true;
  }
}
