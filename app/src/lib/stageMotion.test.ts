import { describe, expect, it } from "vitest";
import { clamp01, easeOutCubic, lerp, lineRandom, springStep } from "./stageMotion";

describe("stageMotion", () => {
  it("clamps values to [0, 1]", () => {
    expect(clamp01(-0.5)).toBe(0);
    expect(clamp01(0)).toBe(0);
    expect(clamp01(0.5)).toBe(0.5);
    expect(clamp01(1)).toBe(1);
    expect(clamp01(1.5)).toBe(1);
  });

  it("lerps linearly between a and b", () => {
    expect(lerp(10, 20, 0)).toBe(10);
    expect(lerp(10, 20, 0.5)).toBe(15);
    expect(lerp(10, 20, 1)).toBe(20);
  });

  it("eases out cubically", () => {
    expect(easeOutCubic(0)).toBe(0);
    expect(easeOutCubic(1)).toBe(1);
    expect(easeOutCubic(0.5)).toBeCloseTo(0.875, 3);
  });

  it("calculates spring response with bounce", () => {
    expect(springStep(0)).toBe(0);
    expect(springStep(-1)).toBe(0);
    // At t ~ 0.35s there is slight overshoot (> 1.0)
    expect(springStep(0.35)).toBeGreaterThan(1.0);
    // Settles close to 1.0 at t = 1.0s
    expect(springStep(1.0)).toBeCloseTo(1.0, 2);
  });

  it("generates deterministic random values in [0, 1)", () => {
    const val1 = lineRandom(1234, 1);
    const val2 = lineRandom(1234, 1);
    const val3 = lineRandom(1234, 2);
    expect(val1).toBe(val2);
    expect(val1).toBeGreaterThanOrEqual(0);
    expect(val1).toBeLessThan(1);
    expect(val3).not.toBe(val1);
  });
});
