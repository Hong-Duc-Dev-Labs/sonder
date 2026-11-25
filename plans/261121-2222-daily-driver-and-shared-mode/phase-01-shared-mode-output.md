# Phase 1 — Shared Mode Output (`sonder-output`, `sonder-engine`)

## Goal

Support opt-in `ShareMode::Shared` alongside the default `ShareMode::Exclusive` across all OS audio backends and the playback engine, while preserving byte-exact bit-perfect playback in Exclusive mode and honest Signal Path reporting in Shared mode.

## Changes

- `crates/sonder-output/src/lib.rs`: add `ShareMode` (`Exclusive` [default], `Shared`) and `OutputBackend::shared_format(&self, device, channels) -> Result<Option<DeviceFormat>, OutputError>`.
- `crates/sonder-output/src/stream.rs`: add `share_mode: ShareMode` to `StreamOptions`.
- `crates/sonder-output/src/wasapi.rs`: implement `shared_format` via `get_mixformat()` and `init_shared` via `StreamMode::EventsShared { autoconvert: true, buffer_duration_hns }`.
- `crates/sonder-output/src/coreaudio.rs`: implement `shared_format` via `kAudioStreamPropertyVirtualFormat` and bypass `HogGuard` / physical format changes when opening in `ShareMode::Shared`.
- `crates/sonder-output/src/alsa.rs`: implement `shared_format` (48 kHz S32) and open `"default"` PCM device with `set_rate_resample(true)` in `ShareMode::Shared`.
- `crates/sonder-output/src/null.rs`: track `share_mode` on `NullSession`, configurable `with_shared_format(...)`, and allow PCM formats in `Shared` mode for engine verification.
- `crates/sonder-engine/src/{lib,player}.rs`: expose `ShareMode`, `PlayerConfig.share_mode`, `Command::SetShareMode`, `Player::set_share_mode`, `SignalPath.share_mode`, automatic DSD→PCM fallback and SDM suppression in Shared mode, mixer-rate SRC targeting, and `"OS Mixer"` stage in `SignalPath`.

## Verification

- Engine & verification tests proving default Exclusive remains byte-exact and Shared mode opens with `ShareMode::Shared`, emits `"OS Mixer"`, marks `bit_perfect == false`, converts DSD → PCM, and switches modes mid-playback cleanly.
