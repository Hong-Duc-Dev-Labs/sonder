# Running Lyrics Effect & Folia Flow Stage Plan

**Date:** 2026-11-17  
**Scope:** `plans/261116-1718-running-lyrics-flow`

## Context & User Scope

Following the feature analysis of `egoist/mygo-music` via `/ak-xia`, the user requested an implementation plan covering:
- **a (Timing Engine)**: Client-side word-by-word timing calculation (`spreadWords` & `prepareTimedLines`) in `lyrics.ts`.
- **b (Karaoke LyricsPanel)**: Real-time word-by-word running glow and 3-dot interlude breathing indicator in `LyricsPanel.svelte`.
- **c (Stage Visualizer)**: A full-screen lyric stage in `NowPlayingView.svelte`, specifically restricted to the **Flow** style from Folia-major / mygo-music (omitting Steps and Tilt).

## Decisions

1. **Client-side `spreadWords` synthesis**:
   - Rather than requiring backend database or schema changes to support syllable-level KRC/TTML, Sonder calculates word-level timings on the fly when loading lyrics.
   - 85% singing duration, 15% breathing pause.
   - CJK runes split individually; spaced languages split on spaces.
2. **GPU & Svelte 5 Performance**:
   - Active line renders words in individual spans only when active.
   - Transitions use GPU-friendly CSS properties (`transform`, `opacity`, `text-shadow`) to guarantee 60+ FPS without layout thrashing.
3. **Folia Flow Style**:
   - Critically-damped spring physics (`springStep`), deterministic pseudo-random scatter (`lineRandom`), ambient accent glow, gentle drift, and 3-dot interlude breathing dots.
   - Integrated as a selectable `coverStyle: "stage"` in `NowPlayingView.svelte`.

## Artifacts Created
- `plans/261116-1718-running-lyrics-flow/plan.md`
- `plans/261116-1718-running-lyrics-flow/phase-01-timing-engine.md`
- `plans/261116-1718-running-lyrics-flow/phase-02-lyrics-panel-karaoke.md`
- `plans/261116-1718-running-lyrics-flow/phase-03-lyric-stage-flow.md`
