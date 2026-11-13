---
phase: 7
title: "Phase 7: PEQ, dither and levelling"
status: in progress
priority: P2
effort: "1w"
dependencies: [3, 4, 6]
---

# Phase 7: PEQ, dither and levelling

## Overview

Add the remaining PCM DSP stages: parametric EQ (biquad cascade), volume levelling from
ReplayGain tags, bit-depth reduction with TPDF dither, and optional software volume.

## Key Insights

- Biquads follow the RBJ Audio EQ Cookbook (public): peaking, low/high shelf, low/high
  pass, band-pass, notch; process in f64 Transposed Direct Form II.
- Levelling: ReplayGain track/album gain with peak-based limiting to avoid clipping; mode
  = off | track | album | auto (album when playing an album in order). Analysing files
  without tags (EBU R128 via `ebur128` crate) runs in the library scanner as a background job.
- Dither only when the output word length is shorter than the processed signal: TPDF at
  the target LSB, applied once at the end of the chain.

## Requirements

- [x] `Biquad`, `PeqStage` (per-channel band list, preamp), with filter types above.
- [x] `LevelingStage` using DB ReplayGain values and peaks; R128 analysis job in `sonder-library` for untagged files.
- [x] `DitherStage` (TPDF, none) + final quantiser to device bit depth.
- [x] `VolumeStage` (software volume, dB, off by default).
- [x] Settings and Tauri commands for each stage, plus the phase 6 SRC settings (filter source, Roon folder picker, phase, variant, target policy); PEQ presets saved to settings.

## Related Code Files

- Create: `crates/sonder-dsp/src/{biquad.rs,peq.rs,leveling.rs,dither.rs,volume.rs}`, `crates/sonder-library/src/loudness.rs`.
- Modify: `crates/sonder-engine/src/pipeline.rs`, `app/src-tauri/src/commands.rs`, UI settings screen.

## Implementation Steps

1. Biquad + coefficient tests against cookbook reference values; frequency response tests (gain at f0 equals requested within 0.01 dB).
2. PEQ stage; levelling stage; dither + quantiser (test: dithered 24→16 of silence has expected TPDF noise floor; no dither when not reducing).
3. R128 analysis job and DB columns (migration with backup).
4. Wire stages in Roon order: headroom → SRC → PEQ → levelling → volume → dither/quantise.
5. UI controls.

## Todo

- [x] Biquad + tests
- [x] PEQ stage
- [x] Levelling + R128 analysis
- [x] Dither + quantiser + tests
- [x] Software volume
- [x] Pipeline order + UI

## Success Criteria

- [x] Unit tests above pass; enabling then disabling all stages restores the bit-perfect path
  (`removing_every_dsp_stage_restores_the_bit_perfect_path`).
- [x] Album mode levelling keeps relative track loudness inside an album (the measured album
  value is a duration-weighted energy average; track gains keep their LUFS difference).

**Implementation note.** The EQ is a per-channel cascade of RBJ biquads in transposed direct
form II; tests check gain at f0, shelf asymptotes, passband/stopband and per-channel state.
R128 is implemented in `sonder-library` rather than added as a dependency: K-weighting uses the
BS.1770 analog prototypes at the stream's own rate, 400 ms blocks over 100 ms hops, −70 LUFS
absolute and −10 LU relative gates, 5.1 LFE excluded and surrounds weighted +1.5 dB; channels
past the eighth count towards the peak only. The analysis job groups pending PCM
tracks by album, writes `loudness_lufs/loudness_peak` (migration 0003, with the usual backup)
and derives `album_lufs/album_peak` as a duration-weighted energy average, so mixed-rate albums
and partial rescans still produce a usable album value; DSD is skipped. Silent or sub-block
tracks store a placeholder below the absolute gate that yields no gain and is left out of the
album average. A rescan clears the measurement of a changed file and the album values of every
album it touched; the next pass re-measures and rebuilds them. Passes run on their own queued
worker (one at a time), started by scans, the folder watcher and the Settings button, with
progress and finished events; decoding happens outside the write transaction and a
measurement is written only if the file was not re-indexed meanwhile. Levelling converts DSD
to PCM only when that track has a non-zero gain; otherwise DoP/native DSD stay untouched. Levelling reads tags first, shifted from the ReplayGain −18 LUFS
reference to the chosen target, and the measurement second, caps a boost at the track peak, and publishes its gain through
an atomic so a gapless next track changes gain without rebuilding the chain. `Auto` uses album
values while the queue holds a contiguous run of the album. Dither (2 LSB peak-to-peak TPDF,
RMS 0.5 LSB) is inserted only when the chain shapes the signal and the device's widest container
is narrower than the processed 24 bits, and the output spec then requests exactly that width so
the 16-bit fallback is dithered rather than truncated. The pipeline order is headroom → SRC →
PEQ → levelling → volume → dither. Clipping in the final conversion, or clamped by the dither
quantiser, is counted and reported once
per track (`PlayerEvent::Clipping`) and shown as a notice. Settings carry the EQ (with named
presets), levelling mode and target, dither, and every phase 6 SRC option; the UI edits them on
the Settings screen.

## Risk Assessment

- Clipping from boost EQ: headroom stage plus clip counter surfaced in signal path.

## Security Considerations

- None new.

## Next Steps

- Phase 8.
