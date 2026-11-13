---
phase: 3
title: "Phase 3: Repeat and crossfade"
status: todo
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
- [ ] `RepeatMode { Off, All, One }` command and snapshot field.
- [ ] One: the next index after the current track is the current track again, pre-buffered
      gaplessly like any next track. Manual next/previous still move.
- [ ] All: after the last entry, continue at index 0 gaplessly.
- [ ] Remove/insert/clear while repeating keeps `end_index` and pre-buffer consistent
      (reuse `remap`).

### Crossfade
- [ ] Settings: `crossfade: { enabled, seconds (1–10, default 4) }`.
- [ ] Eligible transition only when: both tracks are PCM (or both modulated to DSD from PCM,
      mixing before the SDM), both reach the same post-DSP output format, and the two
      entries are not consecutive tracks of the same album. Otherwise the transition is the
      existing gapless/hard one and the signal path says why ("crossfade skipped: same
      album" / "different output format" / "DSD").
- [ ] Equal-power curve (cos/sin), overlap = `seconds`, clamped to half of the shorter track.
- [ ] Manual next during playback with crossfade on: fade over the same curve (Roon behaviour)
      — only when eligible; otherwise jump as today.
- [ ] Repeat-one with crossfade on: no crossfade (same track).
- [ ] With crossfade off, the code path is the existing one (no mixer in the chain).

### UI
- [ ] Repeat button in `TransportBar.svelte` cycling off → all → one, state from the snapshot.
- [ ] Crossfade toggle and seconds slider in `SettingsView.svelte`.
- [ ] Commands, DTOs, regenerated `bindings.ts`.

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

- [ ] Repeat engine + tests
- [ ] Crossfade eligibility + mixer + unit tests
- [ ] Engine overlap with second decoder
- [ ] Crossfade engine tests
- [ ] Transport repeat button, settings UI, bindings
- [ ] README

## Success Criteria

- Tests listed above pass; existing `tests/player.rs` gapless tests unchanged and green.
- Signal path shows the crossfade stage when active and the skip reason when not.

## Risk Assessment

- Largest change in the plan: two live decoders in a 2.3k-line player. Keep the mixer and
  rules in `crossfade.rs`; keep `player.rs` changes to orchestration.
- Levelling differs per track: each pipeline keeps its own levelling gain, so the mix sees
  already-levelled signals.
- Album identity comes from `LoudnessSource`; entries without library data (files played
  outside the library) have no album id ⇒ treated as different albums.
