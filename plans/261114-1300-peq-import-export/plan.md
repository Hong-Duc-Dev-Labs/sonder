---
title: "Import and export parametric EQ as Equalizer APO text (AutoEQ ParametricEQ.txt)"
description: "Read and write the Equalizer APO parametric format AutoEQ publishes, so a headphone correction becomes a Sonder EQ preset in one click, and Sonder presets can be used elsewhere."
status: pending
priority: P2
effort: "~1 day"
tags: [dsp, eq, settings, tauri]
created: 2026-11-14
---

# Parametric EQ import / export

## Outcome

In `Settings → Parametric EQ`, **Import…** reads an Equalizer APO parametric file (the
`ParametricEQ.txt` AutoEQ publishes per headphone) and **Export…** writes the current EQ in the
same format. An import becomes a named, saved preset and is applied at once.

## Constraints

- The webview keeps no file or dialog access (`app/src-tauri/capabilities/default.json`):
  picking, reading and writing happen in Rust commands, like `settings_pick_impulse_response`.
- Parsing and formatting live next to the EQ model in `crates/sonder-dsp/src/peq.rs`, unit
  tested there; the result goes through `Peq::sanitized()` like every other EQ.
- No engine change: Sonder's biquads follow the RBJ cookbook with Q for shelves
  (`crates/sonder-dsp/src/biquad.rs`), the same meaning APO gives `LSC`/`HSC`.
- `src/lib/bindings.ts` is generated: regenerate with `SONDER_UPDATE_BINDINGS=1`, never edit.

## Non-goals

AutoEQ database or online lookup by headphone name; GraphicEQ.txt; per-channel APO configs;
preset rename; per-device preset switching; response graph. (Possible later steps.)

## Format accepted (Equalizer APO)

| Line | Meaning |
|---|---|
| `Preamp: -6.2 dB` | preamp |
| `Filter 3: ON PK Fc 105 Hz Gain 3.4 dB Q 0.70` | band; `OFF` imports it disabled |
| Types | `PK`/`PEQ`→peak, `LSC`/`LS`→lowShelf, `HSC`/`HS`→highShelf, `LP`/`LPQ`→lowPass, `HP`/`HPQ`→highPass, `BP`→bandPass, `NO`→notch |
| `BW Oct n` instead of `Q` | converted, `Q = √(2ⁿ) / (2ⁿ − 1)` |
| No Q on a shelf or pass (`LS`, `HP`) | Q 0.707 |
| `Fc 1.5 kHz` | kHz accepted |
| Blank, `#` comments, BOM, CRLF | ignored |
| Anything else (`Channel:`, `Include:`, `GraphicEQ:`, `Device:`, unknown type) | skipped with a warning, never a silent change |
| More than 32 bands, values outside Sonder's ranges | truncated / clamped by `sanitized()`, with a warning |

A file with no `Preamp` and no usable filter is an error ("not an Equalizer APO parametric
file"). Export writes `Preamp:` then one `Filter N:` line per band, `ON`/`OFF` from `enabled`,
fixed decimal places, LF line endings; import(export(x)) == x for any sanitized EQ.

## Phases

| # | Phase | Status |
|---|---|---|
| 1 | [Parser and formatter in sonder-dsp](./phase-01-apo-text.md) | Pending |
| 2 | [Tauri commands and bindings](./phase-02-commands.md) | Pending |
| 3 | [Settings UI and docs](./phase-03-ui-docs.md) | Pending |

## Acceptance criteria

- [ ] Importing a real AutoEQ `ParametricEQ.txt` (e.g. a 10-band PK + LSC/HSC file) creates
      preset "<headphone name>" (file stem minus ` ParametricEQ`), applies it, and the PEQ card
      shows the same preamp, types, Fc, gain and Q.
- [ ] Skipped lines and clamps are reported in one notice; a non-APO file is rejected with a
      clear message and changes nothing.
- [ ] Export then import gives back the same EQ; the exported file loads in Equalizer APO's
      syntax (checked against the format table above).
- [ ] Cancelling either dialog changes nothing and shows nothing.
- [ ] `cargo test -p sonder-dsp`, `cargo test -p sonder-app`,
      `pnpm check`, `pnpm test`, `pnpm build` pass; bindings regenerated.
- [ ] README "Settings worth knowing" documents Import/Export.

## Risks

- AutoEQ files vary slightly (`LSC` with or without Q, comments, preamp missing) → tolerant
  parser with fixtures for each shape.
- Locale: APO uses `.` decimals; a `,` decimal is rejected with a message rather than misread.

## Rollback

Revert the phase commits; no settings schema change (presets already exist as
`peqPresets`), so nothing persists past a revert.
