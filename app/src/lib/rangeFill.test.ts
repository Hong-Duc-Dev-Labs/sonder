import { describe, expect, it } from "vitest";
import { fillPercent } from "./rangeFill";

describe("fillPercent", () => {
  it("maps the value onto the track, clamped to its ends", () => {
    expect(fillPercent(-60, -60, 0)).toBe("0.00%");
    expect(fillPercent(-15, -60, 0)).toBe("75.00%");
    expect(fillPercent(0, -24, 12)).toBe("66.67%");
    expect(fillPercent(20, 0, 10)).toBe("100.00%");
    expect(fillPercent(-5, 0, 10)).toBe("0.00%");
  });

  it("treats an empty range as unlit", () => {
    expect(fillPercent(3, 5, 5)).toBe("0%");
  });
});
