---
phase: 7
title: "Phase 7: PEQ, dither and levelling"
status: todo
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

- [ ] `Biquad`, `PeqStage` (per-channel band list, preamp), with filter types above.
- [ ] `LevelingStage` using DB ReplayGain values and peaks; R128 analysis job in `sonder-library` for untagged files.
- [ ] `DitherStage` (TPDF, none) + final quantiser to device bit depth.
- [ ] `VolumeStage` (software volume, dB, off by default).
- [ ] Settings and Tauri commands for each stage, plus the phase 6 SRC settings (filter source, Roon folder picker, phase, variant, target policy); PEQ presets saved to settings.

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

- [ ] Biquad + tests
- [ ] PEQ stage
- [ ] Levelling + R128 analysis
- [ ] Dither + quantiser + tests
- [ ] Software volume
- [ ] Pipeline order + UI

## Success Criteria

- Unit tests above pass; enabling then disabling all stages restores the bit-perfect path (byte-equality test still passes).
- Album mode levelling keeps relative track loudness inside an album.

## Risk Assessment

- Clipping from boost EQ: headroom stage plus clip counter surfaced in signal path.

## Security Considerations

- None new.

## Next Steps

- Phase 8.
