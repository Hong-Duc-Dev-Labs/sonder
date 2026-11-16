// Lyrics for the playing track and the line that is being sung. Lyrics found are kept for the
// session; "none" is not, so a .lrc added later or lookups turned on show up next time.

import { commands, type Lyrics, type LyricsLine } from "./bindings";
import { lineRandom } from "./stageMotion";

export type TimedWord = {
  text: string;
  startMs: number;
  endMs: number;
  trailingSpace?: boolean;
  dx?: number;
  dy?: number;
  rot?: number;
  drift?: number;
  scale?: number;
};

export type TimedLine = LyricsLine & {
  endMs: number;
  words: TimedWord[];
};

const loaded = new Map<string, Promise<Lyrics | null>>();
/** Tracks kept; lyrics are small but a long session plays many. */
const KEEP = 200;

/** The track's lyrics, or null when there are none (or the lookup failed). */
export function loadLyrics(
  trackId: number,
  online: boolean,
): Promise<Lyrics | null> {
  const key = `${trackId}:${online}`;
  let found = loaded.get(key);
  if (!found) {
    found = commands.lyricsGet(trackId).then(
      (lyrics) => {
        if (!lyrics) loaded.delete(key);
        return lyrics;
      },
      () => {
        loaded.delete(key);
        return null;
      },
    );
    loaded.set(key, found);
    if (loaded.size > KEEP) loaded.delete(loaded.keys().next().value!);
  }
  return found;
}

/**
 * Index of the line being sung at `ms`: the last one that has started. -1 before the first.
 * Lines are in time order.
 */
export function activeLine(lines: LyricsLine[], ms: number): number {
  let lo = 0;
  let hi = lines.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (lines[mid].startMs <= ms) {
      found = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return found;
}

/** Drops what is kept for `trackId`, after its own lyrics changed. */
export function forgetLyrics(trackId: number) {
  for (const key of [...loaded.keys()]) {
    if (key.startsWith(`${trackId}:`)) loaded.delete(key);
  }
}

const CJK_REGEX = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;

function tokenWeight(token: string): number {
  const chars = Array.from(token);
  let w = 0;
  for (const c of chars) {
    if (/[.,!?:;…—"'\(\)\[\]{}]/u.test(c)) {
      w += 0.25;
    } else {
      w += 1.0;
    }
  }
  return Math.max(0.5, w);
}

/**
 * Splits a lyric line into individual timed words using character-count weighting
 * and an 85% active singing span (15% trailing breath/pause).
 * CJK characters are separated into single runes; Latin/Vietnamese are separated by whitespace.
 */
export function spreadWords(text: string, startMs: number, endMs: number): TimedWord[] {
  const trimmed = text.trim();
  if (!trimmed) return [];

  const tokens: { text: string; trailingSpace: boolean }[] = [];
  let current = "";

  for (const char of trimmed) {
    if (/\s/u.test(char)) {
      if (current) {
        tokens.push({ text: current, trailingSpace: true });
        current = "";
      } else if (tokens.length > 0) {
        tokens[tokens.length - 1].trailingSpace = true;
      }
    } else if (CJK_REGEX.test(char)) {
      if (current) {
        tokens.push({ text: current, trailingSpace: false });
        current = "";
      }
      tokens.push({ text: char, trailingSpace: false });
    } else {
      current += char;
    }
  }
  if (current) {
    tokens.push({ text: current, trailingSpace: false });
  }

  if (tokens.length === 0) return [];

  const safeEndMs = Math.max(startMs, endMs);
  const span = (safeEndMs - startMs) * 0.85;

  let totalWeight = 0;
  for (const token of tokens) {
    totalWeight += tokenWeight(token.text);
  }

  if (totalWeight <= 0) return [];

  let cumWeight = 0;
  const result: TimedWord[] = [];

  for (const token of tokens) {
    const weight = tokenWeight(token.text);
    const nextWeight = cumWeight + weight;
    const wordStart = startMs + Math.round((cumWeight / totalWeight) * span);
    const wordEnd = startMs + Math.round((nextWeight / totalWeight) * span);
    cumWeight = nextWeight;
    result.push({
      text: token.text,
      startMs: wordStart,
      endMs: wordEnd,
      trailingSpace: token.trailingSpace,
    });
  }

  return result;
}

/**
 * Prepares timed lines with word-level breakdown and bounded line durations
 * (up to 6s singing duration per line or until the next line starts).
 */
export function prepareTimedLines(lines: LyricsLine[]): TimedLine[] {
  return lines.map((line, i) => {
    const nextLine = lines[i + 1];
    const endMs = nextLine
      ? Math.max(line.startMs, Math.min(line.startMs + 6000, nextLine.startMs))
      : line.startMs + 6000;
    const words = spreadWords(line.text, line.startMs, endMs).map((w, wi) => ({
      ...w,
      dx: (lineRandom(line.startMs, wi * 5 + 1) - 0.5) * 28,
      dy: (lineRandom(line.startMs, wi * 5 + 2) - 0.5) * 22,
      rot: (lineRandom(line.startMs, wi * 5 + 3) - 0.5) * 8,
      drift: (lineRandom(line.startMs, wi * 5 + 4) - 0.5) * 12,
      scale: 1.05 + lineRandom(line.startMs, wi * 5 + 5) * 0.18,
    }));
    return {
      ...line,
      endMs,
      words,
    };
  });
}
