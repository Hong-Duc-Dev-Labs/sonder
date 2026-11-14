// Presentation helpers for the lossless quality check: how a verdict reads and looks, how
// each criterion's measurement is written, and the scales the report's charts draw on.

import type {
  QualityCriterion,
  QualityEvidence,
  QualityEvidenceStatus,
  QualityVerdict,
} from "./bindings";
import type { MessageKey } from "./i18n.svelte";

/** The colour family a verdict or a status is drawn in. */
export type Tone = "ok" | "warn" | "bad" | "faint";

export function verdictTone(verdict: QualityVerdict): Tone {
  switch (verdict) {
    case "genuine":
      return "ok";
    case "suspect":
      return "warn";
    case "transcoded":
    case "fakeHiRes":
    case "fakeBitDepth":
      return "bad";
    case "inconclusive":
      return "faint";
  }
}

export function statusTone(status: QualityEvidenceStatus): Tone {
  switch (status) {
    case "pass":
      return "ok";
    case "warn":
      return "warn";
    case "fail":
      return "bad";
    case "info":
    case "notMeasured":
      return "faint";
  }
}

/** Whether a verdict is worth flagging in a track list (genuine and inconclusive are not). */
export function isFlagged(verdict: QualityVerdict): boolean {
  return verdictTone(verdict) === "warn" || verdictTone(verdict) === "bad";
}

export const verdictKey = (v: QualityVerdict) => `quality.verdict.${v}` as MessageKey;
export const verdictShortKey = (v: QualityVerdict) => `quality.short.${v}` as MessageKey;
export const criterionKey = (c: QualityCriterion) => `quality.criterion.${c}` as MessageKey;
export const statusKey = (s: QualityEvidenceStatus) => `quality.status.${s}` as MessageKey;

/** The sentence explaining a row, by criterion and status. */
export function reasonKey(e: QualityEvidence): MessageKey {
  const group = e.status === "fail" || e.status === "warn" ? e.status : e.status === "pass" ? "pass" : "other";
  return `quality.reason.${e.id}.${group}` as MessageKey;
}

const kHz = (hz: number) => `${(hz / 1000).toFixed(1)} kHz`;

/** The measured value of a row in its unit, or "" when the row carries none. */
export function evidenceValue(e: QualityEvidence): string {
  const v = e.value;
  if (v == null) return "";
  switch (e.id) {
    case "cutoff":
    case "stability":
    case "channelSymmetry":
      return kHz(v);
    case "holes":
      return `${Math.round(v * 100)} %`;
    case "bitDepth":
      return `${Math.round(v)} bit`;
    case "quietLevel":
      return `${v.toFixed(1)} dBFS`;
    default:
      return `${v.toFixed(1)} dB`;
  }
}

/** Horizontal position (0–1) of `hz` on a log axis from `minHz` to `maxHz`. */
export function logX(hz: number, minHz: number, maxHz: number): number {
  const x = Math.log(Math.max(hz, minHz) / minHz) / Math.log(maxHz / minHz);
  return Math.min(1, Math.max(0, x));
}

/** Frequency ticks for an axis up to `maxHz`: 100 Hz, 1 kHz, then every few kHz. */
export function frequencyTicks(maxHz: number): number[] {
  const candidates = [100, 1000, 5000, 10_000, 16_000, 20_000, 24_000, 48_000, 96_000];
  return candidates.filter((hz) => hz <= maxHz * 0.98);
}

/** Evenly spaced ticks for a linear axis up to `maxHz`, at most twelve of them. */
export function linearTicks(maxHz: number): number[] {
  const step = [1000, 2000, 4000, 8000, 16_000, 32_000].find((s) => maxHz / s <= 12) ?? 64_000;
  const ticks: number[] = [];
  for (let hz = step; hz < maxHz * 0.98; hz += step) ticks.push(hz);
  return ticks;
}

/** How many evidence rows fall under each status. */
export function tally(evidence: QualityEvidence[]): Record<QualityEvidenceStatus, number> {
  const counts: Record<QualityEvidenceStatus, number> = { pass: 0, warn: 0, fail: 0, info: 0, notMeasured: 0 };
  for (const e of evidence) counts[e.status]++;
  return counts;
}

export function tickLabel(hz: number): string {
  return hz >= 1000 ? `${hz / 1000}k` : `${hz}`;
}

/**
 * Colour of a spectrogram cell. `level` 0–255 maps −120…0 dB relative to the map's loudest
 * cell; the ramp runs black → violet → red → amber → pale yellow, readable in both themes
 * because the chart well stays dark.
 */
export function heat(level: number): [number, number, number] {
  const stops: [number, [number, number, number]][] = [
    [0, [0, 0, 4]],
    [0.35, [40, 11, 84]],
    [0.55, [120, 28, 109]],
    [0.72, [212, 72, 66]],
    [0.86, [249, 142, 9]],
    [1, [252, 255, 164]],
  ];
  const x = Math.min(1, Math.max(0, level / 255));
  for (let i = 1; i < stops.length; i++) {
    const [x1, c1] = stops[i];
    if (x <= x1) {
      const [x0, c0] = stops[i - 1];
      const f = (x - x0) / (x1 - x0);
      return [0, 1, 2].map((k) => Math.round(c0[k] + (c1[k] - c0[k]) * f)) as [number, number, number];
    }
  }
  return stops[stops.length - 1][1];
}
