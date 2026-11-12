// Lyrics for the playing track: fetched once per track (and per online setting, so turning
// lookups on retries tracks that had none), and the line that is being sung.

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
    found = commands.lyricsGet(trackId).catch(() => {
      // A failure is not remembered: the next play of the track asks again.
      loaded.delete(key);
      return null;
    });
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
