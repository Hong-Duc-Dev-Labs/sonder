---
phase: 8
title: "Phase 8: SDM, signal path and verification"
status: in progress
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

- [x] `SdmStage` (order 5, order 7), output to the DoP packer from phase 5 or native DSD.
- [x] Zone setting: output mode PCM | DSD (DSD64/128/256) when the device supports it.
- [x] `SignalPath` model emitted with player state; UI panel (Now Playing → signal path).
- [x] Verification suite (`crates/sonder-verify` or `tests/`):
  - [x] bit-perfect: NullBackend byte equality for every PCM format fixture, DSP off;
  - [x] real device: the `--verify-hash` hook from phase 1 on each backend (hash of bytes written into the device buffer vs decoded samples), because WASAPI loopback cannot capture exclusive streams; the suite proves the tap, the real-device run itself is recorded in `docs/verification.md` (open);
  - [x] where hardware allows, S/PDIF loopback capture or an HDCD/DTS-flag test file the DAC recognises only when bits are untouched (manual, documented in `docs/verification.md`);
  - [x] resampler image rejection, PEQ response, dither noise floor (from phases 6–7, run together);
  - [x] DoP round-trip and SDM output decodes (DSD→PCM) to the input tone within spec.
- [x] Release check script: build installer, list bundle contents, fail if any Roon filter file or `roon` CSV is present.
- [x] `docs/` : README (build, run, setting the Roon filter folder), `docs/verification.md`.

## Related Code Files

- Create: `crates/sonder-dsp/src/sdm.rs`, `tools/design_sdm.py`, `crates/sonder-engine/src/signal_path.rs`, `app/src/**/SignalPath*`, `tests/verify/*.rs`, `tools/check_bundle.ps1`, `README.md`, `docs/verification.md`.

## Implementation Steps

1. SDM design script + coefficients; `SdmStage` with stability guard; tests: tone in → DSD → DSD2PCM → tone out, SNR in audio band ≥ target.
2. Benchmark SDM at DSD256 stereo; record realtime factor; if too slow, cap default to DSD128.
3. Signal path model + UI panel.
4. Assemble verification suite; run on Windows with the user's DAC; record results in `docs/verification.md`.
5. Bundle check script; README.

## Todo

- [x] SDM design + stage + tests
- [x] SDM benchmark
- [x] Signal path model + UI
- [x] Verification suite
- [x] Bundle check script
- [x] README + verification doc

## Success Criteria

- [x] Every one of the 8 brainstorm acceptance criteria has evidence in `docs/verification.md`
  (automated tests for all eight; the real-device rows are listed as open, not passed).
- [ ] DAC shows DSD when playing a PCM file with PCM→DSD enabled (manual, open).
- [x] Bundle check passes (no Roon data in installer).

**Implementation note.** `sonder-dsp::Sdm` is an 8× polyphase interpolator (64-tap
Kaiser-windowed sinc, per-phase unity gain) followed by a single-bit CIFF loop whose
feedforward/resonator tables are the Schreier-synthesised single-bit sets for each DSD rate,
5th and 7th order, except DSD64 5th order, which is derived from the DSD128/DSD256 sets
(`tools/design_sdm.py` re-derives their NTF, poles and in-band gain). 0 dBFS
PCM maps to 50 % modulation (the SACD convention sox and Roon follow), loud inter-sample peaks
are limited after interpolation rather than wrapped, and a loop that leaves its stable region (state above 1e7 or
non-finite) resets and reports once per track. The engine resamples PCM to `DSD rate / 8` with
the phase 6 planner, then `DecodeWorker::prepare_sdm` runs the PCM stages, the modulator and
the phase 5 packer; the carrier is DoP or native DSD, chosen from the device probe, with a
fallback to PCM plus a warning when the device takes neither. The signal path reports the SRC
stages, PEQ, levelling, volume, the modulator (rate, order, input → bit rate) and the carrier
as separate stages, and the UI shows that list in Now Playing. Verification:
`crates/sonder-verify/tests/verify.rs` covers byte-exact playback for six depth/rate pairs,
the device-tap (`--verify-hash`) equality, 100 dB resampler image rejection, the RBJ PEQ peak,
dither against truncation harmonics, the DoP round-trip, the SDM round-trip at four
rate/order pairs, the signal path and the forbidden-file guard. Benchmarks (criterion, release,
one second of stereo): DSD64 4.0× realtime, DSD128 2.1×, DSD256 0.98×, DSD256/order 7 0.84×;
DSD128 is documented as the recommended setting. Hardware checks remain open and are listed in
`docs/verification.md`.

## Risk Assessment

- SDM instability on loud masters: limiter + reset + test with a full-scale square wave.
- macOS/Linux real-device verification may lack hardware: record as an open item, not silently passed.

## Security Considerations

- Release check prevents accidental redistribution of copyrighted Roon tables.

## Next Steps

- Later plans: ASIO/native DSD on Windows, convolution, core/remote split, streaming services.
