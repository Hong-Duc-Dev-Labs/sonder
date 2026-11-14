// The metadata editor's field parsing: what the user types becomes what the file's tags get.

import type { CoverImage, MetadataCandidate, TrackEdit } from "./bindings";

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

/** The editor's text boxes. Numbers stay text, so an empty box clears the tag. */
export interface TagForm {
  title: string;
  artists: string;
  album: string;
  albumArtists: string;
  trackNo: string;
  discNo: string;
  year: string;
  genre: string;
}

/** The boxes as the file's tags fill them. */
export function formFromTags(tags: TrackEdit): TagForm {
  return {
    title: tags.title,
    artists: tags.artists.join("; "),
    album: tags.album,
    albumArtists: tags.albumArtists.join("; "),
    trackNo: tags.trackNo?.toString() ?? "",
    discNo: tags.discNo?.toString() ?? "",
    year: tags.year?.toString() ?? "",
    genre: tags.genre,
  };
}

/**
 * The boxes after taking what an online catalogue knows. A field the catalogue leaves empty
 * keeps what the user has: a catalogue with no genre should not wipe the one typed in.
 */
export function applyCandidate(form: TagForm, found: MetadataCandidate): TagForm {
  const text = (value: string, kept: string) => (value.trim() ? value.trim() : kept);
  const names = (value: string[], kept: string) => (value.length ? value.join("; ") : kept);
  const number = (value: number | null, kept: string) => (value ? value.toString() : kept);
  return {
    title: text(found.title, form.title),
    artists: names(found.artists, form.artists),
    album: text(found.album, form.album),
    albumArtists: names(found.albumArtists, form.albumArtists),
    trackNo: number(found.trackNo, form.trackNo),
    discNo: number(found.discNo, form.discNo),
    year: number(found.year, form.year),
    genre: text(found.genre, form.genre),
  };
}

/** Covers larger than this are refused before they are read. */
export const MAX_COVER_BYTES = 20 * 1024 * 1024;

/** A JPEG or PNG `data:` URL as the image it holds, `null` for anything else. */
export function coverFromDataUrl(url: string): CoverImage | null {
  const match = /^data:(image\/(?:jpeg|png));base64,(.+)$/.exec(url);
  return match ? { mime: match[1], data: match[2] } : null;
}

/** An image as a URL an `<img>` can show. */
export function coverDataUrl(image: CoverImage): string {
  return `data:${image.mime};base64,${image.data}`;
}
