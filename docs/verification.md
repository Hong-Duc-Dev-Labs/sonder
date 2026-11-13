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
cargo bench -p sonder-dsp --bench convolver  # convolution cost at 192 kHz
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

Environment: Windows 11 26300, Intel Core i5-12450HX laptop on AC power (4 P-cores with
hyper-threading + 4 E-cores), release build, `cargo bench -p sonder-dsp --bench sdm`
(criterion, one iteration = one second of stereo audio). 2026-11-13.

This machine runs the same code at two speeds: unpinned runs land either near the P-core
figure or up to ~3× slower (core placement and clock state), so each row gives runs pinned to
one P-core and one E-core plus the range seen unpinned. The E-core column is the planning
number: Windows can move a background app's threads to E-cores.

Modulator alone (`sdm/*`):

| Mode | P-core | E-core | Unpinned range | Worst realtime factor |
|------|--------|--------|----------------|-----------------------|
| DSD64, 5th order | — | — | 33–66 ms | 15× |
| DSD128, 5th order | — | — | 74–136 ms | 7.4× |
| DSD256, 5th order | 194 ms | 271 ms | 135–280 ms | 3.6× |
| DSD256, 7th order | 225 ms | 318 ms | 174–379 ms | 2.6× |

Whole PCM→DSD path for a 44.1 kHz source (`pcm_to_dsd/*`: resampling to `DSD rate / 8` in
1024-frame blocks, then the modulator):

| Mode | P-core | E-core | Unpinned | Worst realtime factor |
|------|--------|--------|----------|-----------------------|
| DSD128, 5th order | 215 ms | 381 ms | 403 ms | 2.5× |
| DSD256, 7th order | 371 ms | 632 ms | 705 ms | 1.4× |

For DSD256/order 7 the modulator used to take 340 ms to 1.19 s unpinned and 869 ms pinned to
an E-core, which together with the resampler could underrun. The current loop keeps the channel
state in registers, specialises it per order, builds each byte in a register and runs two
channels interleaved; a test checks it emits the same bytes as the straightforward form,
including stability resets. The resampler now costs more than the modulator on the whole
path. If a slower CPU does underrun, the decode worker starves and the transport's underrun
counter reports it.

Convolution (`cargo bench -p sonder-dsp --bench convolver`, one second of 192 kHz stereo in
4096-frame blocks, a decaying-noise stereo response):

| Response | P-core | E-core | Unpinned | Worst realtime factor |
|----------|--------|--------|----------|-----------------------|
| 65 536 taps | 38 ms | 75 ms | 92 ms | 10.9× |
| 1 s (192 000 taps) | 70 ms | 124 ms | 149 ms | 6.7× |

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
