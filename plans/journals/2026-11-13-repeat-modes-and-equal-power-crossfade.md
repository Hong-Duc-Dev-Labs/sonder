---
title: Repeat modes and equal-power crossfade
date: 2026-11-13
summary: "Engine-owned repeat off/all/one and an equal-power crossfade that skips album runs, with the mixer at the chain's float output"
---

# Repeat modes and equal-power crossfade

## What happened

Implemented phase 3 of the Roon-parity plan: repeat off/all/one owned by the engine queue, and an
equal-power crossfade between queue entries, skipped inside album runs.

Engine:

- `crates/sonder-engine/src/crossfade.rs` (new): pure transition rules (`decide`: repeat-one,
  same track, same album, DSD, different output format, unknown length; length clamped to half of
  the shorter track) and the cos/sin `Fade` mixer that writes into a caller-owned buffer.
- `player.rs`: per-track fields moved into `ChainState` (`take_chain`/`install_chain`), so the
  incoming entry's chain, plan, levelling handle and signal path can be built beside the outgoing
  one. `open_track` split into `begin_chain` + announce. `RepeatMode` + `Command::SetRepeat`;
  `auto_next` repeats/wraps/stops, manual Next wraps under repeat-all. `produce` plans the fade
  once the track is a fade-length from its end, queues the incoming `TrackStart` where the fade
  begins, and `produce_fade` decodes both chains at their float output, mixes a block at a time
  and emits through the incoming chain, so a PCM→DSD pair mixes before the modulator. Manual Next
  flushes the ring to the heard position, rebuilds the outgoing track there and fades the planned
  length. Any `reset_decoding` (seek, stop, stage/device change) cancels a fade; queue edits
  rebuild from the heard position.
- `worker.rs`: `decode_float`, `flush_float` and `emit_float` (PCM conversion, or the
  source chain's `Sdm`/packer).
- `SignalPath.transition` carries the fade length or the skip reason; an active fade adds a
  `Crossfade` stage so the path stops claiming bit-perfection, a skipped one stays a note.

App/UI:

- `RepeatMode` DTO + `player_set_repeat`, snapshot `repeat`; `CrossfadeSetting` in settings
  (1–10 s, default 4, off by default), applied on start and `settings_set`; regenerated
  `bindings.ts`.
- Transport bar: repeat button cycling off → all → one (icons added). Settings: crossfade
  switch and length slider. Signal path panel: transition row. README: crossfade and repeat
  bullets.

## Decision

- The fade runs at the chain's float output (after every stage, before integer conversion or the
  PCM→DSD modulator). The modulator that renders the fade belongs to the incoming track (fresh
  at its first sample); the outgoing track's modulation ends at the fade point. Mixing before
  the SDM is what the plan asked for; moving one modulator across the handover was not needed.
- The fade starts within one decode block (≤ 0.1 s) of its plan and runs to the outgoing track's
  actual end, so a track's ending is never cut. Manual Next uses the planned length exactly.
- A skipped transition is a note (`transition`), not a stage: an album run must keep its
  bit-perfect claim. Only an active fade adds a `Crossfade` stage.

## Verification

- `cargo test --workspace`: all suites green, 68 engine integration tests (3 repeat, 5
  crossfade incl. SDM), 26 engine unit tests (crossfade table and mixer).
- `cargo clippy --workspace --all-targets -- -D warnings`, `pnpm check`, `pnpm test`,
  `pnpm build`, `bash scripts/check-forbidden-files.sh`: clean.
- Headless browser: the transport bar renders the repeat button ("Repeat off", loop icon).
  The Settings view cannot be reached in a plain browser because `app.init` needs the Tauri IPC
  (`transformCallback` fails); the UI is covered by svelte-check and the production build only.

## Next steps

- Phase 4: playlists. Phase 5: real-device (TP35) verification, including the new repeat and
  crossfade behaviour.
- Listening test still open: crossfade across SRC boundaries and inside PCM→DSD at DSD256.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
