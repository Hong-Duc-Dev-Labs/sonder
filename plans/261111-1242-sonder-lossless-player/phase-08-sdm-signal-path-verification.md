---
phase: 8
title: "Phase 8: SDM, signal path and verification"
status: todo
priority: P2
effort: "1.5w"
dependencies: [4, 5, 6, 7]
---

# Phase 8: SDM, signal path and verification

## Overview

Add PCM→DSD (sigma-delta modulation), a signal-path panel in the UI, and a verification
suite that proves every acceptance criterion from the brainstorm report.

## Key Insights

- SDM: 5th and 7th order CIFB/CRFB modulators designed with standard methods (Schreier's
  delta-sigma toolbox approach, coefficients generated offline by `tools/design_sdm.py`),
  running at DSD64/128/256 after upsampling PCM to the DSD rate ÷ some factor and a final
  integer interpolation. Must stay stable at high input level: headroom + limiter on input,
  state reset on instability detection.
- SDM is the most CPU-heavy stage; it runs on the decode worker, never the RT thread, and
  needs a benchmark gate.
- Signal path model: each stage reports `(name, in_format, out_format, quality: Lossless|HighQuality|Enhanced|LowQuality)`; UI renders it like Roon's.

## Requirements

- [ ] `SdmStage` (order 5, order 7), output to the DoP packer from phase 5 or native DSD.
- [ ] Zone setting: output mode PCM | DSD (DSD64/128/256) when the device supports it.
- [ ] `SignalPath` model emitted with player state; UI panel (Now Playing → signal path).
- [ ] Verification suite (`crates/sonder-verify` or `tests/`):
  - bit-perfect: NullBackend byte equality for every PCM format fixture, DSP off;
  - real device: the `--verify-hash` hook from phase 1 on each backend (hash of bytes written into the device buffer vs decoded samples), because WASAPI loopback cannot capture exclusive streams;
  - where hardware allows, S/PDIF loopback capture or an HDCD/DTS-flag test file the DAC recognises only when bits are untouched (manual, documented in `docs/verification.md`);
  - resampler image rejection, PEQ response, dither noise floor (from phases 6–7, run together);
  - DoP round-trip and SDM output decodes (DSD→PCM) to the input tone within spec.
- [ ] Release check script: build installer, list bundle contents, fail if any Roon filter file or `roon` CSV is present.
- [ ] `docs/` : README (build, run, setting the Roon filter folder), `docs/verification.md`.

## Related Code Files

- Create: `crates/sonder-dsp/src/sdm.rs`, `tools/design_sdm.py`, `crates/sonder-engine/src/signal_path.rs`, `app/src/**/SignalPath*`, `tests/verify/*.rs`, `tools/check_bundle.ps1`, `README.md`, `docs/verification.md`.

## Implementation Steps

1. SDM design script + coefficients; `SdmStage` with stability guard; tests: tone in → DSD → DSD2PCM → tone out, SNR in audio band ≥ target.
2. Benchmark SDM at DSD256 stereo; record realtime factor; if too slow, cap default to DSD128.
3. Signal path model + UI panel.
4. Assemble verification suite; run on Windows with the user's DAC; record results in `docs/verification.md`.
5. Bundle check script; README.

## Todo

- [ ] SDM design + stage + tests
- [ ] SDM benchmark
- [ ] Signal path model + UI
- [ ] Verification suite
- [ ] Bundle check script
- [ ] README + verification doc

## Success Criteria

- All 8 brainstorm acceptance criteria are checked off with evidence in `docs/verification.md`.
- DAC shows DSD when playing a PCM file with PCM→DSD enabled.
- Bundle check passes (no Roon data in installer).

## Risk Assessment

- SDM instability on loud masters: limiter + reset + test with a full-scale square wave.
- macOS/Linux real-device verification may lack hardware: record as an open item, not silently passed.

## Security Considerations

- Release check prevents accidental redistribution of copyrighted Roon tables.

## Next Steps

- Later plans: ASIO/native DSD on Windows, convolution, core/remote split, streaming services.
