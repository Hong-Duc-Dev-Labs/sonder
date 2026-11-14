// Lyrics for the playing track and the line that is being sung. Lyrics found are kept for the
// session; "none" is not, so a .lrc added later or lookups turned on show up next time.

import { commands, type Lyrics, type LyricsLine } from "./bindings";

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
