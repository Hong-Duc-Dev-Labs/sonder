---
phase: 1
title: "Phase 1: SDM speed"
status: completed
priority: P1
effort: "3-4d"
dependencies: []
---

# Phase 1: SDM speed

## Overview

Make `sonder-dsp::Sdm` fast enough for DSD256 order 7 with headroom, without changing its
output quality. Chosen approach A (single-thread loop rework); approach B (one thread per
channel) only if A misses the target after measurement.

## Key Insights

- Today (`cargo bench -p sonder-dsp --bench sdm`): DSD256/5 = 1.02 s, DSD256/7 = 1.19 s per
  second of stereo, i.e. ~44–52 ns per bit per channel. A 5th/7th-order CIFF update is a few
  ns of arithmetic, so the time is overhead.
- Overheads visible in `crates/sonder-dsp/src/sdm.rs`:
  - `step()` (`:399`) takes `order` at runtime and iterates slices with bounds checks;
  - every bit checks `is_finite()`/`abs() > STATE_LIMIT` over the whole state;
  - every bit indexes `self.scratch[...]` with a multiply/divide to set one bit;
  - the interpolator ring uses `% PHASE_TAPS` per tap and the two channels run one after the
    other, so the serial loop of one channel never overlaps the other's.
- The loop is inherently serial per channel; independent channels give instruction-level
  parallelism when interleaved.

## Requirements

- [x] Order as a const generic (`step::<5>`, `step::<7>`), state as fixed arrays.
- [x] Build each output byte in a register, write once per 8 bits per channel.
- [x] Stability check once per output byte (or per block) instead of per bit, keeping the
      same reset semantics: a state above `STATE_LIMIT` or non-finite resets and reports.
- [x] Interpolator history as a doubled linear buffer (no modulo in the tap loop).
- [x] Process two channels interleaved in the bit loop (stereo fast path; generic path kept
      for other counts).
- [x] Bench target: DSD256 order 7 ≤ 0.66 s per second of stereo on this machine.
- [x] If A misses the target: per-channel worker threads inside `Sdm` with blocks handed over
      at block granularity (decode worker, never the RT thread). Decide only from the bench.
      — Not needed: A met the target on an E-core (318 ms).

## Related Code Files

- Modify: `crates/sonder-dsp/src/sdm.rs`, `crates/sonder-dsp/benches/sdm.rs` (if a stereo
  fast-path case needs its own bench), `docs/verification.md` (Measured results table),
  `README.md` (the DSD256 CPU note), `app/src/components/SettingsView.svelte` (DSD256 warning
  text, only if the numbers change the advice).

## Implementation Steps

1. Record a baseline bench run (same command, release) before touching code.
2. Add a bit-exact reference test: keep the current `process` as a `#[cfg(test)]` reference
   and assert the new implementation emits identical bytes for random input, full-scale
   square waves (to trigger resets) and silence, at all rate/order pairs. Identical output
   is possible because the arithmetic order stays the same; if a change alters rounding,
   relax to the existing SNR/round-trip tests and say so in the commit.
3. Apply the rework one item at a time, benchmarking after each; keep changes that move
   the number.
4. Re-run the bench three times; report median and spread per `ak-test` explain-the-number.
5. Update the measured table and DSD256 guidance.

## Todo

- [x] Baseline bench recorded
- [x] Reference-equality test
- [x] Const-generic order + register bytes + cheaper stability check
- [x] Linear interpolator history
- [x] Interleaved stereo path
- [x] Bench meets target (or approach B added and measured)
- [x] Docs updated with new numbers

## Success Criteria

- `cargo bench -p sonder-dsp --bench sdm`: DSD256/7 ≤ 0.66 s per second of stereo (median
  of three runs), DSD256/5 not slower than DSD256/7.
- All existing SDM tests (SNR, round-trip, 0 dBFS = 50 %, reset) pass unchanged, plus the
  reference-equality test.
- `Sdm::process` still does not allocate for blocks up to `MAX_FRAMES`.

**Implementation note.** The baseline premise was half right: the 1.19 s figure in
`docs/verification.md` was a slow-state run. The same code measured 340 ms unpinned in its
fast state, 450 ms pinned to a P-core and 869 ms pinned to an E-core (i5-12450HX). The E-core
figure is now the planning number. The rework (const-generic order, coefficients padded to
fixed arrays, doubled interpolator history, channel state copied to locals for the block,
bytes built in a register, channels run in interleaved pairs, one `!(abs <= limit)` compare
per state) is byte-identical to the old loop, including stability resets. A test proves this
against a copy of the old form, with NaN and near-limit state injected mid-stream. DSD256/7:
225 ms P-core, 318 ms E-core, 174–379 ms unpinned. New `pcm_to_dsd/*` bench (44.1k → resampler →
SDM): DSD256/7 371 ms P-core, 632 ms E-core, 705 ms unpinned in the slow state, so ≥ 1.4×
realtime. The resampler now dominates that path. Approach B was not needed.

## Risk Assessment

- End-to-end may still underrun if the upstream resampler to `DSD rate / 8` dominates:
  measure the full chain in phase 5 via the transport underrun counter; if it does, record it
  rather than widening this phase.
- Interleaving changes code shape but not arithmetic order per channel; the equality test
  guards that.
