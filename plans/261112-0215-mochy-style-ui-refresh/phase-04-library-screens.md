---
phase: 4
title: "Phase 4: Library screens"
status: in-progress
priority: P1
effort: "1d"
dependencies: [2]
---

# Phase 4: Library screens

## Requirements

- [x] **Page wash**: album and artist pages set `--wash` from their cover's seed colour, so
      the sidebar and pill take the page's hue while it is open.
- [x] **Album page**: full-width hero — the cover large and sharp, a blurred enlargement of
      it behind, melting into black through `.frost-fade`; display title in white, artist
      link, "2019 • 12 tracks • 54 min • Genre", quality chip (`FLAC 24/96` or
      `Mixed · up to …`); solid white Play pill, glass Shuffle pill, round glass add-to-queue;
      a record peeking from behind the cover; keeps `view-transition-name` on the cover;
      stacks centred when narrow; skeleton rows while loading.
- [x] **Track list**: rows on the black canvas grouped in a soft glass card (tint only);
      number turns into a play glyph on hover or focus; click on the glyph, double-click, or
      Enter (click with `detail 0`) plays; quality badge only where it differs from the
      album; spinning mini disc on the playing row; ⋯ opens the glass menu (Play next, Add to
      queue, Go to album).
- [x] **Album card**: a real `<button>` with Play as a sibling; focus reveals Play (round
      glass over the cover); record slides out of the sleeve on hover; year on the artist
      line; cover radius 16; `view-transition-name` only on the clicked card.
- [x] **Albums header**: large "Albums" title with the count, sort as glass chips.
- [x] **Artist page**: hero from a blurred mosaic of the artist's covers melting into black,
      vinyl avatar, very large name, counts, Play all / Shuffle.
- [x] **Artists list**: vinyl avatars from `Artist.art`, pill filter field, empty states.
- [x] **Search**: large title, glass pill field with icon, pending state (no false
      "No matches"), Albums row then Tracks, empty-query hint with a `Ctrl F` keycap.
- [x] **Cover placeholder** drawn as a vinyl label.

## Success Criteria

- No `backdrop-filter` on cards or rows (only chrome and the hero frost use it).
- White text over a hero with a white cover keeps ≥ 4.5:1.
- Album grid stays virtualised; scroll trace holds 60 fps under the glass.
