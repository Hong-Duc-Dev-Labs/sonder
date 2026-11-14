---
title: Cover, lyrics and online lookup in the metadata editor
date: 2026-11-14
summary: The tag editor now edits the embedded cover and lyrics, and can fill its fields from iTunes, Deezer and MusicBrainz or fetch lyrics online
---

# Cover, lyrics and online lookup in the metadata editor

## What happened
- The editor covered title, artists, album, album artists, numbers, year and genre only. Covers
  and lyrics could not be changed, and filling in a badly tagged file meant typing everything.

## Decision
- `TagEdit` gained `lyrics: Option<String>` (`None` keeps the file's lyrics) and
  `cover: CoverEdit { Keep, Remove, Set(bytes) }`. Both default to "leave alone", so a plain
  field edit never rewrites a picture or a second-language lyrics frame.
- `Set` puts the new front cover first and keeps the other pictures; `Remove` removes every
  picture, because the reader falls back to any picture and a leftover one would still show.
  Only JPEG and PNG are accepted: those are what every tag format takes.
- lofty maps ID3v2 `USLT` to `ItemKey::UnsyncLyrics`, never `ItemKey::Lyrics`. Writes try
  `Lyrics` and fall back to `UnsyncLyrics`; the reader reads both. Before this the reader missed
  lyrics in every ID3v2 tag (WAV, AIFF, DSF, DFF).
- New crate `sonder-metadata`, shaped like `sonder-lyrics`: iTunes, Deezer and MusicBrainz are
  asked in parallel, replies are parsed apart from the requests so they test offline, and
  candidates whose length matches the track sort first. Deezer's search lacks numbers, year and
  genre, so its first three candidates get `/track` and `/album` requests. MusicBrainz is paced
  to one request per 1.1 s: a second search inside a second got HTTP 503.
- Covers download only from `mzstatic.com`, `dzcdn.net` and `coverartarchive.org` over HTTPS,
  through the backend; the webview CSP stays closed to remote images, and thumbnails come back
  as `data:` URLs.
- lofty 0.25.5's `replace_range` fast path (an edit that ends at EOF) truncates to
  `old_len - new_len` instead of `start + new_len`. Only its FLAC writer splices a `File` that
  way, and only a FLAC with no audio frames has metadata at EOF, so real files are safe; a
  test fixture without frames lost everything when its cover was removed. `write_lofty` now
  reads the rewritten copy back before it replaces the original, so a writer bug like this
  becomes an error with the original untouched.
- Online lookups in the editor run only on a click, so they ignore the "online lyrics" setting;
  the buttons' tooltips and the README name what is sent where.

## Verification
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`: pass.
- New tests: FLAC cover/lyrics set and removed with the index following; a rewrite that does
  not read back leaves the original byte for byte; WAV and DSF cover/lyrics set → kept by a later edit → replaced → removed; non-image
  cover refused; LRC format/parse round trip; offline replies of all three catalogues; cover
  host allow-list. `cargo test -p sonder-metadata -- --ignored` hit the live services and
  downloaded a cover.
- `pnpm check` and `vitest`: pass (form helpers covered).
- Not yet done: clicking through the dialog in the running app.
