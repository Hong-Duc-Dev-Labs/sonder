# Journal: Enlarge Turntable Left Column and Refine Lyrics Proportion

**Date:** 2026-11-17  
**Task:** `/ak-fix` Increase turntable size on the left column and reduce/refine oversized lyrics on the right in Now Playing view.

## Symptoms & Observed Issues
1. In Now Playing view (`activeTab === "lyrics"`), the left turntable column was constrained to `max-width: 440px` and the vinyl record was limited to `400px`.
2. The right column stretched up to 800px+ across wide screens, with lyrics text sized up to 28px (`clamp(20px, 1.3cqw + 10px, 28px)`), making the lyrics wall overpower and dwarf the turntable hero.

## Root Cause
- Grid column specification in `NowPlayingView.svelte` allocated `grid-template-columns: minmax(320px, 440px) 1fr`, giving the right side disproportionately greater weight.
- `LyricsPanel.svelte` used large font clamping with tight `line-height: 1.25`, causing dense and visually domineering text clusters.

## Solution & Implementation
- **Enlarged Turntable Left Column:**
  - Expanded `left-col` `max-width` from `440px` to `520px`.
  - Increased `turntable-box` `max-width` from `400px` to `min(480px, 52vh)` (~44% increase in turntable surface area).
  - Increased `square-art-box` `max-width` from `380px` to `min(460px, 50vh)`.
  - Kept viewport-height bounding (`min(..., 52vh)`) to ensure zero vertical overflow on shorter tiled windows or laptop screens.
- **Balanced Layout Grid:**
  - Adjusted `np-body` grid to `grid-template-columns: minmax(380px, 520px) minmax(360px, 620px); justify-content: center; gap: clamp(36px, 4.5vw, 68px);`.
  - Constrained `.right-col` to `max-width: 620px` to prevent over-stretching.
- **Refined Lyrics Typography:**
  - Tuned `LyricsPanel.svelte` font size to `clamp(16px, 1.05cqw + 6px, 22px)` (down from `28px`).
  - Increased line-height to `1.36` with reduced padding (`5px 10px`) for an airy, elegant, editorial layout.

## Verification
- `pnpm --dir app check`: 0 errors, 0 warnings.
- `pnpm --dir app test`: 13 test files passed, 76/76 unit tests passed.
