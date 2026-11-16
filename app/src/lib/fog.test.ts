import { describe, expect, it } from "vitest";
import {
  hexToRgb,
  rgbToHsv,
  hsvToRgb,
  fogColors,
  createBassPulseState,
} from "./fog";

describe("fog color math", () => {
  it("converts hex to normalized rgb", () => {
    expect(hexToRgb("#ff0000")).toEqual([1, 0, 0]);
    expect(hexToRgb("#00ff00")).toEqual([0, 1, 0]);
    expect(hexToRgb("#0000ff")).toEqual([0, 0, 1]);
    expect(hexToRgb("invalid")).toEqual([0.2, 0.2, 0.25]);
  });

  it("converts rgb to hsv and back accurately", () => {
    const redHsv = rgbToHsv(1, 0, 0);
    expect(redHsv[0]).toBeCloseTo(0, 2);
    expect(redHsv[1]).toBeCloseTo(1, 2);
    expect(redHsv[2]).toBeCloseTo(1, 2);

    const redRgb = hsvToRgb(redHsv[0], redHsv[1], redHsv[2]);
    expect(redRgb[0]).toBeCloseTo(1, 2);
    expect(redRgb[1]).toBeCloseTo(0, 2);
    expect(redRgb[2]).toBeCloseTo(0, 2);
  });

  it("generates deep and fog shades matching MyGo's tuning", () => {
    const { deep, fog } = fogColors("#e06c75"); // reddish tint
    // Deep should be a dark base (v ≈ 0.30)
    const deepHsv = rgbToHsv(deep[0], deep[1], deep[2]);
    expect(deepHsv[2]).toBeCloseTo(0.3, 1);
    expect(deep[3]).toBe(1.0);

    // Fog should be a brighter, lighter mist (v ≈ 0.72)
    const fogHsv = rgbToHsv(fog[0], fog[1], fog[2]);
    expect(fogHsv[2]).toBeCloseTo(0.72, 1);
    expect(fog[3]).toBe(1.0);
  });

  it("handles desaturated / grayscale covers gracefully", () => {
    const { deep, fog } = fogColors("#888888");
    const deepHsv = rgbToHsv(deep[0], deep[1], deep[2]);
    const fogHsv = rgbToHsv(fog[0], fog[1], fog[2]);
    expect(deepHsv[1]).toBeLessThan(0.15);
    expect(fogHsv[1]).toBeLessThan(0.15);
  });
});

describe("bass pulse state", () => {
  it("rises fast and decays smoothly", () => {
    const meter = createBassPulseState();
    expect(meter.current).toBe(0);

    // Advance while playing
    meter.step(0.016, true, 500); // 500ms
    expect(meter.current).toBeGreaterThan(0);

    // Stop playing and step
    const beforeStop = meter.current;
    meter.step(0.1, false, 600);
    expect(meter.current).toBeLessThanOrEqual(beforeStop);
  });
});
