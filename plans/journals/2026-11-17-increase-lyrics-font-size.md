# Journal: Increase Lyrics Font Size in Lyrics Panel

**Date:** 2026-11-17  
**Task:** `/ak-fix` Increase lyrics font size in `LyricsPanel.svelte` for comfortable, prominent desktop reading.

## Symptoms & Observed Issues
- Following the previous column resizing, the lyrics text size was reduced to `clamp(16px, 1.05cqw + 6px, 22px)` (~17px on desktop), which felt too small and difficult to read when viewing lyrics from a normal desktop viewing distance.

## Solution & Implementation
- **Enlarged Lyrics Line Font Size:**
  - Upgraded `.line` font size from `clamp(16px, ...)` to `clamp(24px, 2cqw + 14px, 34px)` (~24px–28px on typical desktop windows).
  - Increased line padding to `8px 12px` and set line-height to `1.34` for clean readability.
  - Sized active singing line `.line.on` with a subtle scale emphasis (`scale(1.02)`).
  - Scaled unsynced plain lines (`.line.plain`) and gap spacers to `22px`.
  - Expanded right column `max-width` in `NowPlayingView.svelte` to `680px` to naturally accommodate the larger typeface without premature line wrapping.

## Verification
- `pnpm --dir app check`: 0 errors, 0 warnings.
- `pnpm --dir app test`: 13 test files passed, 76/76 unit tests passed.
