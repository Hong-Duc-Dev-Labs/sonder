---
title: "Phase 7: PEQ, dither and levelling"
date: 2026-11-12
summary: "Added RBJ parametric EQ, ReplayGain/R128 levelling, TPDF dither and the settings/UI that drive them."
---

# Phase 7: PEQ, dither and levelling

## What happened

Implemented phase 7 of the Sonder lossless player: PCM shaping stages and the settings to
drive them.

- `sonder-dsp`: new `biquad` (RBJ cookbook, TDF-II), `peq` (per-channel cascade + preamp),
  `leveling` (ReplayGain gain with peak limiting behind an atomic handle), `dither` (TPDF,
  SplitMix64, final quantiser) and `volume` (moved out of the engine).
- `sonder-library`: new `loudness` module implementing EBU R128/BS.1770 integrated loudness
  and sample peak in-crate, plus a per-album analysis job. Migration `0003_loudness.sql` adds
  `loudness_lufs/loudness_peak/album_lufs/album_peak`; `track_loudness(path)` feeds the engine.
- `sonder-engine`: `DspConfig`, `LoudnessSource`/`NoLoudness`, `Command::SetDsp` and
  `Command::SetSrc`, stage composition in Roon order (headroom → SRC → PEQ → levelling →
  volume → dither), per-track levelling gain and album-auto decision, dither depth from the
  device container, clip counter (`PlayerEvent::Clipping`, once per track).
- App/UI: settings carry EQ + named presets, levelling mode/target, dither and every phase 6
  SRC option; new commands for loudness analysis, pending count and the Roon folder picker;
  Settings screen sections for EQ, levelling/dither and upsampling.

Regression found and fixed during verification: dither was initially inserted on a 16-bit
device even when nothing shaped the signal, which broke the bit-perfect fallback tests. It is
now gated on the chain actually shaping samples (plan, EQ, levelling or volume). Dither depth
is kept when a probe fails under an open exclusive stream instead of being dropped mid-track.
DSD now converts to PCM when EQ/levelling is active, not only for volume.

## Decision

- Implement R128 directly instead of adding the `ebur128` crate: no new dependency, exact
  control of gating and rate handling, covered by analytic tests.
- Album loudness is a duration-weighted energy average of measured tracks, not a decode of the
  whole album: handles mixed-rate albums and partial analyses, and keeps relative track
  loudness by construction.
- Levelling publishes its gain through an `AtomicU64` shared with the stage, so gapless next
  tracks change gain without rebuilding the chain.
- Dither only when the chain shapes the signal and the device's widest container is below 24
  bits; output spec then requests exactly that width.

## Verification

- `cargo test --workspace`: all suites pass (48 sonder-dsp, 27 library unit, 17 library
  integration, 48 engine player tests incl. the new order/dither/bit-perfect ones, app tests).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `pnpm check` (svelte-check, fail on warnings), `pnpm build`, `pnpm test`: clean; bindings
  regenerated.

## Review follow-up (2026-11-13)

A local review before shipping found five problems in the first version, all fixed with tests:

- ReplayGain tag gains ignored the levelling target (they always aimed at −18 LUFS), so tagged
  and measured tracks disagreed at any other target. Tag gains are now shifted to the target.
- Silent or sub-400 ms tracks stored the −71 LUFS placeholder as a measurement and got up to
  +24 dB; they also dragged album averages down. The placeholder now yields no gain and is left
  out of the album average.
- Rescans kept measurements of replaced files and album values of albums that changed. The
  scanner now clears them, and the analysis pass rebuilds album values even with nothing to
  decode.
- The dither quantiser clamps before the integer conversion, so clipping went unreported on
  16-bit devices. Dither now counts clamped input samples into the clip report.
- Analysis held the SQLite write lock while decoding a whole album and could run twice at once
  (post-scan pass and Settings button). Decoding now happens before the write transaction and
  one app-level lock serialises the passes.

A second, fresh-context review before shipping found more:

- The loudness meter indexed its 8-channel arrays with the stream's channel count, so a
  10- or 12-channel file panicked the scan thread and stopped scans for the session. The meter
  now weighs the first eight channels, and a decoder panic counts as a failed file.
- Folder-watcher rescans cleared measurements but never re-measured, and an analysis could
  write the old file's measurement over a re-indexed row. Analysis now has its own queued
  worker started by scans and the watcher, and the write checks `mtime`/`size`.
- Turning levelling on forced every DSD track to PCM for a 0 dB stage. Only a real gain does
  now, and the chain stays empty for DoP/native DSD.
- Surround channels of 5.1 now get the BS.1770 +1.5 dB weight.

The TypeScript bindings were also stale (the bindings test failed) and ten files failed
`cargo fmt --check`; both were fixed before the ship.

## Next steps

- Manual listen: EQ band behaviour, album levelling on a real album, dither to a 16-bit-only
  device.
- GUI click-through of the new Settings sections was not run (no display session); the web
  build and type check are the evidence so far.
- Phase 8 (SDM signal path and verification) remains.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
