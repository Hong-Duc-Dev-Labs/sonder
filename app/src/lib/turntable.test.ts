import { describe, expect, it } from "vitest";
import {
  END_ANGLE,
  END_FRACTION,
  LABEL_FRACTION,
  RECORD_CENTER,
  RECORD_RADIUS,
  REST_ANGLE,
  START_ANGLE,
  START_FRACTION,
  armAngleFor,
  fractionForAngle,
  stylusAt,
} from "./turntable";

const distanceToCenter = (angle: number) => {
  const p = stylusAt(angle);
  return Math.hypot(p.x - RECORD_CENTER.x, p.y - RECORD_CENTER.y);
};

describe("tonearm geometry", () => {
  it("puts the stylus in the lead-in at the start and outside the label at the end", () => {
    expect(distanceToCenter(armAngleFor(0)) / RECORD_RADIUS).toBeCloseTo(START_FRACTION, 2);
    expect(distanceToCenter(armAngleFor(1)) / RECORD_RADIUS).toBeCloseTo(END_FRACTION, 2);
    expect(END_FRACTION).toBeGreaterThan(LABEL_FRACTION);
  });

  it("moves inward monotonically as the song plays", () => {
    let previous = Infinity;
    for (let i = 0; i <= 20; i++) {
      const r = distanceToCenter(armAngleFor(i / 20));
      expect(r).toBeLessThan(previous);
      previous = r;
    }
  });

  it("maps angles back to the same progress", () => {
    for (const f of [0, 0.1, 0.25, 0.5, 0.75, 0.9, 1]) {
      expect(fractionForAngle(armAngleFor(f))).toBeCloseTo(f, 4);
    }
  });

  it("clamps angles past either end of the record", () => {
    expect(fractionForAngle(START_ANGLE - 10)).toBe(0);
    expect(fractionForAngle(END_ANGLE + 10)).toBe(1);
  });

  it("rests clear of the record", () => {
    expect(distanceToCenter(REST_ANGLE)).toBeGreaterThan(RECORD_RADIUS * 1.05);
  });

  it("clamps progress outside 0..1", () => {
    expect(armAngleFor(-1)).toBeCloseTo(armAngleFor(0), 6);
    expect(armAngleFor(2)).toBeCloseTo(armAngleFor(1), 6);
  });
});
