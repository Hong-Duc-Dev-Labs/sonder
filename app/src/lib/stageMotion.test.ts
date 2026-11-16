import { describe, expect, it } from "vitest";
import {
  clamp01,
  computeLineLayout,
  easeOutCubic,
  lerp,
  lineRandom,
  spaceWords,
  springStep,
  type WordShape,
} from "./stageMotion";

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

  it("spaceWords increases margin when words shift toward each other", () => {
    const shapes: WordShape[] = [
      { x: 15, y: 0, rotate: 0, scale: 1.1, drift: 0, margin: 0 },
      { x: -15, y: 0, rotate: 0, scale: 1.1, drift: 0, margin: 0 },
    ];
    const widths = [60, 60];
    const gap = 4;
    spaceWords(shapes, widths, gap, [true, false]);

    // toward = 15 - (-15) = 30
    // Margin for word 0 must include toward distance (at least 30px) + growth reserve + gap + spaceWidth
    expect(shapes[0].margin).toBeGreaterThan(45);
  });

  it("computeLineLayout calculates collision-free word margins for lyrics lines", () => {
    const words = [
      { text: "Vì", trailingSpace: true },
      { text: "mấy", trailingSpace: true },
      { text: "câu", trailingSpace: true },
      { text: "thương", trailingSpace: true },
    ];
    const layout = computeLineLayout(42000, words, 48);
    expect(layout.shapes.length).toBe(4);
    for (const shape of layout.shapes) {
      expect(shape.margin).toBeGreaterThanOrEqual(10);
    }
  });
});
