---
title: Repair crossfade review findings
date: 2026-11-13
summary: "Skipped transitions stay bit-perfect, the fade respects the feed bound and the track-half clamp, and failure/packer handovers are fixed"
---

# Repair crossfade review findings

## What happened

A code review of the phase-3 repeat/crossfade change found ten defects. This session repaired them
and added regression evidence that failed before the fix.

Before evidence:
- `crossfade_inside_an_album_run_stays_byte_exact` / `crossfade_is_skipped_next_to_a_dsd_track`
  failed: a skipped transition carried `SignalStage { name: "Crossfade", detail: "skipped: …" }`
  and `bit_perfect: false`, because the skip note ("crossfade skipped: …") starts with
  "crossfade " and the stage check used `strip_prefix("crossfade ")`.
- `manual_next_clamps_the_fade_to_half_the_incoming_track` failed with "crossfade 2.7 s" against a
  1 s incoming track (half = 0.5 s): manual Next ignored `decide`'s length.
- `a_seek_after_a_skipped_transition_clears_the_note` failed: `reset_decoding` did not clear
  `pending_transition`, so the restarted track showed the skip note.
- `spawn_clamps_the_crossfade_length` failed with "crossfade 11.1 s": `PlayerConfig.crossfade`
  bypassed `sanitized()` at spawn.

## Changes

- `TransitionNote { Faded, Skipped }` replaces the stringly note; only an active fade adds a
  `Crossfade` stage. Skip tests now assert `stages` and `bit_perfect`.
- `produce_fade` stops at `FEED_AHEAD_SAMPLES` and lets `pump` run; a long fade no longer queues
  its whole length (worst case ≈113 MB at 10 s PCM→DSD256) or blocks commands.
- Manual Next uses the decision's clamp (`config.seconds` capped by half the shorter track) and
  the remaining time.
- `FailureRun` keeps failures per entry and is not reset by a re-open, so repeat modes stop after
  a queue's worth of attempts instead of retrying a mid-stream failure forever. A failed incoming
  decoder inside a fade is advanced past (`in_failed`) instead of installed.
- Outgoing `TrackFailed` uses the outgoing index; fade preparation no longer emits duplicate
  failure notices; queue edits drop a planned fade.
- `Engine::new` sanitizes `PlayerConfig.crossfade`.
- The outgoing carrier packer's partial DoP/native frame is adopted by the incoming packer at an
  automatic fade (`Packer::take_pending`/`adopt_pending`, `DecodeWorker::take_packer_pending`/
  `adopt_packer_pending`), so no DSD byte is dropped at the handover.
- Plan phase file records the review fixes and the new tests.

## Verification

- `cargo test -p sonder-engine`: 28 unit + 71 integration tests green (new: `FailureRun`,
  packer handover, manual clamp, seek clears the note, spawn clamps the length, skip-path
  bit-perfect assertions, manual-next mixing assertion).
- `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --check`, `pnpm check`, `pnpm test`, `pnpm build`,
  `bash scripts/check-forbidden-files.sh` clean.
- Not externally testable: the feed-bound effect (no observable surface), the failed-incoming
  advance (no container produces a reproducible mid-stream error), and the Tauri Settings view
  (needs the real shell).

## Next steps

- Phase 4 (playlists) and phase 5 (TP35 evidence) remain.
- Listening test still open for crossfades across SRC boundaries and PCM→DSD at DSD256.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
