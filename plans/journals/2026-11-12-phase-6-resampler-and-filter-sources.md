---
title: Phase 6 resampler and filter sources
date: 2026-11-12
summary: "Polyphase FIR, Sonder/Roon filter sources, automatic SRC fallback"
---

# Phase 6 resampler and filter sources

## What changed

- Added `sonder-dsp`: `FirCore` (polyphase, rational L/M, per-channel history), `Resampler` stage, multistage planner, headroom stage, and the shared `Stage`/`StreamFormat` contract.
- Added `FilterSource` with `SonderDesigned` (runtime Kaiser-windowed sinc, cepstral minimum phase via `realfft`, disk cache) and `RoonTables` (lazy CSV/manifest validation, zero-padding, branch scaling by L). A chain resolves Roon per ratio and falls back to Sonder.
- Added engine SRC policy in `sonder-engine/src/rate.rs`: target policy, force-upsample below 44.1 kHz, per-stage planning, and device-rate fallback.
- Wired `PlayerConfig.src`, headroom before SRC, automatic fallback on `UnsupportedFormat`, and `PlayerEvent::SignalPath` / `Warning`. CLI and app surface both events.

## Decisions

- Minimum-phase tables keep the first 2N coefficients of an 8N-point cepstral transform; truncating at N left about 1 dB passband ripple.
- Multistage chains use the configured variant for the first stage and `Short` for later stages, cutting the PCM→DSD256-rate chain from about 0.13x to about 1.9x realtime.
- DSP asks for 24-bit output, but the engine lowers that to the source depth when the device has no 24-bit container.
- Roon tables remain per ratio; 48k→44.1k has no table and is served by Sonder with the source named in the signal path.

## Verification

- `cargo test --workspace` passes; `cargo clippy --workspace --all-targets -- -D warnings` passes.
- Ignored Roon test passes locally with `SONDER_ROON_FILTERS` (2x, 3/2x, 8x, 160/147, 147/20).
- Integration tests cover 96k→44.1k and 192k→96k SRC fallback (not bit-perfect), plus Roon-enabled 48k→44.1k served by Sonder.
- Quick release benches: 192k→768k stereo ≈183 ms per 1 s (≈5.5x realtime); 44.1k→11.2896 MHz stereo ≈53 ms per 0.1 s (≈1.9x realtime).

## Review fixes

- A code review found that odd source rates crashed the engine. 5512 Hz → 44.1k made the
  multistage planner panic on the prime factor 53. 11127 Hz with minimum phase attempted a
  2^25-point FFT, then panicked when the result exceeded `MAX_TAPS`. Primes above 8 now
  get their own stage. Minimum phase above 131072 linear taps is served linear. A ratio
  over `MAX_TAPS` is refused with a warning, and the track keeps its source rate.
- A previous PCM track's resampler forced the next DSD track to DSD→PCM. The DSD mode now
  depends on volume only, and DoP and native DSD drop the plan.
- The signal path is now sent again when it changes mid-track, for example on a volume
  change. The 16-bit downgrade applies only after the device probe succeeded.
- Not changed: Roon's 16x/32x/64x/1280_147x tables are unused because ratios above 8x are
  split by design.
- One full-suite run timed out once in the new mid-track volume test. It did not
  reproduce in 13 later runs.

## Open

- Manual listen on a real DAC (192 kHz file on a 96 kHz device) remains.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
