---
phase: 5
title: "Phase 5: Real-device verification"
status: completed
priority: P1
effort: "1d"
dependencies: [1, 2, 3]
---

# Phase 5: Real-device verification

## Overview

Run the manual checklist in `docs/verification.md` on the attached DAC
(`[2] Speakers (TP35)`, WASAPI exclusive, 44.1–384 kHz) and record the results. The user
watches the DAC's display for the steps that need eyes (user decision).

## Key Insights

- `sonder-cli play <file> --verify-hash` prints decoded and device SHA-256s and ends with
  "Bit-perfect check PASSED" or names the difference.
- WASAPI loopback cannot capture exclusive streams, so the device tap is the evidence.
- TP35 DoP support is unknown: if DoP is refused, the CLI prints the DSD→PCM reason and that
  outcome is recorded as such, not as a failure of Sonder.
- Playing audio is audible: test tones at −20 dBFS, and the user is told before each step.

## Requirements

- [x] Test fixtures generated into the scratchpad (never into git): 1 kHz sine at −20 dBFS,
      10 s, as FLAC 16/44.1, 24/96, 24/192 and WAV 24/192 (encoder: whichever of `flac`/
      `ffmpeg` is on PATH; otherwise a small `sonder-verify` helper writes WAV and the FLAC
      rows use files from the user's library at those formats).
- [x] `--verify-hash` PASS on device 2 for each fixture.
- [x] `--verify-hash --volume -0.1` FAILS (harness detects change).
- [x] DSF file (user library or generated DSD64 via the SDM) with DoP: user reports the DAC
      display.
- [x] `--pcm-to-dsd dsd64 --sdm-order 5`, then `dsd128`, then `dsd256 --sdm-order 7`: user
      reports the display; CLI underrun counter recorded (covers phase 1 end to end).
- [x] Short listening check with crossfade on between two library tracks and with
      convolution (Dirac IR ⇒ must sound unchanged) — user reports.
- [x] `pwsh tools/check_bundle.ps1 -Build` on the release build.

## Related Code Files

- Modify: `docs/verification.md` (real-device column, measured results, date, device name),
  `plans/261111-1242-sonder-lossless-player/plan.md` phase statuses where the open manual
  item is now closed.

## Implementation Steps

1. `cargo build --release -p sonder-cli`; `sonder-cli devices` to confirm the index.
2. Generate fixtures; run the PCM hash checks; run the negative check.
3. Ask the user before each audible DSD step; record what they see.
4. Bundle check.
5. Update `docs/verification.md`: fill the real-device column with date, device, command and
   result; leave anything not run as **not yet run**.

## Todo

- [x] Fixtures
- [x] PCM hash checks (3 formats + WAV)
- [x] Negative check
- [x] DoP / PCM→DSD with user observation
- [x] Crossfade/convolution listening check
- [x] Bundle check
- [x] Verification doc updated

## Success Criteria

- `docs/verification.md` shows TP35 results for criteria 2, 4, 5 (DSD rows) and 8, with
  commands and dates; nothing marked passed by proxy.

## Risk Assessment

- Exclusive mode can be refused if another app holds the device: close other audio apps,
  record the error if it persists.
- macOS/Linux stay open (no hardware).

## Result

Recorded in `docs/verification.md` under "Real-device run: TP35, 2026-11-13".

- Fixtures: no `flac`/`ffmpeg` on PATH and no 24/192 FLAC in the library, so
  `sonder-verify` gained `fixtures` (verbatim-frame FLAC + WAV writers, round-trip tested) and
  `examples/hardware_fixtures.rs`, written to the scratchpad.
- `--verify-hash` PASSED for FLAC 16/44.1, 24/96, 24/192 and WAV 24/192, 0 underruns.
- The negative check could not run as documented: the CLI declared `--verify-hash` and
  `--volume` as conflicting. The conflict was dropped (PCM→DSD still conflicts); the run with
  `--volume -0.1` now FAILS with differing hashes, as required.
- DSF DSD64 over DoP: clean music. The TP35 LED follows the stream rate (red 44.1, light blue
  176.4, purple 352.8 kHz) and has no DSD indicator: a 176.4 kHz PCM control lit it the same.
  DoP decoding is evidenced by clean sound, not by the display.
- PCM→DSD: DSD64/5, DSD128/5 and DSD128/7 clean with 0 underruns. DSD256 cannot run as DoP on
  this DAC (needs a 705.6 kHz carrier, TP35 stops at 384 kHz): the CLI warns and plays PCM.
- Crossfade: skipped with "different output format" across formats (as specified); smooth
  between same-format tracks of different albums. Dirac convolution sounds unchanged.
- `pwsh tools/check_bundle.ps1 -Build`: passed on both bundles.
