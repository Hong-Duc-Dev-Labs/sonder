# Journal: Apply Momo Trust Sans Font Family to Lyric Stage

**Date:** 2026-11-17  
**Task:** `/ak-fix` Apply the global Momo font (`Momo Trust Sans Variable`) to `LyricStage.svelte` and kinetic stage motion measurements.

## Symptoms & Observed Issues
- In the Stage Flow view, text appeared in the generic fallback sans-serif font rather than the custom `Momo Trust Sans` typography used across the rest of the application.

## Root Cause
- Fontsource packages the variable font under the family name `'Momo Trust Sans Variable'` (defined in `@fontsource-variable/momo-trust-sans` and set to CSS variable `--font`).
- `LyricStage.svelte` hardcoded `font-family: "Momo Trust Sans", sans-serif;` without `"Variable"` or `var(--font)`. Because the family name did not match the registered `@font-face`, the browser fell back to system `sans-serif` (Arial / Segoe UI).
- Similarly, `measureWordWidth` in `stageMotion.ts` specified `"Momo Trust Sans"`.

## Solution & Implementation
- Replaced hardcoded font strings in `LyricStage.svelte` with `font-family: var(--font);`, aligning directly with every other view in Sonder.
- Added a `@font-face` alias mapping `"Momo Trust Sans"` to `local("Momo Trust Sans Variable")` in `app.css`.
- Updated canvas measurement font in `stageMotion.ts` to include `"Momo Trust Sans Variable"`.

## Verification
- `pnpm --dir app check`: 0 errors, 0 warnings.
- `pnpm --dir app test`: 13 test files passed, 76/76 unit tests passed.
