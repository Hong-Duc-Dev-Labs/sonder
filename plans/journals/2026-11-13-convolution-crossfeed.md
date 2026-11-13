---
title: "Convolution and crossfeed"
date: 2026-11-13
summary: "Added a partitioned FFT convolver for WAV impulse responses and bs2b crossfeed to the chain, with settings, signal path entries and engine-level tests."
---

# Convolution and crossfeed

## What happened

Two new PCM stages in `sonder-dsp`, placed headroom → SRC → convolution → PEQ → crossfeed →
levelling → volume → dither.

- The convolver uses uniformly partitioned overlap-save on `realfft`. It holds frames until
  its partition fills instead of delaying the signal, so output is the exact convolution and
  `flush` releases the held frames plus the response's tail. An engine test proves it with a
  Dirac response: byte-exact output across a gapless join, nothing lost, and silence only
  after the music.
- Impulse responses are WAV only (a user decision). A response is resampled once per stream
  rate, with the filter delay removed and the gain kept, then cached. Settings shows the peak
  gain so the user knows how much headroom to give.
- Crossfeed follows libbs2b's coefficients exactly.

## Decisions and surprises

- **The plan's crossfeed criterion was wrong.** bs2b keeps a centred sound's bass level but
  dips it 1.06–1.75 dB above the cut-off. That is the design, so the test checks bass level,
  the dip bound and that the low-end separation equals the preset's feed level.
- **Convolution moves ahead of SRC above 384 kHz.** With PCM→DSD128/256, SRC targets 705.6
  kHz or more, which would stretch a 65k-tap response past a million taps. Found while
  writing the ordering test. The first version of that test assumed DSD64 would trigger the
  move; 352.8 kHz does not, and the engine was right.
- **Response limit is 2 s, not 2^20 frames.** A frame cap after resampling either rejects
  ordinary responses at 768 kHz or allows ~100 MB per channel.
- **A failure is a notice, not a signal-path stage.** An unreadable file or a channel
  mismatch reaches the UI as `PlayerEvent::Warning`. A stage that does not run would wrongly
  clear "bit-perfect".

## Numbers

IR 65 536 taps, 192 kHz stereo: 38 ms per second on a P-core, 75 ms on an E-core, 92 ms
unpinned, so at least 10.9× realtime.

## Open

The Settings controls and the IR picker passed the type check but have not been clicked
through in the running app.
