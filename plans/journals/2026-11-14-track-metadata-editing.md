---
title: Track metadata editing
date: 2026-11-14
summary: A track's ⋯ menu edits its tags and saves them into the file itself, for FLAC, ALAC, WAV, AIFF, DSF and DFF
---

# Track metadata editing

## What happened
- The library read title, artists, album, album artists, track/disc number, year, genre, cover,
  lyrics and ReplayGain, and the UI showed them; nothing could write tags back, so a wrong title
  or missing year in a file stayed wrong.

## Decision
- `TagEdit` holds only the fields the editor owns. `write_file` replaces those and leaves every
  other tag alone: cover art, lyrics, ReplayGain, comments and format-specific frames.
- FLAC, ALAC/MP4, WAV and AIFF go through lofty. Every writable tag in the file gets the edit,
  not only the primary one (Vorbis comments, ID3v2, MP4 ilst): the reader falls back to the
  others, so a WAV's RIFF INFO or an AIFF's text chunks brought cleared values back. A file
  without a primary tag gets a new one of its format's primary type.
- Each artist name is its own value (repeated Vorbis field, repeated MP4 data atom, NUL-separated
  ID3v2.4 text), which the reader splits back into names. Joining them with "; " in one value
  made the library index a single artist called "A; B". Only single-text formats (RIFF INFO,
  AIFF text, ID3v1) still get the joined form.
- Both `TrackArtist` and `TrackArtists` are cleared before names are written. Files tagged by
  other tools carry the credit twice (TPE1 and `TXXX:ARTISTS`), and the reader prefers the
  second, so clearing only TPE1 left the old name in place for ever.
- DSF/DFF tags are parsed as a full `Id3v2Tag`, split into the editable fields and the rest,
  edited and merged back. Going through lofty's generic `Tag` dropped every frame it has no item
  for (PRIV, GEOB, POPM ratings, chapters, synced lyrics).
- DSF: the ID3v2 tag behind the sound data is rebuilt that way, then the header's total length
  and metadata pointer are updated.
- DFF: the tag is replaced where the file keeps it — an `ID3 ` chunk inside FRM8 (FRM8 size
  recomputed), or the chunk or bare tag that sacd_extract appends after the form (rewritten in
  the same style, FRM8 untouched). Data after the form that is not a tag is preserved, and the
  new chunk goes inside the form so the reader finds it. Both spellings of the appended tag are
  now read, which also indexes tags of files written by other tools. An odd last chunk that
  lacks its pad byte counts as ending at the form's end: the reader accepted it, the writer did
  not, so the edit went into a second tag the reader never looked at.
- Every write, lofty's included, rebuilds the file in a temporary file beside it and renames it
  over the original, so a failed write leaves the original as it was. lofty's own `save_to_path`
  writes in place, so it is handed a copy instead. The original is first opened for writing, so
  a file the user may not change is refused rather than replaced (a rename only needs the folder
  to be writable), and the new file takes the original's permissions (a temporary file is
  `0600` on Unix). The source handle is closed before the rename because Windows refuses to
  replace an open file.
- `Library::reindex_track(id)` re-reads one file and updates its row even when `(mtime, size)`
  look unchanged — a tag write can keep both. The track's measured loudness is restored because
  the audio did not change; a track that moved to another album has its album-level values
  invalidated for the next analysis pass.
- Tauri: `track_tags` reads the file fresh for the editor through `read_file_tags`, which skips
  the track number the library guesses from the file name, so Save cannot write a guess the user
  never saw come from the file; a missing title is still filled, as a track needs one.
  `track_update_tags` writes on a
  blocking thread, re-indexes, rewrites the matching queue entries so Now Playing and the queue
  show the new title, emits `LibraryChangedEvent` and starts a loudness pass.
- UI: `TrackEditDialog` is a native `<dialog>` opened from every track's ⋯ menu. Text fields
  for title, artists, album, album artists, year and genre, number boxes for track and disc
  number; several names are typed separated by semicolons. `lib/tagEdit.ts` holds the field
  parsing so vitest covers it.

## Evidence
- `cargo test -p sonder-library`: WAV (hound fixture), DSF and DFF round trips, tag clearing, the
  second artist spelling, and a reindex that moves a track to a new album; `cargo clippy
  --workspace --all-targets -- -D warnings` and `cargo fmt --check` are clean.
- Real copies through a throwaway binary (since deleted): a 54 MB FLAC and a 275 MB DSD128 DSF
  kept their decoded PCM / raw audio bytes identical through two writes, and kept cover art and
  ReplayGain. The DSF needed the two-key artist clearing to drop its old `ARTISTS` frame.
- In the running app (debug build against the Vite dev server, driven through WebView2's
  remote debugging port): the Songs page listed the real library, ⋯ → Edit metadata filled the
  dialog from a real FLAC's tags, Save was disabled on a blank title, and Cancel wrote nothing.
  Saving a edited title on a disposable copy placed inside the library updated the file (reopening
  the editor, which reads the file, showed the new title), the index row and the Songs row;
  deleting the copy and rescanning removed its track and album rows. Every temporary file, the
  launcher script and the throwaway binaries were deleted afterwards.
- `pnpm check`, `pnpm test` and `pnpm build`, plus `cargo test -p sonder-app` with the
  regenerated `bindings.ts`.
- After review, regression tests that failed before their fix: two artists round-trip on WAV
  and DSF, a cleared artist and genre stay cleared next to RIFF INFO, a DFF PRIV frame survives a
  write, an unpadded odd last `ID3 ` chunk is replaced instead of shadowed, and the editor's read
  carries no file-name track number. Also added: a read-only file is refused and left byte-for-
  byte alone, a lofty write leaves no temporary file, and (Unix only, not run here) a rewritten
  file keeps mode `0644`. The real-file and in-app checks above were not repeated.

## Next steps
- Cover art and lyrics are not editable here yet.
- A DFF that carries two tags keeps only the inside one after a write; a file with a strange
  chunk layout is left alone and nothing is written.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
