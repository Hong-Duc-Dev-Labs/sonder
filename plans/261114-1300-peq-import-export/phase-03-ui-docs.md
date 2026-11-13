---
phase: 3
title: "Phase 3: Settings UI and docs"
status: pending
priority: P2
effort: "0.3d"
dependencies: [2]
---

# Phase 3: Settings UI and docs

## Requirements

- [ ] PEQ card actions row (`SettingsView.svelte`): **Import…** and **Export…** buttons beside
      Add band / Save preset. Export is disabled when the EQ has no bands and a 0 dB preamp.
- [ ] Import: on success save `peqPresets[name] = peq`, `peq = peq`, `peqPreset = name` in one
      `saveSettings`; if `name` already exists, ask (existing `app.ask` dialog) to replace or
      keep both (`name (2)`). Notice: "Imported <name>: N bands" plus warnings, if any, in the
      same notice.
- [ ] Export: notice with the saved file name.
- [ ] Errors go through `app.run` (existing error notice).
- [ ] Helper line under the card: "Import a ParametricEQ.txt from AutoEQ or Equalizer APO."

## Files

- Modify: `app/src/components/SettingsView.svelte`, `README.md` (Settings worth knowing:
  one bullet for EQ presets, import and export).

## Validation

- `pnpm check`, `pnpm test`, `pnpm build`.
- `pnpm tauri dev`: import an AutoEQ file → preset appears in the Preset select, bands match
  the file; import the same file again → replace/keep prompt; export → file opens in a text
  editor with the expected lines; re-import it → identical EQ.

## Risk / rollback

UI only; revert the commit.
