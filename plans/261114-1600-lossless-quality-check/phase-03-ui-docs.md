# Phase 3 — UI and docs

## Files

- Create `app/src/components/QualityReportDialog.svelte`, `app/src/components/QualityBadge.svelte`,
  `app/src/lib/quality.ts` (+ `quality.test.ts`: verdict → label/tone mapping, dB → colour ramp)
- Modify `RowMenu.svelte` (Check quality), album / playlist page ⋯ menus (Check album /
  Check playlist), `SongsView.svelte` (Check all songs), `TrackList.svelte` (badge column),
  `app/src/lib/app.svelte.ts` (batch state + event listeners), `locales/en.ts`, `locales/vi.ts`
- Modify `README.md` (new "Quality check" section), `docs/verification.md` (criterion row)

## Report dialog (reuse `Dialog.svelte`)

1. Header: verdict chip (colour + word, never colour alone), confidence, format line
   (e.g. `FLAC · 24-bit / 96 kHz · 4:12`).
2. Evidence rows: status mark + criterion name + measured value + one-sentence reason from
   the catalog. NOT MEASURED is outlined, not filled. MD5 row always adds "proves the file is
   intact, not that the source was lossless".
3. Spectrum: SVG polyline on a log-frequency axis (kHz ticks), dashed line at the cutoff and
   at 20 kHz / Nyquist.
4. Spectrogram: `<canvas>` from the `u8` grid with a fixed dark well in both themes, cutoff
   track drawn over it; caption explains a flat lid vs a ragged edge.
5. Opening shows the cached summary at once (if any) and starts a fresh check for the
   spectrogram with a progress bar; closing cancels it.

## Batch

Progress strip (like the loudness analysis notice): `Checking 3 / 12 — <title>` with Cancel;
on end, a notice summarising counts per verdict. Badges update from `QualityChecked`.

## Validation

`pnpm check`, `pnpm test`, then `pnpm tauri dev`: check a track, an album, cancel a batch,
switch language to Tiếng Việt and confirm every string is translated.
