# Phase 2 — Cache, Tauri commands, CLI

## Files

- Create `crates/sonder-library/src/migrations/0005_quality.sql`, `crates/sonder-library/src/quality.rs`
- Modify `crates/sonder-library/src/{schema.rs,lib.rs,query.rs}` (join verdict into track rows)
- Create `app/src-tauri/src/quality.rs`; modify `commands.rs`, `events.rs`, `main.rs` (register),
  `dto.rs`; regenerate `app/src/lib/bindings.ts`
- Modify `crates/sonder-cli/src/main.rs` (`Check` subcommand)

## Schema 5

```sql
CREATE TABLE quality_checks (
  path TEXT PRIMARY KEY,
  size INTEGER NOT NULL, mtime INTEGER NOT NULL,
  analyzer_version INTEGER NOT NULL,
  verdict TEXT NOT NULL, confidence TEXT NOT NULL,
  report_json TEXT NOT NULL,          -- Report without charts
  checked_at INTEGER NOT NULL
);
```

A row is valid only while size, mtime and `analyzer_version` match; stale rows read as
"not checked". Keyed by path (like playlists) so rescans keep it.

## Commands (follow `library_analyze_loudness` / `loudness.rs` pattern)

- `quality_check_track(track_id) -> QualityReportDto` — full report incl. spectrogram; runs on
  a blocking thread, emits `QualityProgress { track_id, fraction }`; saves the summary.
- `quality_cached(track_id) -> Option<QualityReportDto>` — instant reopen without charts.
- `quality_check_batch(track_ids)` — one worker, one at a time; emits `QualityProgress` and
  `QualityChecked { track_id, verdict, confidence }` per track, `QualityBatchEnded { cancelled }`.
  A second batch while one runs appends unseen ids (no second worker).
- `quality_cancel()`.
- Track DTOs gain `quality: Option<{ verdict, confidence }>`.

## CLI

`sonder-cli check <paths…> [--json]`: folders expand with `AUDIO_EXTENSIONS` plus dsf/dff;
prints `verdict (confidence)` and one line per criterion; `--json` prints the `Report`.

## Validation

`cargo test -p sonder-library` (migration 4→5 backs up and preserves rows; stale-row rule),
`cargo test --workspace`, clippy, `SONDER_UPDATE_BINDINGS=1` regeneration, then run
`cargo run -p sonder-cli -- check <a real album>`.
