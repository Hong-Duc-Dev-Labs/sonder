import { describe, expect, it, vi } from "vitest";

vi.mock("./bindings", () => ({ commands: { lyricsGet: vi.fn() } }));

import { commands } from "./bindings";
import { activeLine, loadLyrics, prepareTimedLines, spreadWords } from "./lyrics";

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

describe("spreadWords", () => {
  it("splits English sentences into words", () => {
    const words = spreadWords("Hello world from Sonder", 0, 10000);
    expect(words.map((w) => w.text)).toEqual(["Hello", "world", "from", "Sonder"]);
  });

  it("splits Vietnamese sentences with diacritics into words", () => {
    const words = spreadWords("Tôi thấy hoa vàng trên cỏ xanh", 0, 7000);
    expect(words.map((w) => w.text)).toEqual([
      "Tôi",
      "thấy",
      "hoa",
      "vàng",
      "trên",
      "cỏ",
      "xanh",
    ]);
  });

  it("splits CJK characters into individual runes", () => {
    const jp = spreadWords("春はあけぼの", 0, 6000);
    expect(jp.map((w) => w.text)).toEqual(["春", "は", "あ", "け", "ぼ", "の"]);

    const cn = spreadWords("夜曲", 0, 2000);
    expect(cn.map((w) => w.text)).toEqual(["夜", "曲"]);
  });

  it("handles mixed Latin and CJK text", () => {
    const mixed = spreadWords("Hello 世界", 0, 5000);
    expect(mixed.map((w) => w.text)).toEqual(["Hello", "世", "界"]);
    expect(mixed[0].trailingSpace).toBe(true);
    expect(mixed[1].trailingSpace).toBe(false);
    expect(mixed[2].trailingSpace).toBe(false);
  });

  it("handles SMP code points like emojis or extension characters without breaking surrogates", () => {
    const words = spreadWords("Music 🎵 𩸽", 0, 3000);
    expect(words.map((w) => w.text)).toEqual(["Music", "🎵", "𩸽"]);
  });

  it("distributes 85% of duration and keeps contiguous time", () => {
    const startMs = 1000;
    const endMs = 11000; // 10000ms duration, 85% is 8500ms
    const words = spreadWords("One Two Three", startMs, endMs);
    expect(words.length).toBe(3);
    expect(words[0].startMs).toBe(startMs);
    expect(words[words.length - 1].endMs).toBe(startMs + 8500);

    for (let i = 0; i < words.length - 1; i++) {
      expect(words[i].endMs).toBe(words[i + 1].startMs);
    }
  });

  it("handles edge cases: empty, whitespace, single word, and endMs <= startMs", () => {
    expect(spreadWords("", 0, 5000)).toEqual([]);
    expect(spreadWords("   \t \n  ", 0, 5000)).toEqual([]);

    const single = spreadWords("Hello", 1000, 3000);
    expect(single.length).toBe(1);
    expect(single[0]).toEqual({
      text: "Hello",
      startMs: 1000,
      endMs: 1000 + Math.round(2000 * 0.85),
      trailingSpace: false,
    });

    const reversed = spreadWords("Hello", 2000, 1000);
    expect(reversed).toEqual([
      { text: "Hello", startMs: 2000, endMs: 2000, trailingSpace: false },
    ]);
  });
});

describe("prepareTimedLines", () => {
  it("calculates line endMs and timed words properly", () => {
    const rawLines = [
      { startMs: 0, text: "Hello world" },
      { startMs: 4000, text: "Line two" },
      { startMs: 20000, text: "Far away line" },
    ];
    const prepared = prepareTimedLines(rawLines);
    expect(prepared.length).toBe(3);

    // Line 0 ends when line 1 begins (4000ms)
    expect(prepared[0].endMs).toBe(4000);
    expect(prepared[0].words.length).toBe(2);

    // Line 1 is capped at 6000ms since line 2 is at 20000ms
    expect(prepared[1].endMs).toBe(4000 + 6000);
    expect(prepared[1].words.length).toBe(2);

    // Last line gets +6000ms
    expect(prepared[2].endMs).toBe(20000 + 6000);
    expect(prepared[2].words.length).toBe(3);
  });
});
