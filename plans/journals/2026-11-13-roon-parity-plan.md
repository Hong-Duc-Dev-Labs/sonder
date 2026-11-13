---
title: "Roon parity plan: SDM speed, convolution/crossfeed, repeat/crossfade, playlists, DAC checks"
date: 2026-11-13
summary: "Compared Sonder with Roon, scoped four gaps with the user, and wrote a five-phase plan."
---

# Roon parity plan

## What happened

A feature comparison against Roon showed Sonder close on the signal chain but missing
playlists, repeat, crossfade, convolution and crossfeed, with PCM→DSD256 below realtime and no
real-device evidence yet. The user chose items 1–4 (DAC verification, queue features,
convolution/crossfeed, SDM speed) and left online metadata and remote control out.

Plan: `plans/261113-0656-roon-parity-playback-features/`. Contract:
`plans/reports/brainstorm-261113-1354-roon-parity-playback-features.md`.

## Decisions

- User decisions: the user watches the TP35 DAC during the DoP/DSD steps; crossfade is
  skipped between consecutive tracks of the same album; convolution takes WAV IRs only.
- SDM: rework the single-thread loop first (const-generic order, bytes built in a register,
  cheaper stability check, interleaved stereo); add per-channel threads only if the bench
  still misses ≤ 0.66 s per second of DSD256/7 audio. Today: 1.19 s, ~44–52 ns per bit per
  channel, which is overhead, not the loop's arithmetic.
- Playlist items reference `tracks.path`, not `tracks.id`, so entries survive a rescan.
- Chain order after the work: headroom → SRC → convolution → PEQ → crossfeed → levelling →
  volume → dither (→ SDM).

## Risks carried forward

- Crossfade needs two live decoders in `player.rs` — the largest change.
- TP35 DoP support is unknown; a refusal is recorded as the DSD→PCM fallback.
