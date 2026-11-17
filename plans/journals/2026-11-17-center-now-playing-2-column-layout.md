# Journal: Center Now Playing 2-Column Layout

**Date:** 2026-11-17  
**Task:** `/ak-fix` Bố cục Now Playing chưa căn giữa, sửa lại cân đối và căn giữa màn hình.

## Symptoms & Observed Issues
In Now Playing view (`activeTab === 'lyrics'`), the two-column presentation (turntable + playback controls on the left, lyrics panel on the right) was visually skewed to the left side of the window:
1. Distance from the left edge of the screen to the left edge of the turntable column was ~260px.
2. Distance from the right edge of the lyrics content to the right edge of the screen was ~570px - 720px, creating an empty, unbalanced void on the right side.
3. The top header down-chevron (which marks 50vw center) pointed into the lyrics column rather than falling in the center gap between the turntable and the lyrics panel.

## Root Cause
1. In `NowPlayingView.svelte`, `.np-body` had `grid-template-columns: minmax(380px, 520px) minmax(360px, 1fr)`.
2. The fractional unit `1fr` on Track 2 absorbed all remaining container width (expanding to 812px inside a 1480px container).
3. Because the tracks consumed 100% of the container width, `justify-content: center` had zero track slack to center the two columns.
4. `.right-col` was capped at `max-width: 680px` without horizontal centering within its 812px cell, keeping it pinned to the left of Track 2 while the remaining ~132px of track space sat empty on the right.
5. As a result, the visual center of the content was shifted ~160px to the left of the true window center.

## Solution & Implementation
- **Equal-Width Symmetrical Columns:**
  - Updated `grid-template-columns` in `.np-body` to `minmax(360px, 520px) minmax(360px, 520px)`.
  - Aligned `.np-body` `max-width` to `1320px` to match `.np-header`'s maximum content width.
  - Set `justify-content: center;` with responsive gap `clamp(32px, 4vw, 64px);`.
- **Balanced Column Containers:**
  - Set `.left-col` to `max-width: 520px; margin: 0 auto;`.
  - Set `.right-col` to `max-width: 520px; margin: 0 auto;`.
  - On standard 1080p desktop (1920px), the left margin to the turntable and the right margin to the lyrics panel are both exactly 408px, with the central gap centered at exactly 50vw (960px) directly aligned beneath the top header chevron.
- **Responsive Adaptations:**
  - Updated the stacked single-column breakpoint to `@media (max-width: 860px)` with `padding: 16px 20px 32px 20px` and centered items (`margin: 0 auto`).
  - Added vertical overflow handling (`@media (max-height: 640px)`) to guarantee smooth scrolling on short windows.

## Verification
- `pnpm --dir app check`: 0 errors, 0 warnings.
- `pnpm --dir app test`: 13 test files passed, 76/76 unit tests passed.
