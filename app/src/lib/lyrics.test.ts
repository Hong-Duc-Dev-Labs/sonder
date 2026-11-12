import { describe, expect, it, vi } from "vitest";

vi.mock("./bindings", () => ({ commands: { lyricsGet: vi.fn() } }));

import { activeLine } from "./lyrics";

const lines = [0, 1000, 1000, 5000].map((startMs, i) => ({
  startMs,
  text: `l${i}`,
}));

describe("activeLine", () => {
  it("is -1 before the first line and with no lines", () => {
    expect(activeLine([{ startMs: 500, text: "a" }], 499)).toBe(-1);
    expect(activeLine([], 1000)).toBe(-1);
  });

  it("picks the last line that has started", () => {
    expect(activeLine(lines, 0)).toBe(0);
    expect(activeLine(lines, 999)).toBe(0);
    // Two lines starting together: the later one in the list is the one shown.
    expect(activeLine(lines, 1000)).toBe(2);
    expect(activeLine(lines, 4999)).toBe(2);
    expect(activeLine(lines, 1e9)).toBe(3);
  });
});
