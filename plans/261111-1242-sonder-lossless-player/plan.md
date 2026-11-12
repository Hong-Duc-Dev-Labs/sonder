---
title: "Sonder — lossless music player in Rust"
description: "Build a local desktop player in Rust + Tauri that plays PCM and DSD bit-perfect on Windows, macOS and Linux, with a Roon-style DSP chain."
status: pending
priority: P1
effort: "~9-12 weeks (solo)"
tags: [feature, backend, frontend, audio, dsp]
blockedBy: []
blocks: []
created: 2026-11-11
---

# Sonder — lossless music player in Rust

## Overview

Sonder scans a local music library, shows it in an album-art-first Tauri UI, and plays
FLAC/ALAC/WAV/AIFF/DSF/DFF bit-perfect through exclusive-mode output on all three desktop
OSes. An optional DSP chain mirrors the order recovered from Roon 2.65: headroom, SRC,
PEQ, levelling, PCM→DSD, dither. Source contract:
[brainstorm report](../reports/brainstorm-261111-1933-sonder-lossless-player.md).

## Goals

| # | Goal | Priority |
|---|------|----------|
| 1 | Provable bit-perfect playback (DSP off) on WASAPI exclusive, CoreAudio hog, ALSA `hw:` | P1 |
| 2 | Gapless queue with correct device rate switching | P1 |
| 3 | Library scan + browse by album/artist/track with cover art | P1 |
| 4 | DSD playback via DoP and DSD→PCM | P2 |
| 5 | Polyphase upsampling with `FilterSource` = Sonder-designed or Roon tables (runtime path) | P2 |
| 6 | PEQ, ReplayGain/levelling, TPDF dither; PCM→DSD (SDM); signal-path UI | P2 |

## Phases

| # | Phase | Status |
|---|-------|--------|
| 1 | [Workspace and WASAPI exclusive spike](./phase-01-workspace-and-wasapi-spike.md) | In progress (manual DAC test open) |
| 2 | [Engine, gapless and OS backends](./phase-02-engine-gapless-and-os-backends.md) | In progress (code + tests done; manual DAC and CI run open) |
| 3 | [Library scanner](./phase-03-library-scanner.md) | Done |
| 4 | [Tauri UI](./phase-04-tauri-ui.md) | Done (PR #4; CI green on all three OSes; scripted DAC click-through not recorded) |
| 5 | [DSD playback](./phase-05-dsd-playback.md) | In progress (code + tests done; manual DAC test open) |
| 6 | [Resampler and filter sources](./phase-06-resampler-and-filter-sources.md) | Pending |
| 7 | [PEQ, dither and levelling](./phase-07-peq-dither-leveling.md) | Pending |
| 8 | [SDM, signal path and verification](./phase-08-sdm-signal-path-verification.md) | Pending |

Order: 1 → 2 → (3 ∥ 6) → 4 needs 2+3 → 5 needs 2+6 (shares the FIR core) → 7 needs 3+4+6 → 8 needs 4+5+6+7.

## Invariants (all phases)

- Render callback is real-time safe: no allocation, locks, I/O or logging on the audio thread.
- DSP off ⇒ output samples equal decoded samples (tested, not assumed).
- Roon filter tables and decompiled Roon code never enter git or a release bundle;
  `.gitignore` blocks `polyphase_*`, `*.csv`, `*.decrypted`, and a CI guard checks
  `git ls-files` from phase 1.
- Keep-open/gapless decisions use the output format after DSP, not the source format.
- When the device cannot play a format, Sonder refuses (phases 1–5) or falls back to SRC
  labelled "not bit-perfect" (phase 6 on); it never silently resamples.
- DoP streams never emit zero frames: underrun, pause and stop use DoP idle silence.
- Commits: gitmoji + conventional commits, no AI references.

## Success Criteria

- [ ] All 8 acceptance criteria in the brainstorm report pass.
- [ ] `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` pass on Windows; CI runs them on macOS and Linux.
- [ ] Bit-perfect harness (phase 8) passes on at least one real DAC per OS available to the user.

## Dependencies

- Rust 1.98 stable, Node LTS + pnpm for the UI, Tauri 2 CLI.
- Hardware: a USB DAC (ideally DoP-capable) for phases 1, 2, 5, 8.
