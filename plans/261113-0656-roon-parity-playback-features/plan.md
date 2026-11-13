---
title: "Roon parity: SDM speed, convolution/crossfeed, repeat/crossfade, playlists, DAC verification"
description: "Close the four playback-side gaps against Roon found in the 2026-11-13 comparison and record real-DAC evidence."
status: pending
priority: P1
effort: "~4-5 weeks (solo)"
tags: [feature, audio, dsp, engine, library, frontend, verification]
created: 2026-11-13
---

# Roon parity: playback-side features

## Overview

Items 1–4 of the Sonder-vs-Roon comparison. Source contract (outcome, constraints, non-goals,
acceptance criteria, SDM trade-offs, user decisions):
[brainstorm report](../reports/brainstorm-261113-1354-roon-parity-playback-features.md).
Builds on the finished engine of [the lossless player plan](../261111-1242-sonder-lossless-player/plan.md).

## Goals

| # | Goal | Priority |
|---|------|----------|
| 1 | PCM→DSD256 order 7 at ≥ 1.5× realtime | P1 |
| 2 | Convolution (WAV IR) and bs2b-style crossfeed in the chain | P2 |
| 3 | Repeat off/all/one and crossfade (skipped inside album runs) | P1 |
| 4 | Playlists in the library and UI | P1 |
| 5 | Real-device evidence on the TP35 DAC in `docs/verification.md` | P1 |

## Phases

| # | Phase | Status |
|---|-------|--------|
| 1 | [SDM speed](./phase-01-sdm-speed.md) | Done |
| 2 | [Convolution and crossfeed](./phase-02-convolution-and-crossfeed.md) | Done |
| 3 | [Repeat and crossfade](./phase-03-repeat-and-crossfade.md) | Done |
| 4 | [Playlists](./phase-04-playlists.md) | Done (in-app walkthrough open) |
| 5 | [Real-device verification](./phase-05-real-device-verification.md) | Pending |

Order: 1, 2 and 4 are independent (different crates); 3 touches `player.rs` after 2 lands its
chain change there; 5 runs last so it also covers the faster SDM and the new stages.

## Invariants

- The audio thread stays real-time safe; new stages allocate only in `configure`.
- Every new option defaults to off; with all of them off the output stays byte-exact
  (existing `sonder-verify` tests plus new ones).
- The signal path names every new stage and every fallback.
- No Roon data in git or bundles.
- Commits: gitmoji + conventional commits, no AI references, no plan/phase IDs in code.

## Success Criteria

- [ ] Every acceptance criterion in the brainstorm report has passing evidence.
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `pnpm check` and `bash scripts/check-forbidden-files.sh` pass.
- [ ] `docs/verification.md` holds TP35 results; anything not run is marked so.

<!-- slug: roon-parity-playback-features -->
