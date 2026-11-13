---
phase: 5
title: "Phase 5: Real-device verification"
status: todo
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

- [ ] Test fixtures generated into the scratchpad (never into git): 1 kHz sine at −20 dBFS,
      10 s, as FLAC 16/44.1, 24/96, 24/192 and WAV 24/192 (encoder: whichever of `flac`/
      `ffmpeg` is on PATH; otherwise a small `sonder-verify` helper writes WAV and the FLAC
      rows use files from the user's library at those formats).
- [ ] `--verify-hash` PASS on device 2 for each fixture.
- [ ] `--verify-hash --volume -0.1` FAILS (harness detects change).
- [ ] DSF file (user library or generated DSD64 via the SDM) with DoP: user reports the DAC
      display.
- [ ] `--pcm-to-dsd dsd64 --sdm-order 5`, then `dsd128`, then `dsd256 --sdm-order 7`: user
      reports the display; CLI underrun counter recorded (covers phase 1 end to end).
- [ ] Short listening check with crossfade on between two library tracks and with
      convolution (Dirac IR ⇒ must sound unchanged) — user reports.
- [ ] `pwsh tools/check_bundle.ps1 -Build` on the release build.

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

- [ ] Fixtures
- [ ] PCM hash checks (3 formats + WAV)
- [ ] Negative check
- [ ] DoP / PCM→DSD with user observation
- [ ] Crossfade/convolution listening check
- [ ] Bundle check
- [ ] Verification doc updated

## Success Criteria

- `docs/verification.md` shows TP35 results for criteria 2, 4, 5 (DSD rows) and 8, with
  commands and dates; nothing marked passed by proxy.

## Risk Assessment

- Exclusive mode can be refused if another app holds the device: close other audio apps,
  record the error if it persists.
- macOS/Linux stay open (no hardware).
