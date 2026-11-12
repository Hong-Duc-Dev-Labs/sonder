---
title: "Plan: vinyl turntable Now Playing from Mochy-Music"
date: 2026-11-12
summary: "Studied Mochy-Music's disc/turntable UI (GPL-3.0, Compose); planned a clean-room Svelte port in 5 phases"
---

# Plan: vinyl turntable Now Playing from Mochy-Music

## What happened
Studied how `realidkroo/Mochy-Music` (@01b06ba) draws its vinyl and turntable player and
wrote `plans/261112-0144-vinyl-turntable-ui/` (5 phases) to build the same experience in
Sonder's Svelte UI.

## Findings
- Mochy is Android/Jetpack Compose, so nothing transplants; only behaviour carries over.
- It is GPL-3.0 while Sonder has no license: the plan requires a clean-room port (no code,
  colour tables or geometry tables copied).
- The key performance idea: the record's surface never rotates (circular grooves, fixed
  light); only the label layer turns, so one composited layer animates per frame.
- Its tonearm doubles as the seek bar (angle from progress by the law of cosines).
- It stops the disc abruptly on pause; the plan adds a spring coast-down instead.

## Decisions (user)
Slow decorative spin, tonearm as seek control, vinyl default with a Vinyl/Square setting and a
mini disc in the transport bar, and all four extra motion groups.

## Next steps
Run `/ak-cook plans/261112-0144-vinyl-turntable-ui/plan.md`.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
