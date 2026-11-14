---
title: Lossless quality check
date: 2026-11-14
summary: A track's ⋯ menu checks whether a lossless file is genuine, a transcode, fake hi-res or padded 24-bit, with evidence, charts, batches and cached badges
---

# Lossless quality check

## What happened
- The feature idea came from two reverse-engineered Android apps (Music Check, Synq). They are
  Flutter AOT builds with no recoverable source, so only criterion names, verdict words and UI
  sentences were available. The analyzer (`crates/sonder-quality`) is Sonder's own design.
- Synthetic fixtures (random-phase noise shaped per bin, overlap-added) passed on the first
  full run. Real albums did not: a genuine 24/96 piano album read 20–66 % "holes", because
  sparse treble and ultrasonic noise leave gaps above 12 kHz with content further up.

## Decision
- Holes count only in frames whose own cutoff sits in lossy territory (≤ 20.5 kHz); the Holes
  row cannot fail once content reaches 21.5 kHz. The piano album then read Genuine/High.
- The "noise floor" row was renamed quiet level and made info-only: the 5th-percentile frame
  RMS of a loud pop master is about −26 dBFS, which is no noise floor.
- A lossy lid outranks fake hi-res and fake bit depth in the verdict order.
- FLAC MD5 runs through symphonia's own verification (`Decoder::open_verifying`), and a pass
  never upgrades a verdict.
- The library caches a summary per path (about 1 KB); the full report with charts was about
  23 KB per track. Charts are recomputed when a report opens.
- Report types derive `specta::Type` behind a crate feature and are exported as `Quality*`,
  instead of mirroring them in DTOs (a bare `SourceInfo` clashed with an existing type).

## Open
- No MP3/AAC encoder on this machine: the thresholds have not met real 128–320 kbps
  transcodes yet (`docs/verification.md`, Quality check).
- The UI has not been clicked through in the running app.
