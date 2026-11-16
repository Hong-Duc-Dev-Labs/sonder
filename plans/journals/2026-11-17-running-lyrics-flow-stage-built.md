# Running Lyrics Effect & Folia Flow Stage Built

**Date:** 2026-11-17  
**Scope:** `plans/261116-1718-running-lyrics-flow`

## Summary of Completed Work

Successfully ported and enhanced the running lyrics effect from `egoist/mygo-music` into Sonder across 3 completed phases:

### 1. Algorithmic Timing Engine (`app/src/lib/lyrics.ts`)
- Implemented `spreadWords(text, startMs, endMs)`:
  - Automatically recognizes CJK characters (Han, Hiragana, Katakana, Hangul) via Unicode scripts regex and splits them into single runes.
  - Recognizes Latin, Vietnamese diacritics/tones, numbers, and symbols, splitting on whitespace while preserving `trailingSpace` flags to avoid improper word gaps in CJK.
  - Distributes 85% of line time across words weighted by character length, leaving 15% for breath pause before the next line. Guarantees contiguous timing (`word[i].endMs === word[i+1].startMs`).
- Implemented `prepareTimedLines(lines)`:
  - Precomputes word breakdowns and bounds singing duration to 6 seconds or until the next line begins.
- Added comprehensive unit tests in `app/src/lib/lyrics.test.ts` covering English, Vietnamese diacritics, CJK, mixed text, 85% duration distribution, continuous order, and boundary conditions (empty lines, single word, reversed/zero time).

### 2. Karaoke Running Glow & Interludes (`app/src/components/LyricsPanel.svelte`)
- Upgraded `LyricsPanel.svelte` to render the active singing line with word-level breakdown (`.word`).
- Words transition smoothly from idle (`opacity: 0.38`) to lit (`opacity: 1`, glowing text-shadow), with the currently sung word popping subtly (`scale: 1.05`, blooming glow).
- Non-active lines remain plain text to minimize DOM node overhead and maintain 60+ FPS scrolling.
- Added 3-dot breathing interlude (`.interlude`) indicator for intros and musical breaks longer than 4 seconds.
- Fully honors accessibility preferences (`prefers-reduced-motion` / `reducedMotion()`).

### 3. Stage Lyric Visualizer with Folia Flow Style (`app/src/components/LyricStage.svelte`)
- Created `app/src/lib/stageMotion.ts` providing underdamped harmonic oscillator physics (`springStep`), deterministic line randomizer (`lineRandom`), `easeOutCubic`, `clamp01`, and `lerp`, accompanied by unit tests in `stageMotion.test.ts`.
- Built `app/src/components/LyricStage.svelte` strictly adhering to the requested **Flow** visualizer:
  - Deterministic typographic scattering (organic tilt ±2°, subtle translation offsets).
  - Spring-loaded word entrance with luminous bloom driven by the album cover's dominant color (`app.current?.artColors[0]`).
  - Next and previous line peek buttons that support seeking.
  - Breathing 3-dot interlude animation centered on stage during instrumental breaks.
- Integrated `CoverStyle::Stage` into Tauri backend (`dto.rs`), regenerated TypeScript bindings, updated `NowPlayingView.svelte` with a 3-way cycle toggle (`vinyl` -> `square` -> `stage`), and added setting tiles in `SettingsView.svelte` with full English and Vietnamese localization.

### 4. Fullscreen Panoramic Stage Upgrade (egoist/mygo-music Parity)
- Completely freed `LyricStage.svelte` from the square album card box constraints (`width: 86%`, `aspect-ratio: 1`, border radius, box shadows).
- Expanded stage container to full width & height (`width: 100%`, `height: 100%`) with typography scaled to grand cinematic proportions (`clamp(28px, 3.6cqw + 14px, 52px)`).
- Re-architected `NowPlayingView.svelte` when `coverStyle === "stage"` into a dedicated Fullscreen Panoramic layout:
  - Upper & central stage canvas dedicated 100% to running lyrics with wide balance and full-bleed ambient aura.
  - Floating frosted bottom dock hosting mini-cover art, track metadata, seekbar with tabular timestamps, and playback transport.
  - Slide-out lyrics panel drawer accessible on demand via the lyrics tool toggle.

## Verification

- `pnpm --dir app test`: 12 test files passed, 69 tests passed (100% green).
- `cargo test -p sonder-app`: 28 Rust tests passed, including TypeScript bindings sync check.
- `pnpm --dir app check`: 0 errors and 0 warnings.
- `pnpm --dir app build`: Production build succeeded in 1.17s without bundling issues.
