---
phase: 6
title: "Phase 6: Resampler and filter sources"
status: in progress
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
  `160_147x`, `320_147x`, `640_147x`, `1280_147x`, and the 48k-family upsamplers
  `147_{20,40,80}x` (L = 147, e.g. 48k→352.8k for `147_20x`; the loaded branch gains are
  unity with L = 147). There is **no 48k→44.1k (147/160)**
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

- [x] `sonder-dsp` crate: `FirCore` (polyphase, rational L/M, per-channel history, chunked f64 inner loop; `wide` for SIMD if stable SIMD is not available).
- [x] `FilterSource` trait `fn get(&self, spec: FilterSpec) -> Option<Arc<FilterTable>>`; impls `SonderDesigned` (runtime design + disk cache) and `RoonTables { dir }`; resolver picks per ratio with fallback to Sonder.
- [x] `FilterSpec { num, den, phase: Linear|Minimum, variant: Precise|Smooth|Short }`.
- [x] `RoonTables` loads CSV lazily, validates (finite, total sum ≈ 1, length matches manifest when present), zero-pads, scales by L; missing folder ⇒ clear warning and Sonder fallback.
- [x] `PolyphaseResampler` stage with multistage planner (above 8x).
- [x] Target policy: max rate supported by device, keep rate family, or fixed rate; plus force-upsample below 44.1k.
- [x] **Automatic SRC fallback** when the device cannot play the source rate even with DSP off (192k file on a 96k DAC, 48k file on a 44.1k-only device, rates below 44.1k); signal path marks it "not bit-perfect".
- [x] Headroom stage (default −3 dB when resampling) preceding SRC.
- [x] Settings exposed through the engine config struct (filter source + folder, phase, variant, target policy); UI controls land in phase 7.

## Related Code Files

- Create: `crates/sonder-dsp/src/{lib.rs,fir.rs,filter_source.rs,roon_tables.rs,sonder_design.rs,resampler.rs,multistage.rs,headroom.rs,stage.rs}`, `crates/sonder-engine/src/rate.rs`, `crates/sonder-dsp/benches/resampler.rs`.
- Modify: `crates/sonder-engine/src/player.rs` (SRC planning, automatic fallback, signal path), `worker.rs` (output depth on 16-bit devices), `stage.rs` (re-export the shared stage contract), `crates/sonder-cli/src/main.rs` and `app/src-tauri/src/events.rs` (signal path and warnings).

## Implementation Steps

1. `FirCore` + tests: impulse response equals table, branch DC gain ≈ 1 within tolerance.
2. Sonder design (Kaiser linear, cepstral minimum) + cache; frequency-response tests (precise: passband ripple ≤ 0.001 dB, stopband ≥ 140 dB target, measured ≥ 120 dB).
3. `RoonTables` loader + per-ratio resolver; Roon tests are `#[ignore]` and read `SONDER_ROON_FILTERS` so CI never needs the files.
4. Resampler stage + multistage planner; tests: 1 kHz sine 2x/4x/8x/44.1→48k/48→44.1k image rejection and THD+N.
5. Benchmarks (`criterion`): 192k→768k stereo and 44.1k→DSD256-rate PCM stereo; record realtime factors.
6. Engine wiring: headroom + SRC, automatic fallback, signal-path source labels.

## Todo

- [x] FIR core + tests
- [x] Sonder runtime filter design + cache + response tests
- [x] RoonTables loader + per-ratio resolver + fallback
- [x] Resampler stage + multistage planner + accuracy tests
- [x] Benchmarks
- [x] Automatic SRC fallback for unsupported device rates
- [x] Engine wiring + config

## Success Criteria

- [x] `cargo test -p sonder-dsp` passes; the ignored Roon test passes locally with `SONDER_ROON_FILTERS=D:\Dev\reverse\roon\extracted\filters` (2x, 3/2x, 8x, 160/147, 147/20 loaded; branch gains within 1e-3).
- [x] 48k→44.1k with Roon enabled is served by Sonder and the signal path says so; covered by `roon_enabled_without_a_48_to_44_table_reports_sonder`.
- [x] A 192 kHz file on a device capped at 96 kHz plays via SRC fallback, labelled not bit-perfect; covered by `a_192k_file_on_a_96k_device_plays_via_src_fallback`.
- [x] CI guard from phase 1 still reports no Roon data in git (no tables or manifests were added).

**Implementation note.** The stage contract moved into `sonder-dsp` so resamplers and the
engine share one interface. Minimum-phase tables keep the first `2N` coefficients of an
`8N`-point cepstral transform: truncating at `N` left ≈1 dB passband ripple. Multistage
plans use the configured variant for the first stage and `Short` for later stages, which is
what makes the PCM→DSD256-rate chain affordable. Quick `criterion` run (release, stereo):
192k→768k ≈183 ms per 1 s of audio (≈5.5× realtime); 44.1k→11.2896M ≈53 ms per 0.1 s
(≈1.9× realtime). `cargo test --workspace` and
`cargo clippy --workspace --all-targets -- -D warnings` pass.

**Odd source rates.** Rates such as 5512 or 11127 Hz reduce to ratios with a prime factor
above 8 (11025/1378, 14700/3709). The planner gives such a prime its own stage instead of
panicking. Minimum-phase designs whose linear prototype exceeds 131072 taps (a 2^20-point
cepstral FFT) are served linear phase, and the signal path names the phase actually used.
A ratio needing more than `MAX_TAPS` is refused; the engine warns and keeps the source rate.
A rate-conversion plan belongs to the PCM source it was built for: a following DoP or
native DSD track drops it instead of being forced to DSD→PCM.

## Risk Assessment

- Misreading normalisation gives a −20·log10(L) dB level error: covered by the DC gain test.
- Minimum-phase design time for long filters: cache on disk; design lazily on first use; show progress.
- 16-bit-only devices: DSP asks for 24 bits, but the engine downgrades the request to the
  source depth when the device has no 24-bit container, so fallback SRC still plays.

## Security Considerations

- Roon tables are copyrighted third-party data: loaded from user path only, never logged in full, never uploaded.

## Next Steps

- Phase 5 reuses `FirCore` for DSD→PCM; phase 7 adds stages and UI; phase 8 uses multistage for PCM→DSD.
- Phase 7 covers the 16-bit-only fallback: a shaped signal sent to a device without a 24-bit
  container is dithered to 16 bits, and the signal path names the dither.
