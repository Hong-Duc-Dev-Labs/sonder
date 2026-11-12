import { describe, expect, it } from "vitest";
import { ZONES, seedOf, trackGaps } from "./vinylSurface";

describe("vinyl surface layout", () => {
  it("gives an album the same record every time and different albums different ones", () => {
    expect(seedOf("Kind of Blue")).toBe(seedOf("Kind of Blue"));
    expect(trackGaps(seedOf("Kind of Blue"))).toEqual(trackGaps(seedOf("Kind of Blue")));
    expect(trackGaps(seedOf("Kind of Blue"))).not.toEqual(trackGaps(seedOf("Blue Train")));
  });

  it("puts 3 to 6 track gaps strictly inside the grooved area, outermost first", () => {
    for (const title of ["", "A", "Kind of Blue", "Citopia", "Yesteryear (Deluxe)", "x".repeat(200)]) {
      const gaps = trackGaps(seedOf(title));
      expect(gaps.length).toBeGreaterThanOrEqual(3);
      expect(gaps.length).toBeLessThanOrEqual(6);
      for (const g of gaps) {
        expect(g).toBeGreaterThan(ZONES.programmeIn);
        expect(g).toBeLessThan(ZONES.programmeOut);
      }
      expect([...gaps].sort((a, b) => b - a)).toEqual(gaps);
    }
  });

  it("keeps the zones in order from the label to the rim", () => {
    const order = [ZONES.label, ZONES.runOut, ZONES.programmeIn, ZONES.programmeOut, ZONES.leadInOut, ZONES.lipIn, 1];
    expect([...order].sort((a, b) => a - b)).toEqual(order);
  });
});
