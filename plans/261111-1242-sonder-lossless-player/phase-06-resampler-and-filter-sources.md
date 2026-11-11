---
phase: 6
title: "Phase 6: Resampler and filter sources"
status: todo
priority: P2
effort: "2w"
dependencies: [2]
---

# Phase 6: Resampler and filter sources

## Overview

One FIR core used by every rate converter in Sonder: a polyphase resampler stage for
upsampling and the automatic SRC fallback, also reused by phase 5 (DSD→PCM) and phase 8
(PCM→DSD). Coefficients come from a `FilterSource` with two real variants:
Sonder-designed filters (default, shipped) and Roon tables loaded at runtime from a
user-configured folder (never bundled).

## Key Insights

- Roon tables (`D:\Dev\reverse\roon\extracted\filters\*.csv`, `index,coefficient`) each
  sum to 1.0 overall, so the interpolator scales by L. Per-branch sums only approximate
  1/L (error bounded by stopband leakage): tests use a tolerance, not equality. Lengths
  are not multiples of L (e.g. 25601 taps for 160/147): zero-pad to `ceil(n/L)·L`.
- Ratios present: integer 2/3/4/6/8/16/32/64x, `3_2x` (32k→48k), 44.1→48 families
  `160_147x`, `320_147x`, `640_147x`, `1280_147x`, and decimating `147_{20,40,80}x`
  (L = 20/40/80, cutoff tracks the output Nyquist). There is **no 48k→44.1k (147/160)**
  table and no 128x/256x: `FilterSource` resolution is **per ratio** — use Roon when the
  table exists and the user enabled it, otherwise Sonder — and the signal path shows which
  source served each stage.
- Roon exposes 4 user choices (linear/minimum × precise/smooth); Sonder exposes the same
  4 and keeps `2s` as an internal low-latency option.
- Cost is `taps / L` multiply-adds per output sample per channel (1280/147 precise ≈ 166
  MAC, table ≈ 1.7 MB) — fine. The expensive case is high ratios (PCM→DSD256): use
  multistage by default above 8x (one sharp 2x/4x stage, then cheap short stages), not a
  single long filter.
- Sonder's own filters are designed **at runtime in Rust** and cached on disk: Kaiser-
  windowed sinc for linear phase; minimum phase by the cepstral method with `realfft`.
  Cap minimum-phase design length (e.g. ≤ 2^20-point FFT) and use multistage for larger
  ratios. No Python step, no committed `.bin` files.

## Requirements

- [ ] `sonder-dsp` crate: `FirCore` (polyphase, rational L/M, per-channel history, chunked f64 inner loop; `wide` for SIMD if stable SIMD is not available).
- [ ] `FilterSource` trait `fn get(&self, spec: FilterSpec) -> Option<Arc<FilterTable>>`; impls `SonderDesigned` (runtime design + disk cache) and `RoonTables { dir }`; resolver picks per ratio with fallback to Sonder.
- [ ] `FilterSpec { num, den, phase: Linear|Minimum, variant: Precise|Smooth|Short }`.
- [ ] `RoonTables` loads CSV lazily, validates (finite, total sum ≈ 1, length matches manifest when present), zero-pads, scales by L; missing folder ⇒ clear warning and Sonder fallback.
- [ ] `PolyphaseResampler` stage with multistage planner (above 8x).
- [ ] Target policy: max rate supported by device, keep rate family, or fixed rate; plus force-upsample below 44.1k.
- [ ] **Automatic SRC fallback** when the device cannot play the source rate even with DSP off (192k file on a 96k DAC, 48k file on a 44.1k-only device, rates below 44.1k); signal path marks it "not bit-perfect".
- [ ] Headroom stage (default −3 dB when resampling) preceding SRC.
- [ ] Settings exposed through the engine config struct (filter source + folder, phase, variant, target policy); UI controls land in phase 7.

## Related Code Files

- Create: `crates/sonder-dsp/src/{lib.rs,fir.rs,filter_source.rs,roon_tables.rs,sonder_design.rs,resampler.rs,multistage.rs,headroom.rs}`.
- Modify: `crates/sonder-engine/src/pipeline.rs`, `crates/sonder-engine/src/player.rs` (fallback SRC on format refusal).

## Implementation Steps

1. `FirCore` + tests: impulse response equals table, branch DC gain ≈ 1 within tolerance.
2. Sonder design (Kaiser linear, cepstral minimum) + cache; frequency-response tests (precise: passband ripple ≤ 0.001 dB, stopband ≥ 140 dB target, measured ≥ 120 dB).
3. `RoonTables` loader + per-ratio resolver; Roon tests are `#[ignore]` and read `SONDER_ROON_FILTERS` so CI never needs the files.
4. Resampler stage + multistage planner; tests: 1 kHz sine 2x/4x/8x/44.1→48k/48→44.1k image rejection and THD+N.
5. Benchmarks (`criterion`): 192k→768k stereo and 44.1k→DSD256-rate PCM stereo; record realtime factors.
6. Engine wiring: headroom + SRC, automatic fallback, signal-path source labels.

## Todo

- [ ] FIR core + tests
- [ ] Sonder runtime filter design + cache + response tests
- [ ] RoonTables loader + per-ratio resolver + fallback
- [ ] Resampler stage + multistage planner + accuracy tests
- [ ] Benchmarks
- [ ] Automatic SRC fallback for unsupported device rates
- [ ] Engine wiring + config

## Success Criteria

- `cargo test -p sonder-dsp` passes; ignored Roon tests pass locally with `SONDER_ROON_FILTERS=D:\Dev\reverse\roon\extracted\filters`.
- 48k→44.1k works with Roon enabled (served by Sonder; signal path says so).
- A 192 kHz file on a device capped at 96 kHz plays via SRC fallback, labelled not bit-perfect.
- CI guard from phase 1 still reports no Roon data in git.

## Risk Assessment

- Misreading normalisation gives a −20·log10(L) dB level error: covered by the DC gain test.
- Minimum-phase design time for long filters: cache on disk; design lazily on first use; show progress.

## Security Considerations

- Roon tables are copyrighted third-party data: loaded from user path only, never logged in full, never uploaded.

## Next Steps

- Phase 5 reuses `FirCore` for DSD→PCM; phase 7 adds stages and UI; phase 8 uses multistage for PCM→DSD.
