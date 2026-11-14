// The metadata editor's field parsing: what the user types becomes what the file's tags get.

/** Splits a field holding several names on the separator the library itself joins with. */
export function splitNames(line: string): string[] {
  return line
    .split(";")
    .map((name) => name.trim())
    .filter(Boolean);
}

/**
 * A number box's value. Empty or unusable text means "no value", which clears the tag; an
 * out-of-range number is brought inside what the tag formats hold.
 */
export function numberField(text: string): number | null {
  const value = Number.parseInt(text.trim(), 10);
  return Number.isFinite(value) && value > 0 ? Math.min(value, 9999) : null;
}
