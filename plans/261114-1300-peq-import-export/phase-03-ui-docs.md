---
phase: 3
title: "Phase 3: Settings UI and docs"
status: completed
priority: P2
effort: "0.3d"
dependencies: [2]
---

# Phase 3: Settings UI and docs

## Requirements

- [x] PEQ card actions row (`SettingsView.svelte`): **Import…** and **Export…** buttons beside
      Add band / Save preset. Export is disabled when the EQ has no bands and a 0 dB preamp.
- [x] Import: on success save `peqPresets[name] = peq`, `peq = peq`, `peqPreset = name` in one
      `saveSettings`; if `name` already exists, ask (existing `app.ask` dialog) to replace or
      keep both (`name (2)`). Notice: "Imported <name>: N bands" plus warnings, if any, in the
      same notice.
- [x] Export: notice with the saved file name.
- [x] Errors go through `app.run` (existing error notice).
- [x] Helper line under the card: "Import a ParametricEQ.txt from AutoEQ or Equalizer APO."

## Files

- Modify: `app/src/components/SettingsView.svelte`, `README.md` (Settings worth knowing:
  one bullet for EQ presets, import and export).

## Validation

- `pnpm check`, `pnpm test`, `pnpm build` — pass.
- `pnpm tauri dev`: import an AutoEQ file → preset appears in the Preset select, bands match
  the file; import the same file again → replace/keep prompt; export → file opens in a text
  editor with the expected lines; re-import it → identical EQ. **Not run:** the native file
  dialogs need a real window and a human click; the parse, naming and round-trip halves are
  covered by `crates/sonder-dsp/src/peq_apo.rs` and `app/src-tauri/src/dto.rs` tests.

## Risk / rollback

UI only; revert the commit.
