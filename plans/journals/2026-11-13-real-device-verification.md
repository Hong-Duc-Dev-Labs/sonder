---
title: "Real-device verification on the TP35: bit-perfect, DoP by ear, DSD256 out of reach"
date: 2026-11-13
summary: "Hash checks passed at three rates, DoP and PCM→DSD played clean, and the TP35 turned out to have no DSD indicator and no carrier for DSD256 over DoP."
---

# Real-device verification

## What happened

The phase assumed an encoder on PATH or 24/192 FLAC in the library; neither existed. A
verbatim-frame FLAC writer in `sonder-verify` (round-trip tested against the decoder) made the
tones instead. The first test looked like a broken writer until the numbers showed the decoder
returns samples left-justified in 32 bits: 465 became 465 × 2¹⁶.

`--verify-hash` passed on the TP35 for FLAC 16/44.1, 24/96, 24/192 and WAV 24/192. The
documented negative check (`--volume -0.1` must fail) could not run: the CLI declared the two
flags conflicting. The conflict was dropped, and the negative run now fails with differing
hashes, which is the point of it.

## Decisions

- DoP evidence is the sound, not the display. The TP35's LED colour follows the stream rate:
  DoP DSD64 and a 176.4 kHz PCM control lit it the same light blue. A DAC that ignores DoP
  plays the marker bytes as loud hiss; the music played clean.
- DSD256 is recorded as not runnable on this DAC (DoP needs a 705.6 kHz carrier; the TP35 stops
  at 384 kHz). The order-7 modulator was run at DSD128 instead.
- The first crossfade attempt did not fade: the tracks differed in output format, and the
  signal path said so, as specified. A same-format pair faded smoothly.

## Open

- Gapless-by-ear and rate switching (criterion 3), resampler/EQ listening (6) and the UI
  click-through (1, 7) were not part of this phase and stay open.
- S/PDIF capture, macOS and Linux hardware: no equipment.
