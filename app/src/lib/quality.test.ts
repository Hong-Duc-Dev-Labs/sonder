import { describe, expect, it } from "vitest";
import {
  evidenceValue,
  frequencyTicks,
  heat,
  isFlagged,
  linearTicks,
  logX,
  reasonKey,
  statusTone,
  tally,
  verdictTone,
} from "./quality";

describe("verdict presentation", () => {
  it("flags only verdicts that point at a problem", () => {
    expect(verdictTone("genuine")).toBe("ok");
    expect(verdictTone("suspect")).toBe("warn");
    expect(verdictTone("transcoded")).toBe("bad");
    expect(verdictTone("fakeHiRes")).toBe("bad");
    expect(verdictTone("fakeBitDepth")).toBe("bad");
    expect(verdictTone("inconclusive")).toBe("faint");
    expect(isFlagged("genuine")).toBe(false);
    expect(isFlagged("inconclusive")).toBe(false);
    expect(isFlagged("suspect")).toBe(true);
    expect(isFlagged("transcoded")).toBe(true);
  });

  it("never draws an unmeasured row as a pass", () => {
    expect(statusTone("notMeasured")).toBe("faint");
    expect(statusTone("pass")).toBe("ok");
  });

  it("picks the reason by criterion and status", () => {
    expect(reasonKey({ id: "cutoff", status: "fail", value: 16000 })).toBe("quality.reason.cutoff.fail");
    expect(reasonKey({ id: "md5", status: "notMeasured", value: null })).toBe("quality.reason.md5.other");
    expect(reasonKey({ id: "quietLevel", status: "info", value: -40 })).toBe("quality.reason.quietLevel.other");
  });
});

describe("evidenceValue", () => {
  it("writes each measurement in its unit", () => {
    expect(evidenceValue({ id: "cutoff", status: "fail", value: 16100 })).toBe("16.1 kHz");
    expect(evidenceValue({ id: "holes", status: "fail", value: 0.237 })).toBe("24 %");
    expect(evidenceValue({ id: "bitDepth", status: "fail", value: 16 })).toBe("16 bit");
    expect(evidenceValue({ id: "slope", status: "fail", value: 52.16 })).toBe("52.2 dB");
    expect(evidenceValue({ id: "md5", status: "pass", value: null })).toBe("");
  });
});

describe("tally", () => {
  it("counts rows per status, unmeasured ones apart from passes", () => {
    const counts = tally([
      { id: "cutoff", status: "pass", value: 46_800 },
      { id: "slope", status: "warn", value: 40 },
      { id: "md5", status: "notMeasured", value: null },
      { id: "holes", status: "notMeasured", value: null },
    ]);
    expect(counts).toEqual({ pass: 1, warn: 1, fail: 0, info: 0, notMeasured: 2 });
  });
});

describe("chart scales", () => {
  it("places frequencies on a log axis", () => {
    expect(logX(20, 20, 20_000)).toBe(0);
    expect(logX(20_000, 20, 20_000)).toBe(1);
    expect(logX(632.4555, 20, 20_000)).toBeCloseTo(0.5, 4);
    expect(logX(5, 20, 20_000)).toBe(0);
  });

  it("keeps ticks inside the axis", () => {
    expect(frequencyTicks(22_050)).toEqual([100, 1000, 5000, 10_000, 16_000, 20_000]);
    expect(frequencyTicks(48_000)).toContain(24_000);
    expect(frequencyTicks(48_000)).not.toContain(48_000);
  });

  it("spaces linear ticks evenly and keeps them inside the axis", () => {
    expect(linearTicks(48_000)).toEqual([4000, 8000, 12_000, 16_000, 20_000, 24_000, 28_000, 32_000, 36_000, 40_000, 44_000]);
    expect(linearTicks(22_050)).toEqual([2000, 4000, 6000, 8000, 10_000, 12_000, 14_000, 16_000, 18_000, 20_000]);
    expect(linearTicks(88_200).length).toBeLessThanOrEqual(12);
    expect(linearTicks(88_200).at(-1)!).toBeLessThan(88_200);
  });

  it("ramps from black to pale yellow", () => {
    expect(heat(0)).toEqual([0, 0, 4]);
    expect(heat(255)).toEqual([252, 255, 164]);
    const mid = heat(140);
    expect(mid[0]).toBeGreaterThan(mid[2]);
  });
});

describe("catalog coverage", () => {
  it("has a sentence for every criterion and status group the report can show", async () => {
    const { en } = await import("./locales/en");
    const criteria = [
      "cutoff",
      "slope",
      "stability",
      "holes",
      "stereoField",
      "channelSymmetry",
      "bitDepth",
      "quietLevel",
      "hiRes",
      "dsdSource",
      "md5",
    ] as const;
    const statuses = ["pass", "warn", "fail", "info", "notMeasured"] as const;
    for (const id of criteria) {
      expect(en).toHaveProperty(`quality.criterion.${id}`);
      for (const status of statuses) {
        expect(en).toHaveProperty(reasonKey({ id, status, value: null }));
      }
    }
    const verdicts = ["genuine", "suspect", "transcoded", "fakeHiRes", "fakeBitDepth", "inconclusive"] as const;
    for (const v of verdicts) {
      for (const prefix of ["verdict", "short", "summary"]) expect(en).toHaveProperty(`quality.${prefix}.${v}`);
    }
    for (const s of statuses) expect(en).toHaveProperty(`quality.status.${s}`);
  });
});
