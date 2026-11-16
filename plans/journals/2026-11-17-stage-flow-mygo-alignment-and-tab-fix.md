# Journal: Align Stage Flow Typography with MyGo Music & Fix Tab Navigation

**Date:** 2026-11-17  
**Task:** `/ak-fix` Stage flow alignment with egoist/mygo-music, shadow removal, and tab switching resolution.

## Symptoms & Observed Issues
1. **Word Overlap & Clumping:** In Stage flow, words clumped together (e.g. `mấycâuthươngcòntrong`) and shifted words collided on top of one another (e.g. `đang` overlapping with `giữ`).
2. **Muddy Black Shadows:** Heavy drop shadows (`text-shadow: 0 2px 14px rgba(0,0,0,0.88)...`) and dark radial backgrounds caused text to look blurry, muddy, and visually crowded.
3. **Tab Switching Lock:** Clicking "Lời bài hát" or "Hàng đợi" did not switch views; the user was locked in Stage mode.

## Root Cause Analysis
1. **Tab Locking:** In `NowPlayingView.svelte`, an `$effect` continuously evaluated `if (app.settings.coverStyle === "stage" && activeTab !== "stage") activeTab = "stage";`. When `coverStyle` was `"stage"`, any user selection (`selectTab("lyrics")` or `selectTab("queue")`) was immediately overridden on the next microtask.
2. **Word Collision:** In `stageMotion.ts`, margin between words was a hardcoded constant (`16px`). Random displacement offsets shifted adjacent words toward each other by up to 32px without collision avoidance. Furthermore, flexbox collapsed raw whitespace nodes `{' '}`, causing Latin/Vietnamese words to bunch together.
3. **Typography & Shadows:** Font size was capped at an excessively large 76px and wrapped with harsh black text shadows, whereas `mygo-music` uses crisp, shadow-free display typography (`min(64, max(36, w * 0.045))`).

## Solutions & Implementation
- **Ported `spaceWords` Algorithm:** Ported `mygo-music`'s exact `spaceWords` method from `internal/app/ui_stage.go`. Calculates margin dynamically per word:
  `margin = max(2*gap, grow + next + toward + gap) + spaceWidth`
  where `grow` and `next` account for the 1.4x word scale expansion, `toward` compensates for directional random displacement, and `spaceWidth` guarantees natural word separation.
- **Removed All Shadows:** Stripped all `text-shadow` and `box-shadow` from `LyricStage.svelte` and `NowPlayingView.svelte`, matching the minimalist, crisp design of `mygo-music`.
- **Fixed Tab State Reaction:** Refactored the reactive effect in `NowPlayingView.svelte` to track setting changes (`lastCoverStyle`) rather than fighting active user tab clicks.
- **Responsive Font Scaling:** Sized stage typography via `stageFontSize = min(64, max(36, winW * 0.045)) * fontMultiplier`.
- **Minimalist Stage Foot:** Replaced the heavy capsule button with a clean, centered next-line subtitle matching `mygo-music`'s footer.
- **Unit Testing:** Added unit tests verifying collision-free word spacing and layout margin calculations.

## Verification
- `pnpm --dir app check`: 0 errors, 0 warnings.
- `pnpm --dir app test`: 13 test files passed, 76/76 tests passed.
