// Motion and math primitives for the Folia Flow lyric stage.
// Damped spring physics, deterministic pseudo-random seeds per lyric line, and easing helpers.

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
 * Analytic step response of an underdamped harmonic oscillator (spring).
 * Normalized so t is in seconds (or scaled unit), starting at 0 and settling at 1.0 with a soft bounce.
 * Natural frequency ~10 rad/s, damping ratio ~0.707.
 */
export function springStep(t: number): number {
  if (t <= 0) return 0;
  const e = Math.exp(-10 * t);
  return 1 - e * (Math.cos(10 * t) + Math.sin(10 * t));
}

/**
 * Deterministic pseudo-random number in [0, 1) derived from a seed (e.g. line start timestamp) and offset.
 * Guarantees consistent typography layout across seeks while giving organic variation.
 */
export function lineRandom(seed: number, offset: number): number {
  const x = Math.sin(seed * 9301 + offset * 49297 + 233280) * 10000;
  return x - Math.floor(x);
}
