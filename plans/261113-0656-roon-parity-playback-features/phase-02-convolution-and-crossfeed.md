---
phase: 2
title: "Phase 2: Convolution and crossfeed"
status: completed
priority: P2
effort: "1w"
dependencies: []
---

# Phase 2: Convolution and crossfeed

## Overview

Two new PCM stages in `sonder-dsp`, wired into the engine chain, settings, signal path and UI.
Chain order after this phase:
headroom → SRC → **convolution** → PEQ → **crossfeed** → levelling → volume → dither (→ SDM).

## Key Insights

- `sonder_dsp::Stage` (`crates/sonder-dsp/src/stage.rs`) already defines the contract;
  `realfft` is a `sonder-dsp` dependency; `biquad.rs` gives the shelving filters crossfeed needs.
- Headroom and SRC are added in `crates/sonder-engine/src/rate.rs:93`; PEQ, levelling and
  volume in `Engine::compose_stages` (`crates/sonder-engine/src/player.rs:1031`). Convolution
  must sit after SRC, so it goes at the start of `compose_stages` (which runs after the rate
  plan), then PEQ, then crossfeed after PEQ.
- `StageKey` (`player.rs:470`) decides when stages are rebuilt; both new settings join it.
- WAV IRs are read with `hound` (workspace dependency).

## Requirements

### Convolution
- [x] `Convolver` stage: uniformly partitioned overlap-save, partition size 1024 frames
      (latency = one partition), complex multiply-accumulate over a frequency-domain delay
      line; all buffers allocated in `configure`.
- [x] IR loading off the audio thread: WAV (16/24/32-bit int, 32-bit float), 1 channel
      (applied to every channel) or exactly the stream's channel count; other counts are an
      error shown in settings. Max IR length 2^20 frames after resampling.
- [x] IR rate ≠ stream rate: resample the IR once with Sonder's resampler at configure time;
      the signal path says "IR resampled from X to Y".
- [x] IR gain: no automatic normalisation; the existing headroom setting covers boost.
      Settings show the IR's peak frequency-response gain so the user can set headroom.
- [x] `flush` emits the IR tail; `reset` clears history.
- [x] Settings: `convolution: { enabled, path }`; missing/unreadable file ⇒ stage off and a
      warning in the signal path, never a playback failure.

### Crossfeed
- [x] `Crossfeed` stage, bs2b topology (low-pass on the cross path, high-shelf on the direct
      path, level-compensated), presets: Default (700 Hz / 4.5 dB), Chu Moy (700 Hz / 6 dB),
      Jan Meier (650 Hz / 9.5 dB).
- [x] Stereo only; other channel counts bypass and the signal path says so.
- [x] Settings: `crossfeed: { enabled, preset }`.

### Wiring
- [x] `DspConfig` gains `convolution` and `crossfeed`; `StageKey` includes them.
- [x] Signal path lists each stage with its parameters; both mark the path "not bit-perfect".
- [x] Tauri settings DTOs (serde defaults keep old settings files loading), a "Pick IR file"
      command using the native picker (like `settings_pick_roon_folder`), regenerated
      `bindings.ts`, and controls in `SettingsView.svelte`.

## Related Code Files

- Create: `crates/sonder-dsp/src/convolver.rs`, `crates/sonder-dsp/src/crossfeed.rs`.
- Modify: `crates/sonder-dsp/src/lib.rs`, `crates/sonder-engine/src/dsp.rs`,
  `crates/sonder-engine/src/player.rs` (`compose_stages`, `StageKey`, `signal_path`),
  `crates/sonder-engine/src/stage.rs`, `app/src-tauri/src/settings.rs`,
  `app/src-tauri/src/commands.rs`, `app/src-tauri/src/main.rs`, `app/src/lib/bindings.ts`
  (generated), `app/src/components/SettingsView.svelte`, `README.md` (settings list),
  `crates/sonder-verify/tests/verify.rs`.

## Implementation Steps

1. `Convolver` with unit tests: Dirac IR ⇒ identity (≤ 1e-12); random signal vs direct-form
   convolution ≤ 1e-9; block sizes 1, 333, 1024, 4096 give identical output; flush emits
   exactly `ir_len - 1` tail frames.
2. IR loader + resampling + error cases (wrong channel count, empty file, too long).
3. `Crossfeed` with unit tests: L = R input keeps its level within 0.1 dB at 100 Hz–10 kHz;
   hard-left 5 kHz tone leaks to the right channel attenuated per preset; mono input bypasses.
4. Engine wiring + signal path + `sonder-verify` cases: both off ⇒ byte-exact (existing
   test extended), both on ⇒ listed in the signal path.
5. Settings/commands/UI; `pnpm check`.

## Todo

- [x] Convolver + tests
- [x] IR loader + resampling + errors
- [x] Crossfeed + tests
- [x] Engine chain, StageKey, signal path
- [x] Settings, picker command, bindings, UI
- [x] README settings list

## Success Criteria

- Unit tests above pass; `cargo test --workspace`, clippy and `pnpm check` pass.
- A 65 536-tap stereo IR at 192 kHz runs ≥ 10× realtime on this machine (criterion bench
  added to `benches/`).
- With both stages off, `dsp_off_is_byte_exact_for_every_pcm_depth_and_rate` still passes.

**Implementation note.** `sonder-dsp::convolver` holds `ImpulseResponse`, which loads WAV
files with `hound`, caches the last file by path, mtime and length, and caches resampled copies
per rate. Resampling uses Sonder's precise linear filters, removes their delay (read from a
unit impulse) and scales by `from / to` so the gain is unchanged. `Convolver` is a uniformly
partitioned overlap-save convolver on `realfft`. It holds up to one partition and emits the
exact convolution, so no silence is added; `flush` emits the held frames plus `len − 1` tail
frames. `sonder-dsp::crossfeed` follows libbs2b's `init` and `cross_feed` exactly. In the
engine, `DspConfig` gains `convolution: Option<Result<Arc<ImpulseResponse>, IrError>>` and
`crossfeed`, and `StageKey` keys the response by identity. The chain is headroom → SRC →
convolution → PEQ → crossfeed → …. The signal path names the file, taps, rate and any
resampling. Tests:
- `sonder-dsp`: 12 tests: Dirac identity, direct-form match ≤ 1e-9, block-size invariance,
  per-channel stereo, pass-through on a channel mismatch, resampled timing and 0 dB gain, peak
  gain, limits, and WAV loading.
- `sonder-verify`: a Dirac response stays byte-exact across a gapless join with no frames lost;
  stage order and detail in the signal path; an unreadable file gives a warning and keeps
  bit-perfect output.
- `sonder-engine`: convolution moves ahead of SRC only above 384 kHz.
- App: settings mapping and old settings files still load.

Bench, IR 65 536 taps, 192 kHz stereo: 38 ms on a P-core, 75 ms on an E-core, 92 ms unpinned,
≥ 10.9× realtime.

**Deviations.**
- Partition size adapts to the response: 1024–8192 frames, about 128 partitions, instead of a
  fixed 1024. A 1 s response at 192 kHz would need 188 partitions of 1024; at 8192 the delay
  line stays short. Latency is not affected, because the output is not delayed.
- The limit is 2 s of response at its own rate, not 2^20 frames after resampling. A frame count
  would either reject ordinary 65k-tap responses at 768 kHz or allow ~100 MB per channel; a
  duration is what users understand.
- New: above 384 kHz (PCM→DSD128/256), convolution runs before SRC at the source rate. Without
  this, a response would be stretched to millions of taps at the modulator's input rate.
- An unreadable file or a channel mismatch is reported as a player notice
  (`PlayerEvent::Warning`), not as a stage in the signal path. A stage that does not run would
  wrongly mark the path "not bit-perfect".
- Crossfeed acceptance: bs2b keeps a centred sound's level at low frequencies (≤ 0.05 dB) but
  dips it 1.06 dB (Jan Meier) to 1.75 dB (Default) above the cut-off. This is libbs2b's design,
  so the "L = R within 0.1 dB at 100 Hz–10 kHz" criterion was wrong. The test asserts the bass
  level, no boost, the documented dip bound, and a low-frequency separation equal to the
  preset's feed level.
- Not checked in the running app: the Settings controls and the IR picker (UI type check only).

## Risk Assessment

- One-partition latency adds 1024 frames at track start; it is a stage delay like the
  resampler's and must be trimmed on seek/start the same way (check how `Pipeline` handles
  the resampler's group delay and reuse it).
- Very long IRs at 384 kHz cost memory: the 2^20 cap bounds it (~32 MB stereo complex).
