// Motion and math primitives for the Folia Flow lyric stage.
// Damped spring physics, deterministic pseudo-random seeds per lyric line, and easing helpers
// ported directly from MyGo Music (egoist/mygo-music internal/app/ui_stage.go).

/**
 * Clamps a number to the [0, 1] range.
 */
export function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

/**
 * Linear interpolation between a and b by factor t.
 */
export function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

/**
 * Cubic ease-out curve for smooth deceleration.
 */
export function easeOutCubic(t: number): number {
  const c = clamp01(t);
  return 1 - Math.pow(1 - c, 3);
}

/**
 * Quintic smootherstep curve (Perlin) with zero 1st and 2nd derivatives at endpoints.
 */
export function smootherstep(t: number): number {
  const c = clamp01(t);
  return c * c * c * (c * (c * 6 - 15) + 10);
}

/**
 * Analytic step response of an underdamped harmonic oscillator (spring).
 * Normalized so t is in seconds, starting at 0 and settling at 1.0 with a soft bounce.
 * Tuned to natural frequency ~16.2 rad/s, damping ratio ~0.58 (10.5% bounce, settles < 475ms).
 */
export function springStep(t: number): number {
  if (t <= 0) return 0;
  const wd = 13.2;
  const decay = Math.exp(-9.4 * t);
  return 1 - decay * (Math.cos(wd * t) + 0.712 * Math.sin(wd * t));
}

/**
 * Deterministic pseudo-random number in [0, 1) derived from a seed and offset.
 * Uses 32-bit integer bit mixing (SplitMix32) to guarantee uniform distribution
 * and prevent coordinate correlation between adjacent words.
 */
export function lineRandom(seed: number, offset: number): number {
  let h = (Math.imul(seed | 0, 0x9e3779b9) ^ Math.imul(offset | 0, 0x85ebca6b)) >>> 0;
  h ^= h >>> 16;
  h = Math.imul(h, 0x85ebca6b);
  h ^= h >>> 13;
  h = Math.imul(h, 0xc2b2ae35);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

export interface WordShape {
  x: number;
  y: number;
  rotate: number;
  scale: number;
  drift: number;
  margin: number;
  last?: boolean;
}

export interface LineLayout {
  justify: "flex-start" | "center" | "flex-end";
  liftRatio: number;
  shapes: WordShape[];
}

/**
 * Computes organic line layout matching Folia / MyGo Music:
 * Lines alternate between start, center, and end alignment with organic vertical lift.
 */
export function computeLineLayout(startMs: number, wordCount: number): LineLayout {
  const seed = startMs;
  const justifyOptions: ("flex-start" | "center" | "flex-end")[] = ["flex-start", "center", "flex-end"];
  const liftOptions = [-0.14, 0, 0.12];

  const justify = justifyOptions[Math.abs(Math.floor(seed)) % 3];
  const liftRatio = liftOptions[Math.abs(Math.floor(seed * 2)) % 3];

  const shapes: WordShape[] = [];
  for (let i = 0; i < wordCount; i++) {
    const ws = seed + i * 8;
    shapes.push({
      x: (lineRandom(ws, 1) - 0.5) * 32,
      y: (lineRandom(ws, 2) - 0.5) * 28,
      rotate: (lineRandom(ws, 3) - 0.5) * 8,
      scale: 1.08 + lineRandom(ws, 4) * 0.16,
      drift: (lineRandom(ws, 5) - 0.5) * 8,
      margin: 16,
      last: i === wordCount - 1,
    });
  }
  return { justify, liftRatio, shapes };
}

export interface WordFrameState {
  x: number;
  y: number;
  scale: number;
  rotate: number;
  opacity: number;
  blur: number;
  textColorAlpha: number;
  glow: number;
  margin: number;
}

/**
 * Evaluates the visual state of a word at nowMs based on MyGo Music's exact spring and motion curves.
 */
export function evaluateWord(
  shape: WordShape,
  wordStartMs: number,
  wordEndMs: number,
  nowMs: number,
  reduced: boolean = false,
): WordFrameState {
  if (reduced) {
    const isLit = nowMs >= wordStartMs;
    return {
      x: 0,
      y: 0,
      scale: 1,
      rotate: 0,
      opacity: isLit ? 1 : 0.35,
      blur: 0,
      textColorAlpha: isLit ? 1 : 0.5,
      glow: isLit ? 0.4 : 0,
      margin: 14,
    };
  }

  const lookahead = 80; // ms
  const start = wordStartMs - lookahead;
  const end = Math.max(wordEndMs, wordStartMs + 150);
  const since = (nowMs - start) / 1000;
  const after = (nowMs - end) / 1000;
  const singing = Math.max(0.08, (end - wordStartMs) / 1000);

  // Waiting: small, off to the side, turned and unseen
  const waitingX = shape.x + Math.sin(shape.y) * 100;
  const waitingY = shape.y + Math.cos(shape.x) * 50;
  let x = waitingX;
  let y = waitingY;
  let scale = 0.5;
  let rotate = shape.rotate + 20;
  let opacity = 0;
  let blur = 10;
  let textColorAlpha = 0.5;
  let glow = 0;

  if (since > 0) {
    // Sung: springing to its place, larger, sharp, filling with light
    const e = springStep(since);
    x = lerp(waitingX, shape.x, e);
    y = lerp(waitingY, shape.y, e);
    scale = lerp(0.5, shape.scale * 1.4, e);
    rotate = lerp(shape.rotate + 20, shape.rotate, e);
    opacity = clamp01(since / 0.1);
    blur = 10 * (1 - clamp01(since / 0.2));
    textColorAlpha = lerp(0.5, 1.0, clamp01(since / singing));
    glow = clamp01(since / Math.max(0.08, 0.3 * singing));
  }

  if (after > 0) {
    // Past: smooth transition to rest scale using quintic smootherstep (zero jerk)
    const q = smootherstep(after / 0.5);
    scale = lerp(shape.scale * 1.4, shape.scale, q);
    opacity = lerp(1, 0.82, q);
    rotate = shape.rotate + shape.drift * clamp01(after / 5);
    textColorAlpha = lerp(1, 0.5, easeOutCubic(after / 0.8));
    glow = 1 - clamp01(after / 0.9);
  }

  return {
    x,
    y,
    scale,
    rotate,
    opacity,
    blur,
    textColorAlpha,
    glow,
    margin: shape.margin,
  };
}
