import { describe, expect, it, vi } from "vitest";

vi.mock("./bindings", () => ({ commands: { lyricsGet: vi.fn() } }));

import { commands } from "./bindings";
import { activeLine, loadLyrics } from "./lyrics";

const lyricsGet = vi.mocked(commands.lyricsGet);

describe("loadLyrics", () => {
  it("keeps lyrics found but asks again after none or a failure", async () => {
    const found = { source: "LRCLIB", synced: true, instrumental: false, lines: [] };
    lyricsGet.mockResolvedValueOnce(found);
    expect(await loadLyrics(1, true)).toBe(found);
    expect(await loadLyrics(1, true)).toBe(found);
    expect(lyricsGet).toHaveBeenCalledTimes(1);

    lyricsGet.mockResolvedValueOnce(null).mockRejectedValueOnce("offline").mockResolvedValueOnce(found);
    expect(await loadLyrics(2, true)).toBeNull();
    expect(await loadLyrics(2, true)).toBeNull();
    expect(await loadLyrics(2, true)).toBe(found);
    expect(lyricsGet).toHaveBeenCalledTimes(4);
  });
});

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
