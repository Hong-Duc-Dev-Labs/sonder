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
| 2 | DSP off ⇒ bit-identical output for 16/44.1, 24/96, 24/192 | `dsp_off_is_byte_exact_for_every_pcm_depth_and_rate` (also 24/48, 16/48, 24/192); `the_device_bytes_match_the_decoded_samples` exercises the `--verify-hash` tap | TP35, WASAPI exclusive, 2026-11-13: PASSED for FLAC 16/44.1, 24/96, 24/192 and WAV 24/192; `--volume -0.1` FAILED as required ([run](#real-device-run-tp35-2026-11-13)) |
| 3 | Gapless within a format; correct rate switching | `crates/sonder-engine/tests/player.rs`: `same_format_tracks_play_gapless_and_byte_exact`, `narrower_track_stays_in_the_open_wider_container`, rate-switch tests | listening test: **not yet run** |
| 4 | DSD over DoP on a DoP DAC, DSD→PCM otherwise | `dop_playback_roundtrips_the_source_dsd_bytes`, `native_dsd_sends_four_source_bytes_per_word`, `dsd_mode_pcm_converts_instead_of_packing`, `dop_round_trip_preserves_every_byte` | TP35, 2026-11-13: DSF DSD64 over DoP plays clean music (a DAC that ignores DoP plays it as loud hiss); the TP35 shows no DSD indicator, its LED follows the carrier rate ([run](#real-device-run-tp35-2026-11-13)) |
| 5 | Upsampling with Sonder or Roon tables, no clipping, image rejection; PCM→DSD plays as DoP | `resampler_rejects_images_by_more_than_100_db`, `pcm_to_dsd64_reaches_the_device_over_dop`, `pcm_to_dsd128_and_dsd256_use_the_wider_carriers`, `pcm_to_dsd_uses_native_words_when_the_device_prefers_them`, `sdm_output_decodes_to_the_input_tone` | S/PDIF loopback capture or a DTS/HDCD-flagged file the DAC recognises only when untouched: **not yet run**; PCM→DSD on the TP35, 2026-11-13: DSD64/5th, DSD128/5th and DSD128/7th play a clean tone over DoP with 0 underruns; DSD256 falls back to PCM because DoP needs a 705.6 kHz carrier and the TP35 stops at 384 kHz ([run](#real-device-run-tp35-2026-11-13)) |
| 6 | PEQ, levelling, TPDF dither selectable and measured | `peq_matches_the_rbj_response`, `dither_replaces_truncation_distortion_with_noise`; phase 7 unit tests for levelling and dither | listening test with an EQ boost: **not yet run** |
| 7 | Signal path visible in the UI | `the_signal_path_lists_every_active_stage`; `PlayerSnapshot.signal_path` feeds the Now Playing panel; `cargo test -p sonder-verify` and the UI type check | panel inspected by hand: **not yet run** |
| 8 | Roon filter files absent from git and release bundles | `scripts/check-forbidden-files.sh` (CI), `the_repository_has_no_roon_filter_files`, `pwsh tools/check_bundle.ps1` | `pwsh tools/check_bundle.ps1 -Build`, 2026-11-13: both bundles (`Sonder_0.1.0_x64_en-US.msi`, `Sonder_0.1.0_x64-setup.exe`) built and the MSI unpacked; "Bundle check passed: no Roon filter files" |

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

## Quality check

`cargo test -p sonder-quality` checks every verdict on synthesized files (random-phase noise
shaped in the frequency domain, no real encoder): full-band noise → Genuine/High; brick walls at
16, 19.5 and 20 kHz → Transcoded, and with holes plus folded stereo → Transcoded/High; a wall at
21.1 kHz → Suspect/Low; 16-bit samples padded to 24 → Fake bit depth; 96 kHz walled at 22 kHz →
Fake hi-res; −80 dBFS or 2 s → Inconclusive; FLAC MD5 pass, missing and corrupted; a DSF file
through the DSD→PCM path. `cargo test -p sonder-library --test quality` covers the cache.

Real files, release build, 2026-11-14, same laptop as above (`sonder-cli check`):

- 10 tracks of a 24/96 piano album: all Genuine/High, 1.1–2.8 s per 3–6 min track.
- 120 random files from a personal library (FLAC and ALAC, 16/24-bit, 44.1–96 kHz): 70
  Genuine/High, 49 Genuine/Medium (content fading out gently below 21.5 kHz, edge moving), 1
  Suspect/Low (hard, fixed wall at 21.4 kHz). No reference verdicts exist for these files, so
  this shows the spread and the false-alarm rate on material believed genuine, not accuracy.
- An earlier build flagged sparse-treble piano and hi-res ultrasonic noise as "holes"; holes are
  now only counted in frames whose top edge sits in lossy territory (≤ 20.5 kHz).

Not yet run: real MP3/AAC → FLAC transcodes at 128/192/256/320 kbps (needs an external encoder
and files the user owns), to confirm the thresholds outside synthetic signals.

## Real-device run: TP35, 2026-11-13

Device `[2] Speakers (TP35)`, WASAPI exclusive, PCM 44.1–384 kHz at 16, 24 (packed) and 32
bits; Windows 11 26300; release `sonder-cli`. Test files from
`cargo run -p sonder-verify --example hardware_fixtures -- <folder>` (1 kHz sine at −20 dBFS,
10 s; FLAC written with verbatim frames, checked by
`hardware_fixture_flac_decodes_to_the_written_samples`). The user listened and watched the
DAC for the DSD rows.

Bit-perfect, `sonder-cli play <file> --device 2 --verify-hash`:

| File | Device format | Decoded = device SHA-256 | Underruns | Result |
|------|---------------|--------------------------|-----------|--------|
| FLAC 16/44.1 | 44.1 kHz, 16-bit | `87cfb57b…79bdecf4` | 0 | PASSED |
| FLAC 24/96 | 96 kHz, 24-bit packed | `8d45daa0…3d35f01a` | 0 | PASSED |
| FLAC 24/192 | 192 kHz, 24-bit packed | `f0005c18…56fe502f` | 0 | PASSED |
| WAV 24/192 (same samples) | 192 kHz, 24-bit packed | `f0005c18…56fe502f` | 0 | PASSED |
| FLAC 16/44.1 with `--volume -0.1` | 44.1 kHz, 24-bit packed | decoded `9e11e15f…`, device `35bc0e17…` | 0 | FAILED, as required |

DSD, user observation:

| Command | Device stream | What the user heard and saw |
|---------|---------------|-----------------------------|
| DSF DSD64 (music), `--dsd-mode dop` | DoP, 176.4 kHz / 32-bit | Clean music; LED light blue |
| Same DSF, `--dsd-mode pcm --dsd-pcm-rate 176400` (control) | PCM 176.4 kHz / 24-bit | LED light blue |
| Sine 16/44.1, plain PCM (control) | PCM 44.1 kHz / 16-bit | LED red |
| Sine, `--pcm-to-dsd dsd64 --sdm-order 5` | DoP, 176.4 kHz | Clean tone; 0 underruns |
| Sine, `--pcm-to-dsd dsd128 --sdm-order 5` | DoP, 352.8 kHz | Clean tone; LED purple; 0 underruns |
| Sine, `--pcm-to-dsd dsd128 --sdm-order 7` | DoP, 352.8 kHz | Clean tone; LED purple; 0 underruns |
| Sine, `--pcm-to-dsd dsd256 --sdm-order 7` | PCM 44.1 kHz, with "the device does not carry DSD256 as DoP or native DSD; playing PCM" | Clean tone; LED red |

The TP35's LED shows the stream rate (red 44.1 kHz, light blue 176.4 kHz, purple
352.8 kHz) and has no DSD indicator: DoP and the PCM control at the same rate light it the
same. That DoP is decoded rests on the sound. A DAC that does not decode DoP plays the marker
bytes and the raw bitstream as PCM, which is loud hiss over the music. DSD256 over DoP cannot
run on this DAC; the order-7 modulator was covered at DSD128, and the DSD256 cost is in
[Measured results](#measured-results).

Release app (`target/release/sonder.exe`) on the TP35, the user listening:

| Setting | What happened |
|---------|---------------|
| Crossfade 4 s, Next between tracks of different albums and different output formats | No fade; the signal path said "crossfade skipped: different output format", as specified |
| Crossfade 4 s, Next between two same-format tracks of different albums | Smooth fade, no clicks or dropouts |
| Convolution with `dirac-44100.wav` (fixture example), toggled on and off during a 44.1 kHz track | Sounds identical either way |

## Real-hardware checklist (manual)

Windows (WASAPI exclusive):

0. Test files: `cargo run -p sonder-verify --example hardware_fixtures -- <folder>` writes the
   tones and Dirac responses below into a folder outside the repository.
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
