---
phase: 1
title: "Phase 1: Workspace and WASAPI exclusive spike"
status: todo
priority: P1
effort: "1w"
dependencies: []
---

# Phase 1: Workspace and WASAPI exclusive spike

## Overview

Retire the biggest risk first: prove that Rust can decode a FLAC file and push it
bit-perfect to a DAC through WASAPI exclusive mode, including sample-rate switching.
Also lay down the cargo workspace every later phase builds on.

## Key Insights

- `cpal` only exposes shared mode on WASAPI, so exclusive output needs the `wasapi`
  crate (HEnquist/wasapi-rs, used by CamillaDSP) or direct `windows` crate calls.
- Exclusive mode requires an exact format match (rate, bit depth, container: 16, 24-in-32,
  32 int/float). The device may reject 24-bit packed; negotiate via `IsFormatSupported`
  and fall back 24-in-32 → 32 int.
- Event-driven exclusive mode needs a buffer period aligned to the device's
  `GetDevicePeriod`; misalignment produces `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED`.

## Requirements

- [ ] Cargo workspace `D:\Dev\Sonder` with crates: `sonder-decode`, `sonder-output`, `sonder-engine` (empty shell), `sonder-cli`.
- [ ] `git init`, `.gitignore` (target, node_modules, `polyphase_*`, `*.csv`, `filters_manifest.json`, `*.decrypted`), `rustfmt.toml`, MIT/Apache licence choice left to user.
- [ ] CI guard from day one: a script fails if `git ls-files` lists any `polyphase_*`, `*.csv` under a filters path, or `*.decrypted`; verify with `git check-ignore -v polyphase_2x_linear_phase_precise.csv`.
- [ ] If the device does not support the source rate/format, refuse with a clear message listing supported formats (automatic SRC fallback arrives in phase 6).
- [ ] `sonder-decode`: wrap `symphonia` (features: flac, alac, wav, aiff, isomp4) into `Decoder` yielding interleaved `i32`/`f64` frames plus `SourceFormat { rate, bits, channels }`.
- [ ] `sonder-output`: `OutputBackend` trait + `WasapiExclusive` impl: enumerate devices, query supported formats, open at source rate, event-driven render loop fed from a lock-free SPSC ring buffer (`rtrb`).
- [ ] `sonder-cli play <file> [--device N]` plays one file exclusively; `sonder-cli devices` lists devices + supported rates.

## Architecture

```
decoder thread ──frames──▶ rtrb SPSC ring ──▶ WASAPI event thread (RT, no alloc)
                                              └─ packs i32 → device container
```

`OutputBackend` (shared by phase 2 backends):

```rust
pub trait OutputBackend {
    fn devices() -> Result<Vec<DeviceInfo>>;            // name, id, supported formats, dop/native dsd hints
    fn open(&mut self, dev: &DeviceId, fmt: DeviceFormat) -> Result<Stream>;
}
pub struct DeviceFormat { rate: u32, container: SampleContainer, channels: u16 }
```

## Related Code Files

- Create: `Cargo.toml` (workspace), `crates/sonder-decode/src/lib.rs`, `crates/sonder-output/src/{lib.rs,wasapi.rs,format.rs}`, `crates/sonder-cli/src/main.rs`, `.gitignore`, `rust-toolchain.toml`.

## Implementation Steps

1. `git init`; create workspace and crates; pin toolchain 1.98.
2. Implement `sonder-decode` over symphonia; unit-test format detection on generated WAV/FLAC fixtures (generate fixtures in tests with `hound` + `flac` encoding via symphonia-free path, or commit tiny public-domain files).
3. Implement sample packing (`i32` → S16LE / S24LE-in-32 / S32LE) with exhaustive unit tests (round-trip, sign extension).
4. Implement `WasapiExclusive`: COM init on the render thread, `IAudioClient::Initialize(EXCLUSIVE, EVENTCALLBACK)`, format negotiation, MMCSS "Pro Audio" thread priority.
5. Wire decoder → `rtrb` → render thread in `sonder-cli`; handle underrun by writing silence and counting it.
6. Manual verification on the user's DAC at 44.1/96/192 kHz.

## Todo

- [ ] Workspace + git + gitignore
- [ ] symphonia decoder wrapper + tests
- [ ] sample packing + tests
- [ ] WASAPI exclusive backend
- [ ] CLI play/devices
- [ ] Manual DAC test at three rates

## Success Criteria

- `cargo test -p sonder-decode -p sonder-output` passes.
- `sonder-cli play file.flac` plays 16/44.1, 24/96, 24/192 with the DAC display showing the source rate.
- Debug hook `--verify-hash`: hashes the exact bytes written into the WASAPI buffer and compares them to the decoded samples packed into the same container; hashes match. (WASAPI loopback cannot capture exclusive streams, so this hook is the primary bit-perfect evidence. Bit-perfect means equal sample values after container alignment, e.g. 16-bit in 24-in-32.)
- A 10-minute playback logs 0 underruns.

## Risk Assessment

- DAC rejects a container: fallback chain + clear error listing supported formats.
- Driver quirks under exclusive mode: keep backend thin so phase 2 can add ASIO if needed (ASIO is deferred, see plan open questions).

## Security Considerations

- None beyond file input; symphonia handles untrusted files, but decode on a non-RT thread and treat errors as skip-track, never panic.

## Next Steps

- Phase 2 generalises the engine and adds CoreAudio + ALSA backends.
