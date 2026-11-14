// Formatting for the Stats page and the sleep timer: chart labels, bar scale, countdowns.

import type { Locale } from "./i18n.svelte";

/** The BCP 47 tag dates are written in for an interface language. */
export function dateLocale(locale: Locale): string {
  return locale === "vi" ? "vi-VN" : "en-GB";
}

/**
 * A chart bar's label: a day (`YYYY-MM-DD`) as "14 Nov" / "14 thg 11", a month (`YYYY-MM`) as
 * "Nov 26" / "thg 11 26". Parsed as local dates, since the library groups by local time.
 */
export function barLabel(label: string, locale: Locale): string {
  const [y, m, d] = label.split("-").map(Number);
  if (!y || !m) return label;
  const date = new Date(y, m - 1, d || 1);
  const options: Intl.DateTimeFormatOptions = d
    ? { day: "numeric", month: "short" }
    : { month: "short", year: "2-digit" };
  return new Intl.DateTimeFormat(dateLocale(locale), options).format(date);
}

/** Bar heights as fractions of the tallest bar (0 when every bar is empty). */
export function barHeights(values: number[]): number[] {
  const top = Math.max(0, ...values);
  return values.map((v) => (top > 0 ? v / top : 0));
}

/** Which bars get a label under the chart: the first, the last and a few evenly between. */
export function labelledBars(count: number, max = 6): Set<number> {
  const shown = new Set<number>();
  if (count <= 0) return shown;
  if (count <= max) {
    for (let i = 0; i < count; i++) shown.add(i);
    return shown;
  }
  const step = (count - 1) / (max - 1);
  for (let k = 0; k < max; k++) shown.add(Math.round(k * step));
  return shown;
}

/** Whole minutes left until `endsAtMs`, rounded up, never below 0. */
export function minutesLeft(endsAtMs: number, nowMs: number): number {
  return Math.max(0, Math.ceil((endsAtMs - nowMs) / 60_000));
}
