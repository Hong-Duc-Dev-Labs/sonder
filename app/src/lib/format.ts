import type { OutputInfo, PlayerSnapshot, SourceInfo } from "./bindings";

export function duration(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms)) return "–:––";
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

export function rate(hz: number): string {
  const khz = hz / 1000;
  return `${Number.isInteger(khz) ? khz : khz.toFixed(1)} kHz`;
}

export function sourceLabel(s: SourceInfo): string {
  return `${s.bits}-bit / ${rate(s.rate)}`;
}

export function outputLabel(o: OutputInfo): string {
  const container = o.containerBits === o.bits ? `${o.bits}-bit` : `${o.bits}-in-${o.containerBits}`;
  return `${container} / ${rate(o.rate)}`;
}

/** Whether the device receives the decoded samples unchanged. */
export function isBitPerfect(snap: PlayerSnapshot): boolean {
  const { source, output } = snap;
  return (
    snap.volumeDb == null &&
    source != null &&
    output != null &&
    output.rate === source.rate &&
    output.bits >= source.bits &&
    output.channels === source.channels
  );
}

export function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

/** A length for meta lines: "54 min", "1 h 12 min". */
export function durationShort(ms: number): string {
  const minutes = Math.max(1, Math.round(ms / 60_000));
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return h ? `${h} h${m ? ` ${m} min` : ""}` : `${m} min`;
}

type TrackFormat = { codec: string; bits: number | null; rate: number };

/** How a track's format reads on a chip: "FLAC 24/96", "DSD64". */
export function formatLabel(t: TrackFormat): string {
  if (t.codec === "dsf" || t.codec === "dff") return `DSD${Math.round(t.rate / 44_100)}`;
  const khz = t.rate / 1000;
  return `${t.codec.toUpperCase()} ${t.bits ?? "?"}/${Number.isInteger(khz) ? khz : khz.toFixed(1)}`;
}

/** One chip for a set of tracks: their shared format, or the best of mixed ones. */
export function qualitySummary(tracks: TrackFormat[]): string | null {
  if (!tracks.length) return null;
  const labels = new Set(tracks.map(formatLabel));
  if (labels.size === 1) return [...labels][0];
  const best = [...tracks].sort((a, b) => (b.bits ?? 0) * b.rate - (a.bits ?? 0) * a.rate)[0];
  return `Mixed · up to ${formatLabel(best).split(" ").pop()}`;
}

export function isHiRes(t: TrackFormat): boolean {
  return (t.bits ?? 0) > 16 || t.rate > 48_000 || t.codec === "dsf" || t.codec === "dff";
}

/** A new array in random order (Fisher–Yates). */
export function shuffled<T>(items: T[]): T[] {
  const out = [...items];
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}
