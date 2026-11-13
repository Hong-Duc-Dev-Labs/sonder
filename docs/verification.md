# Verification

What Sonder proves automatically, what has to be checked on hardware, and how each brainstorm
acceptance criterion is covered. Update the "recorded" columns when a run happens; an empty
real-device column means **not yet run**, never "passed".

## Run the automated suite

```sh
cargo test --workspace
cargo test -p sonder-verify            # the acceptance suite on its own
cargo clippy --workspace --all-targets -- -D warnings
cargo bench -p sonder-dsp --bench sdm  # PCM→DSD cost
bash scripts/check-forbidden-files.sh
pwsh tools/check_bundle.ps1            # -Build builds the installer first
```

The suite uses the null backend, so it needs no audio hardware and runs on every CI OS.

## Acceptance criteria

| # | Criterion | Automated evidence | Real-device evidence |
|---|-----------|--------------------|----------------------|
| 1 | Mixed FLAC/ALAC/WAV/AIFF/DSF/DFF library browsable by album/artist/track with cover art | `cargo test -p sonder-library`; phase 3/4 tests; UI scans and browses | UI click-through unrecorded |
| 2 | DSP off ⇒ bit-identical output for 16/44.1, 24/96, 24/192 | `dsp_off_is_byte_exact_for_every_pcm_depth_and_rate` (also 24/48, 16/48, 24/192); `the_device_bytes_match_the_decoded_samples` exercises the `--verify-hash` tap | `sonder-cli play <files> --verify-hash` on WASAPI exclusive: **not yet run** |
| 3 | Gapless within a format; correct rate switching | `crates/sonder-engine/tests/player.rs`: `same_format_tracks_play_gapless_and_byte_exact`, `narrower_track_stays_in_the_open_wider_container`, rate-switch tests | listening test: **not yet run** |
| 4 | DSD over DoP on a DoP DAC, DSD→PCM otherwise | `dop_playback_roundtrips_the_source_dsd_bytes`, `native_dsd_sends_four_source_bytes_per_word`, `dsd_mode_pcm_converts_instead_of_packing`, `dop_round_trip_preserves_every_byte` | DAC locks on DSD when playing a DSF file: **not yet run** |
| 5 | Upsampling with Sonder or Roon tables, no clipping, image rejection; PCM→DSD plays as DoP | `resampler_rejects_images_by_more_than_100_db`, `pcm_to_dsd64_reaches_the_device_over_dop`, `pcm_to_dsd128_and_dsd256_use_the_wider_carriers`, `pcm_to_dsd_uses_native_words_when_the_device_prefers_them`, `sdm_output_decodes_to_the_input_tone` | S/PDIF loopback capture or a DTS/HDCD-flagged file the DAC recognises only when untouched: **not yet run**; DAC showing DSD with PCM→DSD on: **not yet run** |
| 6 | PEQ, levelling, TPDF dither selectable and measured | `peq_matches_the_rbj_response`, `dither_replaces_truncation_distortion_with_noise`; phase 7 unit tests for levelling and dither | listening test with an EQ boost: **not yet run** |
| 7 | Signal path visible in the UI | `the_signal_path_lists_every_active_stage`; `PlayerSnapshot.signal_path` feeds the Now Playing panel; `cargo test -p sonder-verify` and the UI type check | panel inspected by hand: **not yet run** |
| 8 | Roon filter files absent from git and release bundles | `scripts/check-forbidden-files.sh` (CI), `the_repository_has_no_roon_filter_files`, `pwsh tools/check_bundle.ps1` | release bundle check on a built installer: **not yet run** |

## Measured results

Environment: Windows 10 26300, AMD64, release build, `cargo bench -p sonder-dsp --bench sdm`
(criterion, one iteration = one second of stereo audio). 2026-11-13.

| Mode | Time per 1 s of audio | Realtime factor |
|------|----------------------|-----------------|
| DSD64, 5th order | 252 ms | 4.0× |
| DSD128, 5th order | 477 ms | 2.1× |
| DSD256, 5th order | 1.02 s | 0.98× |
| DSD256, 7th order | 1.19 s | 0.84× |

The modulator alone (before the resampler that feeds it) is already at the limit for DSD256 on
this machine, so DSD128 is the recommended setting and DSD256/order 7 can underrun on slower
CPUs. This is recorded as a known limit, not hidden: the decode worker starves and the
underrun counter in the transport reports it.

Modulator quality (`tools/design_sdm.py`): every shipped table is stable (all poles inside the
unit circle), and the SDM tests measure ≥ 90 dB in-band SNR at DSD64 with the 5th-order table
and better with the 7th order (bitstream spectrum, harmonics excluded), plus 0 dBFS = 50 %
modulation and a state-limit reset that keeps a loud master from wedging the loop. A test
checks that every rate puts its NTF zeros at the same audio frequencies, and the DSD64
5th-order table (derived from the DSD128/DSD256 sets, pole radius 0.925 like its neighbours)
must keep the 10–20 kHz band at ≥ 110 dB SNR; the earlier copy of the DSD256 table measured
101.5 dB there, the derived one 117.5 dB.

DSP measurements recorded by the suite (values from the run in CI/host, 2026-11-13):

- Resampler 44.1k → 96k image rejection: > 100 dB (test fails below that).
- PEQ peak at 1 kHz, +6 dB: within 2 % of the RBJ response.
- Dither: a −90 dBFS 1 kHz tone at 16 bits without dither shows a third harmonic above −40 dB;
  with TPDF dither the same bin is below −50 dB and the error becomes a flat noise floor.
- SDM: decoded tone within 0.04 of the expected 0.35 amplitude at all four rate/order pairs,
  second harmonic below −60 dB.

## Real-hardware checklist (manual)

Windows (WASAPI exclusive):

1. `sonder-cli devices`, then `sonder-cli play <16/44.1 file> --verify-hash` — the command
   prints the decoded and device SHA-256s and ends with "Bit-perfect check PASSED", or fails
   naming the difference. Repeat for 24/96 and 24/192, and with `--volume -0.1` (must fail
   verification, proving the harness detects changes).
2. Play a DSF file with a DoP-capable DAC: the DAC's display must show DSD64/128. On a device
   without DoP the CLI prints the DSD→PCM reason.
3. Play a PCM file with `--pcm-to-dsd dsd64 --sdm-order 5`: the DAC must show DSD64. Repeat at
   DSD128; check the transport does not report underruns.
4. Bit-identity with no capture device: WASAPI loopback cannot capture exclusive-mode streams,
   so the `--verify-hash` tap is the evidence. Where a S/PDIF input exists, record the DAC's
   output and compare; a DTS- or HDCD-flagged file that the DAC recognises only when the bits
   are untouched is an additional check.
5. `pwsh tools/check_bundle.ps1 -Build` on the release build.

macOS (CoreAudio hog mode) and Linux (ALSA `hw:`) have no hardware in this environment: CI
compiles the backends and the engine is exercised through the null backend. Record real-device
results here when a machine is available; do not mark them passed by proxy.
