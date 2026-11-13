---
title: "Phase 8: SDM, signal path and verification"
date: 2026-11-13
summary: "Added PCM→DSD modulation (5th/7th order), wired it to DoP/native output, exposed the signal path in the UI, and built the acceptance suite, bundle check and docs."
---

# Phase 8: SDM, signal path and verification

## What happened

Implemented the last phase of the Sonder plan: PCM→DSD, the signal-path panel and the
verification suite.

- `sonder-dsp`: new `sdm` module. A 64-tap Kaiser-windowed polyphase interpolator lifts the
  PCM coming from the resampler (`DSD rate / 8`) to the bit rate, then a single-bit CIFF
  modulator runs at DSD64/128/256 with 5th- or 7th-order Schreier-designed tables. Input is
  clamped to ±0.5 (0 dBFS = 50 % modulation, the SACD convention), the state resets when the
  loop leaves its stable region (|state| > 1e7 or non-finite), and output is interleaved
  MSB-first DSD bytes. Tests cover the round-trip through `DsdToPcm`, a ≥ 90 dB in-band SNR
  target measured on the bitstream (harmonics excluded, Blackman-Harris window), order 7
  shaping harder than order 5, and a full-scale square wave that must not wedge the loop.
- `sonder-engine`: `SdmConfig` (`SdmMode` Off/DSD64/128/256 + order), `Command::SetSdm` and
  `Player::set_sdm`, an `Sdm` path in `DecodeWorker` (resampler chain → modulator → phase 5
  packer), DoP/native carrier choice from the device probe with a PCM fallback warning, dither
  disabled for the modulator, and a signal path that names SRC stages, PEQ, levelling, volume,
  the modulator (rate/order/rates) and the carrier. `reusable` now treats the native-DSD word
  container as reusable (previously every native track reopened the stream; that would have
  broken PCM→DSD gapless too).
- App/UI: settings gain `pcmToDsd` and `sdmOrder`; `PlayerSnapshot.signalPath` carries the
  engine's path; Now Playing has a signal-path toggle and a panel listing each stage, and the
  bit-perfect badge now trusts the engine's verdict.
- CLI: `--pcm-to-dsd` and `--sdm-order`; `--verify-hash` conflicts with them.
- `crates/sonder-verify`: acceptance suite on the null backend — byte-exact playback for six
  depth/rate pairs, the `--verify-hash` tap equality, resampler image rejection (> 100 dB),
  the RBJ PEQ peak (+6 dB within 2 %), dither versus truncation harmonics, the DoP round-trip,
  the SDM round-trip at four rate/order pairs, the signal-path model and a forbidden-file
  walk. `tools/check_bundle.ps1` inspects the bundle, staged resources and `app/dist`;
  `tools/design_sdm.py` re-derives the modulator NTF, poles and in-band gain; `README.md` and
  `docs/verification.md` document build, settings (including the Roon filter folder) and the
  acceptance evidence.

## Decisions

- Coefficients: the published Schreier single-bit CIFF sets for these exact bit rates,
  re-expressed for the sox-style delayed-feedback loop, with `tools/design_sdm.py` as the
  reviewable NTF/stability/report tool. Designing a new set at runtime would add startup cost
  and an untested stability risk for no audible gain.
- 8× internal interpolation instead of resampling PCM all the way to the bit rate: the same
  work is far cheaper with the existing polyphase planner plus a short interpolator, and it
  keeps the modulator at one fixed rate for both 44.1k and 48k families (the input has already
  been resampled to `rate/8`).
- SNR is measured on the bitstream, not on decoded PCM: a 120 dB DSD→PCM decimator still leaks
  enough of the loud out-of-band DSD noise to hide a floor better than ~50 dB.

## Verification

- `cargo test --workspace`: all suites pass (sonder-dsp 55, engine 55 including the new
  PCM→DSD/native-gapless tests, sonder-verify 9, library/app/lyrics/output unchanged).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean; `cargo fmt --all -- --check`:
  clean.
- `pnpm check`, `pnpm test` (12 tests), `pnpm build`: clean; bindings regenerated.
- Benchmark (release, one second of stereo): DSD64 252 ms (4.0× realtime), DSD128 477 ms
  (2.1×), DSD256 1.02 s (0.98×), DSD256/order 7 1.19 s (0.84×). DSD128 is documented as the
  recommended setting; DSD256 can underrun on slower machines.
- `pwsh tools/check_bundle.ps1` and `bash scripts/check-forbidden-files.sh`: pass (no Roon
  files tracked or bundled).
- Real-DAC checks (DoP lock, DSD→PCM, `--verify-hash` on WASAPI exclusive, S/PDIF loopback,
  bundle check on a fresh installer) are listed in `docs/verification.md` as open.

## Next steps

- Run the hardware checklist on the user's DAC.
- Consider optimising the modulator (interpolator SIMD, skip per-phase normalisation) before
  recommending DSD256 on laptops.

## Review fixes (same day)

A code review of the pending phase 8 diff found six issues; all are fixed with tests that
fail on the old code.

- **DSD64 5th-order table was DSD256's.** Each resonator `g` sets an NTF zero at
  `sqrt(g)·bit rate/2π`; the shared table put DSD64's zeros at 3.0/5.0 kHz instead of
  11.9/20.0 kHz. The replacement keeps DSD128's zero frequencies and extrapolates the NTF
  denominator from DSD128 and DSD256 (each halving of the rate moves every coefficient 4× as
  far), then solves for `a` in this loop. The same procedure reproduces the published DSD64
  7th-order set to six digits; the pole radius is 0.925, like DSD128. 10–20 kHz SNR went from
  101.5 dB to 117.5 dB. Python is not installed on this machine, so the check was done with a
  scratch Rust program rather than `tools/design_sdm.py`.
- **Signal path listed Levelling for DoP/native DSD** when levelling was on with no gain, and
  so lost "Bit-perfect". The PCM stages are now listed only when the output is not a bitstream,
  matching `compose_stages`.
- **Gapless PCM→DSD reopened the device** when native DSD was requested on a DoP-only DAC and
  the device answered "busy" while our stream held it. `choose_sdm` now reuses the open
  stream, as `choose_dsd_mode` does.
- The "cannot carry DSD" warning is shown once per device and rate, not on every track.
- The ±0.5 limit now applies after the 8× interpolator, so inter-sample overshoot is limited
  as documented. The interpolator comments now give the real cutoff (1/32 of the bit rate),
  and the unreachable centre-tap value is correct.
