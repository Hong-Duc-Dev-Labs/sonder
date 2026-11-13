---
phase: 2
title: "Phase 2: Convolution and crossfeed"
status: todo
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
- [ ] `Convolver` stage: uniformly partitioned overlap-save, partition size 1024 frames
      (latency = one partition), complex multiply-accumulate over a frequency-domain delay
      line; all buffers allocated in `configure`.
- [ ] IR loading off the audio thread: WAV (16/24/32-bit int, 32-bit float), 1 channel
      (applied to every channel) or exactly the stream's channel count; other counts are an
      error shown in settings. Max IR length 2^20 frames after resampling.
- [ ] IR rate ≠ stream rate: resample the IR once with Sonder's resampler at configure time;
      the signal path says "IR resampled from X to Y".
- [ ] IR gain: no automatic normalisation; the existing headroom setting covers boost.
      Settings show the IR's peak frequency-response gain so the user can set headroom.
- [ ] `flush` emits the IR tail; `reset` clears history.
- [ ] Settings: `convolution: { enabled, path }`; missing/unreadable file ⇒ stage off and a
      warning in the signal path, never a playback failure.

### Crossfeed
- [ ] `Crossfeed` stage, bs2b topology (low-pass on the cross path, high-shelf on the direct
      path, level-compensated), presets: Default (700 Hz / 4.5 dB), Chu Moy (700 Hz / 6 dB),
      Jan Meier (650 Hz / 9.5 dB).
- [ ] Stereo only; other channel counts bypass and the signal path says so.
- [ ] Settings: `crossfeed: { enabled, preset }`.

### Wiring
- [ ] `DspConfig` gains `convolution` and `crossfeed`; `StageKey` includes them.
- [ ] Signal path lists each stage with its parameters; both mark the path "not bit-perfect".
- [ ] Tauri settings DTOs (serde defaults keep old settings files loading), a "Pick IR file"
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

- [ ] Convolver + tests
- [ ] IR loader + resampling + errors
- [ ] Crossfeed + tests
- [ ] Engine chain, StageKey, signal path
- [ ] Settings, picker command, bindings, UI
- [ ] README settings list

## Success Criteria

- Unit tests above pass; `cargo test --workspace`, clippy and `pnpm check` pass.
- A 65 536-tap stereo IR at 192 kHz runs ≥ 10× realtime on this machine (criterion bench
  added to `benches/`).
- With both stages off, `dsp_off_is_byte_exact_for_every_pcm_depth_and_rate` still passes.

## Risk Assessment

- One-partition latency adds 1024 frames at track start; it is a stage delay like the
  resampler's and must be trimmed on seek/start the same way (check how `Pipeline` handles
  the resampler's group delay and reuse it).
- Very long IRs at 384 kHz cost memory: the 2^20 cap bounds it (~32 MB stereo complex).
