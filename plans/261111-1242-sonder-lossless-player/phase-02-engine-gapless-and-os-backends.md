---
phase: 2
title: "Phase 2: Engine, gapless and OS backends"
status: todo
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

- [ ] `sonder-engine`: `Player` with commands play/pause/stop/seek/next/prev/enqueue/clear, state events (position, track, format, underruns) over a crossbeam channel.
- [ ] Gapless transitions for same-format tracks; reopen on format change.
- [ ] `Stage` trait (interleaved f64): `fn configure(&mut self, input: StreamFormat) -> StreamFormat; fn max_out(&self, in_frames: usize) -> usize; fn process(&mut self, input: &[f64], out: &mut [f64]) -> usize; fn flush(&mut self, out: &mut [f64]) -> usize; fn reset(&mut self)` — allocation only in `configure`; variable output size returned as a count.
- [ ] Device error channel from every backend: device removed (`AUDCLNT_E_DEVICE_INVALIDATED`, ALSA `ENODEV`), exclusive access denied (`AUDCLNT_E_DEVICE_IN_USE`), xrun (ALSA `EPIPE`), sleep/resume. Policy: pause + emit `DeviceLost`/`DeviceBusy` event; recover xruns in place; retry open on device re-appearance. Device add/remove notifications feed the `devices changed` event used by phase 4.
- [ ] Unsupported output format with DSP off: refuse with a clear error (phase 6 adds SRC fallback).
- [ ] Integer passthrough fast path when the stage list is empty (no f64 conversion at all).
- [ ] `CoreAudioHog` backend (macOS) and `AlsaHw` backend (Linux) behind `cfg(target_os)`.
- [ ] Software volume is a `Stage` (off by default) so bit-perfect stays the default.
- [ ] GitHub Actions CI: fmt, clippy, test on windows/macos/ubuntu (no audio device; backends tested via a `NullBackend` that records written bytes).

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

- [ ] Device error channel + recovery policy; NullBackend can inject each error and tests assert the policy
- [ ] Player + commands + events
- [ ] Queue, gapless, seek
- [ ] Stage pipeline + passthrough fast path
- [ ] NullBackend + byte-equality tests
- [ ] CoreAudio hog backend
- [ ] ALSA hw backend
- [ ] CI workflow

## Success Criteria

- `cargo test -p sonder-engine` includes a gapless byte-equality test and passes.
- On Windows: an album plays gapless (no audible gap on a live album); switching from 44.1 to 96 kHz reopens the device correctly.
- CI green on all three OSes.

## Risk Assessment

- macOS/Linux hardware not available to the user: backends compile and pass NullBackend tests; real-device test listed as an open item in phase 8.
- Seek accuracy in FLAC without seek tables: accept symphonia's coarse seek then decode-skip to the exact frame.

## Security Considerations

- No network surface. Hog mode must be released on crash: register drop guard and a panic hook that releases hog.

## Next Steps

- Phases 3, 5, 6 can run in parallel once this lands.
