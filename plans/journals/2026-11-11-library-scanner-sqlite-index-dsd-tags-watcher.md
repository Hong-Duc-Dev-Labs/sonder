---
title: "Library scanner: SQLite index, DSD tags, watcher"
date: 2026-11-11
summary: "sonder-library crate built; lofty lacks DSD, macOS watcher path aliasing fixed; 10k tracks scan in 3.9 s, rescan 85 ms"
---

# Library scanner: SQLite index, DSD tags, watcher

## What happened
Implemented the library scanner on branch `feat/library-scanner`: a new `sonder-library` crate
(SQLite schema with versioned migrations and backup-before-migrate, tag reading, album grouping,
cover thumbnail cache, incremental scanner, file watcher, browse/search queries) and a
`sonder-cli scan` command for timing real libraries. 27 tests pass on Windows (unit, mixed-format
fixture tree, incremental delta, roots, watcher); clippy `-D warnings` is clean.

## Findings
- lofty 0.25 supports neither DSF nor DFF (the plan assumed DSF worked). DSF/DFF headers are
  parsed by hand, and their ID3v2 blob goes through lofty's MPEG reader with property reading
  off, the only public entry to its ID3v2 parser, so every format shares one tag mapping.
- macOS FSEvents reports `/private/var/...` for a root added as `/var/...`; without mapping event
  paths back to the root spelling, every watcher event would be dropped.
- Deferred SQLite transactions that read before writing can fail with BUSY_SNAPSHOT when two
  connections write; write transactions are now `IMMEDIATE`.
- An unplugged drive must never look like "all files deleted": missing roots and unreadable
  folders keep their tracks.

## Measurements
10 000 generated header-only FLAC tracks (1 000 albums, distinct 600 px covers), Windows 11,
release build: full scan 3.86 s, no-change rescan 85 ms, album list 4 ms, search 2 ms.

## Next steps
- Run CI on macOS/Linux (the watcher test is the most platform-sensitive).
- Time `sonder-cli scan` on a real ~10k-track library and record it in the phase file.
- Phase 4 (Tauri UI) consumes the query API.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
