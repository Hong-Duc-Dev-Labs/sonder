import { describe, expect, it } from "vitest";
import { barHeights, barLabel, labelledBars, minutesLeft } from "./stats";

describe("barLabel", () => {
  it("writes days and months in the interface language", () => {
    expect(barLabel("2026-11-14", "en")).toBe("14 Nov");
    expect(barLabel("2026-11", "en")).toBe("Nov 26");
    expect(barLabel("2026-11-14", "vi")).toMatch(/14/);
    expect(barLabel("2026-11", "vi")).toMatch(/11/);
  });

  it("leaves an unexpected label as it is", () => {
    expect(barLabel("soon", "en")).toBe("soon");
  });
});

describe("barHeights", () => {
  it("scales to the tallest bar", () => {
    expect(barHeights([0, 50, 100])).toEqual([0, 0.5, 1]);
    expect(barHeights([0, 0])).toEqual([0, 0]);
    expect(barHeights([])).toEqual([]);
  });
});

describe("labelledBars", () => {
  it("labels every bar of a short chart", () => {
    expect([...labelledBars(7, 7)]).toEqual([0, 1, 2, 3, 4, 5, 6]);
  });

  it("labels the ends and evenly spaced bars of a long one", () => {
    const shown = labelledBars(30, 6);
    expect(shown.size).toBe(6);
    expect(shown.has(0)).toBe(true);
    expect(shown.has(29)).toBe(true);
    expect(labelledBars(0).size).toBe(0);
  });
});

describe("minutesLeft", () => {
  it("rounds up and stops at zero", () => {
    expect(minutesLeft(10 * 60_000, 0)).toBe(10);
    expect(minutesLeft(10 * 60_000, 1)).toBe(10);
    expect(minutesLeft(60_001, 0)).toBe(2);
    expect(minutesLeft(0, 5000)).toBe(0);
  });
});
