# Remove Sidebar Active Indicator Straight Divider

**Date:** 2026-11-17
**Task:** `/ak-fix bỏ phần divider thẳng ở sidebar giúp tôi`

## 1. Issue & Cause
In commit `6f87ad6`, a vertical straight active indicator pill (`.nav::before` and `.nav.active::before`) was added to sidebar nav items (`width: 3px; height: 20px; border-radius: 999px; background: var(--ink, #fff)`).
This rendered as an unwanted vertical straight divider line `|` on the active item (e.g. `| 🏠 Trang chủ`), cluttering the minimal glass aesthetic of the sidebar pill.

## 2. Solution
Removed `.nav::before` and `.nav.active::before` pseudo-elements in [App.svelte](file:///D:/Dev/Sonder/app/src/App.svelte).
The active state now relies cleanly on `.nav.active`'s soft background (`var(--soft-active)`), primary ink color (`var(--ink)`), and subtle hairline border (`var(--hairline)`), matching Sonder's minimalist design.

## 3. Verification
- `pnpm --dir app check`: 0 errors, 0 warnings.
- `pnpm --dir app test`: 13 test files passed (76 tests).
