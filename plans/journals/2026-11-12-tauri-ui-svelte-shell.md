---
title: "Tauri UI: Svelte shell over engine and library"
date: 2026-11-12
summary: "sonder-app (Tauri 2 + Svelte 5) built; engine gained play-index and live device switch; installers build on windows-gnu"
---

# Tauri UI: Svelte shell over engine and library

## What happened
Built phase 4: `app/` (Svelte 5 + Vite, picked by the user) with `app/src-tauri` adapting the
engine and library to 30 typed commands and 7 events through `tauri-specta`. Screens: Albums
(virtualised, paged), Album, Artists, Artist, Search, Now Playing (source → device format,
bit-perfect badge), Queue panel, Settings (roots, scan progress, device, volume or bit-perfect).
`pnpm tauri build` produces an MSI and an NSIS installer on Windows. All workspace tests,
clippy `-D warnings`, fmt and `svelte-check` pass.

## Findings
- The engine had no way to start a given queue entry or change device; added `PlayIndex` and
  `SetDevice`. Reading the position before the old render thread exits drops or repeats a few
  frames; stopping the stream first makes the switch byte-exact (tested).
- WASAPI probing uses COM in multithreaded mode, which the Tauri main thread (STA) cannot
  enter: app state opens on a helper thread and every command is `async`.
- Probing a device Sonder holds exclusively can return no formats; the device list keeps the
  last formats seen per device.
- `tauri-specta` rc.28 splits each type into Serialize/Deserialize variants by default;
  `disable_serde_phases()` gives one type per Rust type. A test fails on stale bindings.
- windows-gnu: the linker warns ".rsrc merge failure: multiple non-default manifests" (MinGW's
  default manifest plus Tauri's). The app starts and installers build; dialogs not yet tested.
- The first release build died with `STATUS_DLL_INIT_FAILED` during a session shutdown; a rerun
  passed unchanged.

## Next steps
- Manual click-through on a real library and DAC (add root → scan → play → seek → next →
  switch device), including the folder picker on the GNU build.
- First CI run with the new UI build and WebKitGTK steps.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
