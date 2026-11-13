---
phase: 2
title: "Phase 2: Tauri commands and bindings"
status: pending
priority: P2
effort: "0.3d"
dependencies: [1]
---

# Phase 2: Commands

## Requirements

- [ ] `peq_import() -> AppResult<Option<PeqImport>>`: open dialog filtered to `*.txt`, read
      (UTF-8, BOM tolerated, size cap ~256 KB), parse; `None` when cancelled.
      `PeqImport { name: String, peq: PeqSetting, warnings: Vec<String> }`, `name` = file stem
      with a trailing ` ParametricEQ` removed.
- [ ] `peq_export(peq: PeqSetting, name: String) -> AppResult<Option<String>>`: save dialog
      defaulting to `<name> ParametricEQ.txt`, write `to_apo_text`; returns the path, `None`
      when cancelled.
- [ ] Both run blocking work in `spawn_blocking`, mirroring `settings_pick_impulse_response`.
- [ ] Register in the specta builder; regenerate `src/lib/bindings.ts` with
      `SONDER_UPDATE_BINDINGS=1`. Capabilities stay unchanged.

## Files

- Modify: `app/src-tauri/src/commands.rs`, `app/src-tauri/src/dto.rs` (DTO + conversion to
  and from `PeqSetting`), `app/src-tauri/src/main.rs` (registration),
  `app/src/lib/bindings.ts` (generated).

## Validation

- `cargo test` for the app crate (the stale-bindings check in `main.rs` passes), `pnpm check`.
- Manual: cancel both dialogs → no change.

## Risk / rollback

New commands only; revert the commit.
