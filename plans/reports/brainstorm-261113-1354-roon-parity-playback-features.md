# Brainstorm — Roon parity: hardware verification, queue features, convolution/crossfeed, SDM speed

Date: 2026-11-13. Scope: items 1–4 of the Sonder-vs-Roon comparison. Items on online
metadata and remote control are out of scope.

## Outcome

1. **Real-device verification** on the attached USB DAC (`Speakers (TP35)`, WASAPI, up to
   384 kHz): `sonder-cli play --verify-hash` passes at 16/44.1, 24/96 and 24/192; the same run
   with `--volume -0.1` fails (proves the harness detects changes); DSF over DoP and PCM→DSD64/128
   are checked against the DAC's display by the user. Results recorded in `docs/verification.md`.
2. **Playlists, repeat, crossfade.**
   - Playlists stored in the SQLite library: create, rename, delete, add tracks, reorder,
     remove, play. A Playlists view and an "Add to playlist" entry in the row menu.
   - Repeat off / all / one, owned by the engine queue so gapless pre-buffering follows it.
   - Crossfade 1–10 s, equal-power, off by default. Skipped between consecutive tracks of the
     same album played in order (user decision). PCM only: never across native DSD/DoP
     tracks; applies before the modulator when PCM→DSD is on. Falls back to a gapless/hard
     transition when the two tracks reach different output formats, and says so.
3. **Convolution and crossfeed** as new PCM stages.
   - Convolution: WAV impulse response only (user decision), 1 channel (applied to every
     channel) or N channels matching the stream; uniformly partitioned overlap-save FFT
     convolution on `realfft` (already a `sonder-dsp` dependency); the IR is resampled to the
     stream rate when it differs.
   - Crossfeed: bs2b-style with presets, stereo only, bypassed otherwise.
   - Chain order: headroom → SRC → convolution → PEQ → crossfeed → levelling → volume → dither
     (→ SDM). Both stages appear in the signal path and mark it "not bit-perfect".
4. **SDM speed:** PCM→DSD256 7th order at ≥ 1.5× realtime on this machine
   (`cargo bench -p sonder-dsp --bench sdm` ≤ 0.66 s per second of stereo audio; today 1.19 s).

## Constraints

- Real-time-safe audio thread: no allocation, locks or I/O in `process`/`flush`/`reset`.
- DSP, crossfade and volume off ⇒ byte-exact output (existing tests keep passing, new ones
  cover crossfade-off and repeat).
- Signal path names every new stage and every fallback (crossfade skipped, IR resampled).
- No Roon data in git or bundles; `scripts/check-forbidden-files.sh` keeps passing.
- Existing settings files must load unchanged (new fields default to off).
- Library schema change goes through the existing migration path in
  `crates/sonder-library/src/schema.rs`.

## Non-goals

Online metadata, remote control/multi-room, a transport shuffle toggle, M3U import/export,
Roon-style `.zip`/`.cfg` convolution configs, procedural EQ, speaker setup, macOS/Linux
hardware runs.

## Acceptance criteria

- Tests: repeat-one/all stay gapless and byte-exact; crossfade off is byte-exact; crossfade
  on mixes with an equal-power curve and is skipped inside an in-order album; convolution
  matches direct-form convolution within 1e-9 and a Dirac IR is identity; crossfeed leaves a
  mono (L = R) signal's level unchanged and attenuates far-channel highs as designed;
  playlist CRUD and ordering in `sonder-library`.
- SDM bench target above met; existing SDM quality tests (SNR, round-trip) unchanged.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `pnpm check` (UI type check) and `bash scripts/check-forbidden-files.sh` pass.
- `docs/verification.md` holds the TP35 results, with anything not run marked so.

## Trade-offs (SDM)

| Approach | Load-bearing assumption | Fails first when |
|---|---|---|
| A. Single-thread loop rework: order as a const generic, bits collected in a register, cheaper stability check, two channels interleaved for ILP (**chosen**) | Most of the ~44 ns/bit/channel is overhead, not the inherent serial loop | The upstream resampler dominates the end-to-end cost |
| B. One thread per channel | Two idle cores and cheap per-block sync | Weak laptops, or sync jitter under real-time load |
| C. SIMD across channels | Channels can run lock-step | Per-channel stability resets diverge lanes |

A is cheapest to abandon; B is added only if A misses the bench target after measurement.

Better approaches: none — recommended direction is the requested one (convolution reuses
`realfft` and the `Stage` trait; crossfade/repeat belong in the engine queue that already owns
gapless decisions in `crates/sonder-engine/src/player.rs`).

## Evidence

- `sonder-cli devices` lists `[2] Speakers (TP35)` with 44.1–384 kHz.
- Chain assembly: `crates/sonder-engine/src/rate.rs:93` (headroom, SRC) and
  `crates/sonder-engine/src/player.rs:1039` (PEQ, levelling, volume).
- SDM hot loop: `crates/sonder-dsp/src/sdm.rs:347` (`process`), `:399` (`step`).

## Risks

- Crossfade needs two decoders live during the overlap; the player (2.3k lines) owns one
  decode worker today — largest change in this set.
- TP35's DoP support is unknown; if it lacks DoP, item 1 records the DSD→PCM fallback instead.
