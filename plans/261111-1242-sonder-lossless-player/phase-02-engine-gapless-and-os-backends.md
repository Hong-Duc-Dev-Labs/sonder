---
phase: 2
title: "Phase 2: Engine, gapless and OS backends"
status: in-progress
priority: P1
effort: "2w"
dependencies: [1]
---

# Phase 2: Engine, gapless and OS backends

## Overview

Turn the spike into a playback engine: queue, transport commands, gapless transitions,
device rate switching, a DSP stage pipeline (empty for now), and exclusive backends for
macOS and Linux.

## Key Insights

- Gapless = keep the device stream open while the next track resolves to the same
  **output** `DeviceFormat` (after the DSP chain, not the source format: with SRC on,
  44.1k and 88.2k tracks can both go out at 352.8k). Pre-decode the next track into the
  same ring. Output format change ⇒ flush stage tails, drain, reopen.
- The ring carries format/epoch markers so the render thread knows exactly where the old
  format ends before a reopen; stateful stages keep history across same-format tracks.
- DSP runs before the ring, so control changes (volume, PEQ, seek) lag by the ring depth:
  bound ring depth (~100–200 ms) and flush it on seek or DSP change.
- symphonia reports encoder delay/padding for some codecs; honour trim so gapless albums
  have no clicks.
- macOS: hog mode (`kAudioDevicePropertyHogMode`) + set physical format
  (`kAudioStreamPropertyPhysicalFormat`) for integer mode; use `coreaudio-sys` /
  `objc2-core-audio`. Linux: `alsa` crate opening `hw:CARD,DEV` with exact hw params,
  no plug layer.
- The DSP pipeline is a fixed-order `Vec<Box<dyn Stage>>` mirroring Roon's chain; with
  zero stages the path is identity, which is what bit-perfect tests exercise.

## Requirements

- [x] `sonder-engine`: `Player` with commands play/pause/stop/seek/next/prev/enqueue/clear, state events (position, track, format, underruns) over a crossbeam channel.
- [x] Gapless transitions for same-format tracks; reopen on format change.
- [x] `Stage` trait (interleaved f64): `fn configure(&mut self, input: StreamFormat) -> StreamFormat; fn max_out(&self, in_frames: usize) -> usize; fn process(&mut self, input: &[f64], out: &mut [f64]) -> usize; fn flush(&mut self, out: &mut [f64]) -> usize; fn reset(&mut self)` — allocation only in `configure`; variable output size returned as a count.
- [x] Device error channel from every backend: device removed (`AUDCLNT_E_DEVICE_INVALIDATED`, ALSA `ENODEV`), exclusive access denied (`AUDCLNT_E_DEVICE_IN_USE`), xrun (ALSA `EPIPE`), sleep/resume. Policy: pause + emit `DeviceLost`/`DeviceBusy` event; recover xruns in place; retry open on device re-appearance. Device add/remove notifications feed the `devices changed` event used by phase 4.
- [x] Unsupported output format with DSP off: refuse with a clear error (phase 6 adds SRC fallback).
- [x] Integer passthrough fast path when the stage list is empty (no f64 conversion at all).
- [x] `CoreAudioHog` backend (macOS) and `AlsaHw` backend (Linux) behind `cfg(target_os)`.
- [x] Software volume is a `Stage` (off by default) so bit-perfect stays the default.
- [x] GitHub Actions CI: fmt, clippy, test on windows/macos/ubuntu (no audio device; backends tested via a `NullBackend` that records written bytes).

## Architecture

```
Player (control thread)
  ├─ Queue
  ├─ DecodeWorker: Decoder → [Stage...] → pack → rtrb
  └─ OutputBackend stream (RT thread)
NullBackend: records bytes to Vec for tests
```

## Related Code Files

- Create: `crates/sonder-engine/src/{lib.rs,player.rs,queue.rs,pipeline.rs,stage.rs}`, `crates/sonder-output/src/{coreaudio.rs,alsa.rs,null.rs}`, `.github/workflows/ci.yml`.
- Modify: `crates/sonder-cli/src/main.rs` (queue a folder, gapless).

## Implementation Steps

1. Extract the phase-1 wiring into `Player` + `DecodeWorker`; add `NullBackend`.
2. Implement queue + gapless + seek; tests with `NullBackend` assert that concatenated output equals concatenated decoded samples for two same-format tracks.
3. Implement `Stage` pipeline and integer fast path.
4. Implement `CoreAudioHog` and `AlsaHw`; manual test on each OS if hardware/VM access exists, otherwise compile-check in CI and mark manual test pending.
5. Add CI workflow.

## Todo

- [x] Device error channel + recovery policy; NullBackend can inject each error and tests assert the policy
- [x] Player + commands + events
- [x] Queue, gapless, seek
- [x] Stage pipeline + passthrough fast path
- [x] NullBackend + byte-equality tests
- [x] CoreAudio hog backend
- [x] ALSA hw backend
- [x] CI workflow
- [ ] Manual: Windows album gapless by ear + 44.1→96 kHz reopen on a real DAC
- [ ] Manual: CoreAudio and ALSA on real hardware (compile-checked only; tracked in phase 8)
- [ ] CI green on all three OSes (workflow not yet run)

## Success Criteria

- `cargo test -p sonder-engine` includes a gapless byte-equality test and passes.
- On Windows: an album plays gapless (no audible gap on a live album); switching from 44.1 to 96 kHz reopens the device correctly.
- CI green on all three OSes.

## Risk Assessment

- macOS/Linux hardware not available to the user: backends compile and pass NullBackend tests; real-device test listed as an open item in phase 8.
- Seek accuracy in FLAC without seek tables: accept symphonia's coarse seek then decode-skip to the exact frame.

## Security Considerations

- No network surface. Hog mode must be released on crash: register drop guard and a panic hook that releases hog.

## Implementation Notes

- One engine thread (`player.rs`) owns queue, decoder, DSP worker and stream; it decodes into a
  small feed of `TrackStart` / samples / `End` items and pushes them into the existing `rtrb`
  ring. Format boundaries are feed markers: a track whose post-DSP spec does not fit the open
  device format drops the producer (end of input), the stream drains and closes, and the next
  stream opens. Epochs are a flush generation on `StreamHandle`: the render thread discards the
  queue and acknowledges; `frames_consumed` (written + discarded) maps device progress back to
  track positions.
- Gapless also covers a narrower track after a wider one at the same rate (16-bit in an open
  24-bit container is still bit-perfect).
- Policy: `DeviceLost`/`DeviceNotFound` → pause + `DeviceLost`, retry when the device is listed
  again (presence, rate-limited, so sleep/resume without a list change also recovers);
  `DeviceInUse` → pause + `DeviceBusy`, play retries; xruns counted in place; unsupported format
  → `TrackFailed` and skip. Hot-plug is detected by polling `OutputBackend::device_ids`.
- After the queue plays to the end, play starts at the first track enqueued afterwards; with
  nothing added it starts the queue over. A seek within the playing track does not emit
  `TrackChanged`.
- ALSA opens `hw:` non-blocking (a blocking open of a busy device waits in the kernel instead
  of failing) and waits on `EAGAIN` itself.
- ALSA exposes S16 / S24_3LE / S32 only (ALSA `S24_LE` is LSB-aligned); 24-bit sources fall back
  to S32 unchanged.
- CoreAudio: hog mode + integer physical format; the IOProc writes Float32 (the HAL has had no
  integer virtual format since 10.6), exact for ≤24-bit sources. True 32-bit sources lose the
  low 8 bits on macOS — revisit in phase 8.
- macOS and Linux backends are type-checked (`cargo clippy --target …`) but untested on hardware.

## Next Steps

- Phases 3, 5, 6 can run in parallel once this lands.
