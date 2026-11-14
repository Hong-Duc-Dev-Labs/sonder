---
title: "Lossless quality check: tell genuine lossless from transcodes, fake 24-bit and fake hi-res"
description: "Analyse a track's spectrum, real bit depth and FLAC MD5 to say whether a lossless file is genuine, with evidence, a spectrum/spectrogram view, batch checks and cached badges."
status: completed
priority: P2
effort: "~3 days"
tags: [analysis, library, dsp, ui, tauri, cli]
created: 2026-11-14
---

# Lossless quality check

## Outcome

From a track's ⋯ menu, **Check quality** opens a report: a verdict (Genuine lossless,
Suspect, Transcoded from lossy, Fake hi-res, Fake bit depth, Inconclusive), a confidence,
one evidence row per criterion (pass / warn / fail / not measured, each with a sentence that
says why), the average spectrum with the detected cutoff, and a spectrogram. An album, a
playlist or a multi-selection can be checked as a batch, one track at a time, each result
landing as it finishes. Results are cached in the library, so checked tracks show a small
quality badge in track lists. `sonder-cli check <files|folders>` prints the same report.

## Source and provenance

Behaviour reference: Music Check 1.0.0 (`com.verifymusic.app`) and the `lossless_check`
module of Synq 1.5.1, both proprietary Flutter AOT apps reverse-engineered at
`D:\Dev\reverse\{Music_Check_1.0.0,Synq_Offline_Music_1.5.1}`. **No source code exists or is
copied**: only the criterion names, verdict vocabulary and user-facing explanations were
recovered from string tables (`dart/libapp_strings*.txt`). The algorithms below are Sonder's
own, built on standard signal analysis. Do not copy their UI text verbatim; write Sonder's.

Recovered criteria (`lossless_analyzer.dart`): `_cutoffCriterion`, `_slopeCriterion`,
`_stabilityCriterion`, `_holesCriterion`, `_bitDepthCriterion`, `_noiseFloorCriterion`,
`_stereoFieldCriterion`, `_channelCriterion`, `_dsdCutoffCriterion`, `_dsdModulationCriterion`,
`_codecCriterion`, `_bitrateCriterion`; helpers `_median`, `_interquartileRange`, `_bandPower`,
`_channelDelta`. Verdicts: genuineLossless, lossy, fakeHiRes, fakeBitDepth, likely transcoded,
inconclusive; evidence status includes NOT MEASURED.

## Constraints

- Analysis is pure Rust in a new crate `crates/sonder-quality` (depends on `sonder-decode`,
  `sonder-dsp` for `DsdToPcm`, `realfft`), unit-tested with synthesized signals. No UI logic.
- Decoder output is left-justified `i32` (`crates/sonder-decode/src/lib.rs:345`): real bit
  depth comes from the samples themselves, exactly.
- FLAC MD5 uses symphonia's own verification (`DecoderOptions { verify: true }` +
  `finalize().verify_ok`, `symphonia-bundle-flac-0.6.1/src/decoder.rs:275`). A valid MD5
  proves integrity only, never that the source was lossless; the UI must say so.
- Cache lives in the library DB as migration `0005_quality.sql` (schema 5; the existing
  migrator backs up `library.db.bak-4` first). Keyed by path, invalidated by size + mtime +
  analyzer version.
- Webview keeps no file access; everything goes through Tauri commands; `bindings.ts` is
  regenerated with `SONDER_UPDATE_BINDINGS=1`, never hand-edited.
- Every new string in `en.ts` and `vi.ts` (typed; `pnpm check` fails on a missing key).
- Analysis never runs on the audio thread and never touches the engine; a batch runs one
  track at a time on a worker thread and is cancellable.

## Non-goals

Lossy formats (Sonder does not play MP3/AAC, so codec/bitrate criteria are dropped);
deleting or moving files flagged as fake; uploading anything; a quick/deep mode split (a full
track decodes and analyses in about a second on desktop, so every check is the full scan);
a continuous background pass over the whole library (a "Check all songs" batch is enough).

## Decisions (challenge phase)

| Decision | Source's way | Sonder's way |
|---|---|---|
| Decode | ffmpeg-kit | `sonder_decode::Decoder` (FLAC/ALAC/WAV/AIFF), `DsdToPcm` at 176.4 kHz for DSF/DFF |
| Quick vs deep | first seconds vs full track, deep is paid | full track always |
| Concurrency | one at a time ("two at once makes both slower") | one worker thread, cancellable |
| Storage | drift `verify_verdict` cache | `quality_checks` table, summary only (verdict + evidence, ~1 KB); charts are recomputed when the report opens |
| Verdict honesty | "Inconclusive is NOT a pass", lowered confidence at 21–22 kHz | same rules, own wording |

## Phases

| # | Phase | Depends on |
|---|---|---|
| 1 | [Analyzer crate](phase-01-analyzer.md) | — |
| 2 | [Cache, commands, CLI](phase-02-cache-commands-cli.md) | 1 |
| 3 | [UI and docs](phase-03-ui-docs.md) | 2 |

## Acceptance criteria

1. Synthesized fixtures give the expected verdict: full-band noise → Genuine; the same
   brick-walled at 16 / 19.5 / 20 kHz → Transcoded; 24-bit file whose low 8 bits are zero →
   Fake bit depth; 96 kHz file with nothing above 22 kHz → Fake hi-res; a wall at 21.5 kHz →
   Suspect with lowered confidence; near-silence or < 3 s → Inconclusive.
2. A FLAC with a corrupted frame reports MD5 FAIL; a FLAC without MD5 reports NOT MEASURED;
   neither changes a spectral verdict to Genuine.
3. Report shows verdict, confidence, every criterion row, spectrum with cutoff line and a
   spectrogram; Vietnamese and English both complete.
4. Batch check on an album shows progress per track, can be cancelled, and the badges appear
   without a rescan; a changed file loses its cached result.
5. `sonder-cli check` prints the verdict and criteria for files and folders, exit 0.
6. `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `pnpm check`, `pnpm test` pass.

## Rollback

Each phase is its own commit. Schema 5 adds one table only; reverting the code leaves an
unused table (the migrator refuses a newer DB, so a downgrade restores `library.db.bak-4`).

## Implementation notes (2026-11-14)

All three phases are done; acceptance rows 1–6 are covered by `cargo test -p sonder-quality`,
`cargo test -p sonder-library --test quality`, `cargo test --workspace`, clippy, `pnpm check` and
`pnpm test`. Changes from the plan text, decided during implementation:

- Verdict precedence: a lossy lid outranks fake hi-res and fake bit depth (an MP3 upsampled to
  96/24 is first of all a transcode).
- "Noise floor" became **quiet level** (RMS of the quietest 5 % of frames, info only): on real
  pop masters it read −26 dBFS, which is not a noise floor.
- Holes are only counted in frames whose own cutoff is ≤ 20.5 kHz: the first build flagged a
  genuine 24/96 piano album (sparse treble, ultrasonic noise) at 20–66 % holes.
- The cutoff track has one value per spectrogram column (≤ 400), not per second.
- The cache stores no charts (the full report was ~23 KB per track, ~230 MB per 10k tracks).
- Cancel is split: closing a report stops only its own check; the sidebar cancels the batch.
- Report types derive `specta::Type` behind a `specta` feature of `sonder-quality`, exported as
  `Quality*` names, instead of duplicate DTOs.
- Not done: a run on real MP3/AAC → FLAC transcodes (no encoder on this machine), and a
  click-through of the UI in the running app.
