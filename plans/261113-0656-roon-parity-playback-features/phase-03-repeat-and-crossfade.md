---
phase: 3
title: "Phase 3: Repeat and crossfade"
status: completed
priority: P1
effort: "1-1.5w"
dependencies: [2]
---

# Phase 3: Repeat and crossfade

## Overview

Repeat off/all/one owned by the engine queue, and an equal-power crossfade between PCM tracks
that is skipped between consecutive queue entries of the same album (user decision).

## Key Insights

- The engine owns the queue and the gapless decisions: `continue_after_end`
  (`crates/sonder-engine/src/player.rs:806`), `finish_queue` (`:2166`), `open_track(...,
  continuous)` (`:1689`), `end_index`, and `pipeline_reach` (`:910`) for pre-buffering.
- `in_album_run` (`:1272`) already reads album identity through `LoudnessSource`
  (`TrackGain.album_id`); the crossfade skip uses the same lookup on the two adjacent
  entries.
- Today a single decoder feeds one stage pipeline. Crossfade needs the next track's decoder
  and its own stage pipeline live during the overlap; the mix runs on the decode worker side
  (where `produce`/`pump` run), never in the render callback.

## Requirements

### Repeat
- [x] `RepeatMode { Off, All, One }` command and snapshot field.
- [x] One: the next index after the current track is the current track again, pre-buffered
      gaplessly like any next track. Manual next/previous still move.
- [x] All: after the last entry, continue at index 0 gaplessly.
- [x] Remove/insert/clear while repeating keeps `end_index` and pre-buffer consistent
      (reuse `remap`).

### Crossfade
- [x] Settings: `crossfade: { enabled, seconds (1–10, default 4) }`.
- [x] Eligible transition only when: both tracks are PCM (or both modulated to DSD from PCM,
      mixing before the SDM), both reach the same post-DSP output format, and the two
      entries are not consecutive tracks of the same album. Otherwise the transition is the
      existing gapless/hard one and the signal path says why ("crossfade skipped: same
      album" / "different output format" / "DSD").
- [x] Equal-power curve (cos/sin), overlap = `seconds`, clamped to half of the shorter track.
- [x] Manual next during playback with crossfade on: fade over the same curve (Roon behaviour)
      — only when eligible; otherwise jump as today.
- [x] Repeat-one with crossfade on: no crossfade (same track).
- [x] With crossfade off, the code path is the existing one (no mixer in the chain).

### UI
- [x] Repeat button in `TransportBar.svelte` cycling off → all → one, state from the snapshot.
- [x] Crossfade toggle and seconds slider in `SettingsView.svelte`.
- [x] Commands, DTOs, regenerated `bindings.ts`.

## Related Code Files

- Create: `crates/sonder-engine/src/crossfade.rs` (mixer + eligibility rules, unit-testable
  without a device).
- Modify: `crates/sonder-engine/src/player.rs`, `crates/sonder-engine/src/lib.rs`,
  `crates/sonder-engine/tests/player.rs`, `app/src-tauri/src/{commands,settings,dto,main}.rs`,
  `app/src/lib/bindings.ts` (generated), `app/src/lib/app.svelte.ts`,
  `app/src/components/{TransportBar,SettingsView}.svelte`, `README.md`.

## Implementation Steps

1. Repeat in the engine + tests in `tests/player.rs` on the null backend:
   repeat-one plays the same track twice byte-exact with no gap; repeat-all wraps from the
   last to the first gaplessly; removing the current entry while repeat-all is on continues
   with the right track.
2. `crossfade.rs`: eligibility function (pure, table-tested) and an equal-power mixer that
   writes into a caller-owned buffer.
3. Engine: second decoder + pipeline opened `seconds` before the end when eligible; mix
   during the overlap; hand over the second pipeline as the current one afterwards. Seek
   into the overlap cancels the fade.
4. Tests: crossfade off ⇒ existing gapless byte-exact tests unchanged; crossfade on between
   two tones at different frequencies ⇒ output power stays within ±0.5 dB across the overlap
   and both tones present mid-fade; same-album entries ⇒ byte-exact gapless join; DSF
   neighbour ⇒ no crossfade.
5. UI + settings + bindings; `pnpm check`.

## Todo

- [x] Repeat engine + tests
- [x] Crossfade eligibility + mixer + unit tests
- [x] Engine overlap with second decoder
- [x] Crossfade engine tests
- [x] Transport repeat button, settings UI, bindings
- [x] README

## Success Criteria

- Tests listed above pass; existing `tests/player.rs` gapless tests unchanged and green.
- Signal path shows the crossfade stage when active and the skip reason when not.

**Implementation note.** `RepeatMode` and `CrossfadeConfig` live in `sonder-engine`
(`Command::SetRepeat`, `Command::SetCrossfade`, `PlayerConfig::crossfade`). The queue
advances through `auto_next` (one repeats the entry, all wraps, off stops); manual Next uses
`manual_next` (wraps under all, moves under one) and still jumps when no fade applies.
A queue edit during a fade rebuilds the current track from its heard position, so decoded-ahead
audio of a moved entry is never remapped by stale indices.

`crossfade.rs` holds the pure rules (`decide`: repeat-one / same track / same album / DSD /
different format / unknown length, with the length clamped to half of the shorter track) and the
equal-power `Fade` (cos/sin, first frame outgoing-only, last incoming-only). The engine split
its per-track fields into `ChainState` (`take_chain`/`install_chain`), so the incoming entry's
chain, plan, levelling handle and signal path can be built beside the outgoing one. Near the end
(`decide` length plus one block) the incoming chain is prepared; its `TrackStart` is queued
where the fade begins, and `produce_fade` decodes both chains at the float output
(`DecodeWorker::decode_float`, after every stage and before the integer conversion or the
PCM→DSD modulator), mixes a block at a time and emits through the incoming chain
(`emit_float`), so a modulated pair mixes before the SDM. At the end of the fade the incoming
chain becomes the current one; the modulator and packer state start with that track. Manual
Next flushes the ring to the heard position and rebuilds the outgoing track there, then fades
the planned length. `reset_decoding` (seek, stop, stage changes, device changes) cancels a fade.
The signal path's new `transition` field carries the fade length or the skip reason; an active
fade adds a `Crossfade` stage (so the path stops claiming bit-perfection), a skipped one is only
a note.

Tests:
- `crossfade.rs`: equal-power level within ±0.5 dB over five windows of two uncorrelated tones,
  both tones present mid-fade, endpoints exact, missing-side fades, and the whole decision table.
- `sonder-engine`: repeat-one replays the track byte-exact with no gap; repeat-all wraps from
  the last entry to the first with no gap; removing the playing entry under repeat-all carries on
  with the right track; a fade between two tones holds the level within ±0.5 dB across the
  overlap and leaves the incoming track's tail untouched; same-album entries join byte-exact
  gaplessly with the "same album" note and stay bit-perfect (no `Crossfade` stage); a DSD
  neighbour is not faded, says "DSD" and stays bit-perfect; Next during playback fades into the
  next track and keeps its tail (the test also proves the samples are mixed, not jumped); PCM→DSD
  fades before the modulator and stays on one DoP carrier; a manual Next clamps the fade to half
  of a shorter incoming track; a seek after a skipped transition clears the note; a raw spawn
  config is clamped to the 1–10 s setting; a whole queue of decode failures ends a repeat run.
- App: crossfade settings default off, a file without them loads, and 1–10 s is clamped.

Verification run: `cargo test --workspace` (all suites green; engine: 28 unit + 71
integration tests), `cargo clippy --workspace --all-targets -- -D warnings`,
`pnpm check`, `pnpm test`, `pnpm build`, `bash scripts/check-forbidden-files.sh`.

**Review fixes.** A code review of this phase found defects that this revision repairs:
- A skipped transition was rendered as a `Crossfade` stage (its note starts with "crossfade ")
  and marked the path not bit-perfect. The note is now typed
  (`TransitionNote::{Faded, Skipped}`), so only an active fade adds a stage; the album and DSD
  skip tests assert `stages` and `bit_perfect` (they failed before the fix).
- `produce_fade` mixed the whole fade in one `produce` call, bypassing
  `FEED_AHEAD_SAMPLES`: up to ~113 MB at a 10 s PCM→DSD256 fade, and the device ring (150 ms)
  could starve while the engine handled no commands. It now stops once the feed reaches the
  usual bound and lets `pump` run between blocks.
- Manual Next ignored the decision's length, so the fade could exceed half of a short incoming
  track and cut abruptly when that track ended. It now uses the clamp
  (`manual_next_clamps_the_fade_to_half_the_incoming_track`).
- The decode-failure counter was reset on every successful re-open, so repeat-one/all could
  retry a file that fails mid-stream forever; it is now per entry (`FailureRun`, unit-tested),
  and a failed incoming decoder inside a fade is advanced past instead of installed.
- The outgoing `TrackFailed` named the incoming index under a repeat-all wrap; fade planning no
  longer emits duplicate failure notices; `reset_decoding` clears a stale skip note
  (`a_seek_after_a_skipped_transition_clears_the_note`); queue edits drop a planned fade.
- `PlayerConfig.crossfade` is sanitized at spawn (`spawn_clamps_the_crossfade_length`).
- The outgoing carrier packer's partial frame is handed to the incoming packer at an automatic
  fade, so no DoP/native byte is dropped at the handover.

**Deviations.**
- A planned fade starts within one decode block (≤ 0.1 s) of its plan; the actual length is the
  outgoing track's remaining output, so the track's ending is never cut. Manual Next uses the
  decision's length, capped by what is left of the outgoing track.
- The fade mixes at the chain's float output. For a modulated pair the mix runs before the
  `Sdm`, but the modulator that renders the fade is the incoming track's (fresh at its first
  sample), not a continuation of the outgoing track's loop; the outgoing track's modulation ends
  at the fade point.
- A transition that skipped says why in the signal path's `transition`; only an active fade adds
  a stage, so a skipped one cannot wrongly mark an album run "not bit-perfect".
- At an automatic fade handover the outgoing packer's partial DoP/native carrier frame is adopted
  by the incoming packer, so the byte stream continues without a gap or padding; a manual fade
  rebuilds the outgoing track, so it starts on a fresh byte boundary.
- Not checked in the running Tauri app: the Settings crossfade controls and the Now Playing
  transition row (the plain-browser render cannot reach the Settings view: `app.init` needs the
  Tauri IPC and fails outside the shell). The transport bar's repeat button was rendered in a
  headless browser (label "Repeat off", loop icon). UI type check and production build pass.

## Risk Assessment

- Largest change in the plan: two live decoders in a 2.3k-line player. Keep the mixer and
  rules in `crossfade.rs`; keep `player.rs` changes to orchestration.
- Levelling differs per track: each pipeline keeps its own levelling gain, so the mix sees
  already-levelled signals.
- Album identity comes from `LoudnessSource`; entries without library data (files played
  outside the library) have no album id ⇒ treated as different albums.
