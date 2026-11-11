---
title: "Engine, gapless and OS backends"
date: 2026-11-11
summary: "Phase 2 engine, gapless, null-backend fault tests, WASAPI/ALSA/CoreAudio backends; 56 tests pass on Windows"
---

# Engine, gapless and OS backends

## What happened

Implemented the playback engine phase: `Player` (one engine thread owning queue, decoder,
DSP worker and stream), gapless transitions, exact seek, `Stage`/`Pipeline` with an integer
passthrough path, software volume as a stage, a `NullBackend` with fault injection
(busy, unplug/replug, xrun), WASAPI error classification (`AUDCLNT_E_DEVICE_INVALIDATED`,
missed events → `DeviceLost`), an ALSA `hw:` backend, a CoreAudio hog backend, and a
3-OS CI matrix. 56 tests pass on Windows; the 13 player tests were stable over 11 runs.

Surprises:
- The local windows-gnu Rust toolchain could not build the `windows` crates: its bundled
  `dlltool` shells out to a GNU `as` that was not installed. Fixed by installing WinLibs
  (MinGW-w64) via winget and putting its `bin` on PATH for builds.
- symphonia 0.6 readers mark encoder delay/padding on packets (`trim_start`/`trim_end`) but
  the decoders do not apply it, so `sonder-decode` now trims itself and decode-skips after an
  accurate seek to land on the exact frame.
- ALSA `S24_LE` is LSB-aligned, unlike our MSB-aligned 24-in-32, so ALSA only offers
  S16 / S24_3LE / S32.
- macOS IOProcs are Float32 since 10.6; hog mode + integer physical format is exact only for
  sources of 24 bits or fewer.

## Decision

Format boundaries are feed markers on the engine side (drain the stream, reopen) instead of
in-ring markers; flushes are a generation counter acknowledged by the render thread, and
`frames_consumed` maps device progress to track positions. Hot-plug is detected by polling
`device_ids`. A lost device is retried whenever it is listed again (covers sleep/resume
without a list change).

## Next steps

- Manual Windows test: live album gapless by ear, 44.1 → 96 kHz reopen on a real DAC.
- Run the CI workflow; macOS/Linux backends are type-checked only.
- Real-hardware CoreAudio/ALSA tests (phase 8); decide on 32-bit sources on macOS.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
