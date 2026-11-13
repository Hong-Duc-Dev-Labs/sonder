---
title: "Import and export parametric EQ as Equalizer APO text (AutoEQ ParametricEQ.txt)"
description: "Read and write the Equalizer APO parametric format AutoEQ publishes, so a headphone correction becomes a Sonder EQ preset in one click, and Sonder presets can be used elsewhere."
status: completed
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
file"), naming the first skipped lines when the file had the right lines but none usable. Export
writes `Preamp:` then one `Filter N:` line per band, `ON`/`OFF` from `enabled`, the shortest
decimal form that reads back exactly (fixed notation, no exponent — fixed decimal places would
break the round trip), LF line endings; import(export(x)) == x for any sanitized EQ.

## Phases

| # | Phase | Status |
|---|---|---|
| 1 | [Parser and formatter in sonder-dsp](./phase-01-apo-text.md) | Completed |
| 2 | [Tauri commands and bindings](./phase-02-commands.md) | Completed |
| 3 | [Settings UI and docs](./phase-03-ui-docs.md) | Completed (native-dialog click-through open) |

## Acceptance criteria

- [ ] Importing a real AutoEQ `ParametricEQ.txt` (e.g. a 10-band PK + LSC/HSC file) creates
      preset "<headphone name>" (file stem minus ` ParametricEQ`), applies it, and the PEQ card
      shows the same preamp, types, Fc, gain and Q. — parse and naming proven by tests
      (`peq_apo::tests::reads_the_autoeq_shape`, `dto::tests::import_names_the_preset_after_the_file`);
      the dialog click-through is open.
- [x] Skipped lines and clamps are reported in one notice; a non-APO file is rejected with a
      clear message and changes nothing. (one warning notice joined from `warnings`; `AppError`
      text; nothing saved before `peqImport` returns)
- [x] Export then import gives back the same EQ; the exported file loads in Equalizer APO's
      syntax (checked against the format table above). (`peq_apo::tests::exports_what_it_reads`
      round-trips every kind; the writer only emits lines the table lists)
- [ ] Cancelling either dialog changes nothing and shows nothing. — both commands return
      `Ok(None)` before writing; not clicked (native dialogs).
- [x] `cargo test -p sonder-dsp`, `cargo test -p sonder-app`,
      `pnpm check`, `pnpm test`, `pnpm build` pass; bindings regenerated.
- [x] README "Settings worth knowing" documents Import/Export.

## Risks

- AutoEQ files vary slightly (`LSC` with or without Q, comments, preamp missing) → tolerant
  parser with fixtures for each shape.
- Locale: APO writes `.` decimals (it rewrites `,` to `.` before parsing); a `,` decimal is
  rejected here with a message rather than guessed, because a `,` is also a thousands
  separator.
- Shelves without Q: APO's own fallback slope is S = 0.9/12 = 0.075, far gentler than the
  0.707 this parser stores (APO `BiQuadFilterFactory`), and APO's `LS`/`HS` mark a corner
  frequency while `LSC`/`HSC` mark a centre (RBJ's f0, which Sonder follows). Both are per this
  plan's table; a re-exported Q-less shelf therefore sounds steeper in APO than a Q-less `LS`
  line did. AutoEQ files always carry Q.

## Implementation Notes

- The parser lives in `crates/sonder-dsp/src/peq_apo.rs` (re-exported as `sonder_dsp::ApoImport`
  and `ApoError`), not in `peq.rs`, which stays the model plus the stage.
- Tolerance details: a unit may follow (`Fc 105 Hz`, `Gain 3.4 dB`) or precede (`BW Oct 1.0`)
  or be glued to the value (`3.4dB`); a missing Q uses 0.707 for every shape, and only the
  clamps and skips produce warnings, so a clean AutoEQ file imports silently.
- A `,` anywhere a number is expected fails the whole import instead of being read as two
  values; unknown lines, types and keys are skipped one line at a time with `line N: …`.
- Export writes `Gain` and `Q` on every band so non-peak shapes keep their stored values and
  the round trip is exact; numbers use Rust's shortest round-trip decimal form (no exponents in
  Sonder's ranges).
- `PeqImport::read` caps the file at 256 KB, rejects non-UTF-8 with a message, and names the
  preset from the stem minus a trailing ` ParametricEQ` (case-insensitive).
- The UI keeps the whole import in one `saveSettings` call; a name collision asks
  replace-or-keep, and keeps both under the next free `name (N)`, which is the name the prompt
  names. The success notice waits for the save to come back, and shows the first three
  warnings plus "and N more".
- The export dialog starts from `<preset> ParametricEQ.txt` with the characters Windows forbids
  replaced (`export_file_name`): `rfd` turns a rejected default name into "no file chosen", so
  an unsanitized name made `Export…` look dead.
- Review fixes: `ApoError` carries the first skipped-line reasons (a file with `Filter` lines
  that all fail no longer claims it has none); repeated `Preamp`/`Fc`/`Gain`/`Q`/`BW` keys warn
  and the last value wins; the preamp slider spans the model's −60…+12 dB, so an imported
  −30 dB preamp is not silently moved to −24 on the first touch.
- Second review pass: `Q` and `BW Oct` now share one slot, so whichever comes last wins
  (Equalizer APO itself always lets a bandwidth win; see the third pass) and a cross-spelling override is reported instead of silent; an unusable
  bandwidth (`0`, negative, overflowing) warns and keeps any explicit Q; a comment after the
  values says to put it on its own line; skipped-line warnings cut the echoed line to 60
  characters (a `GraphicEQ:` line is thousands); a NUL byte means "UTF-16, not UTF-8"; the
  size cap is checked again after reading; the export clamps through `sanitized()` and names
  the file when the write fails. New tests: both `Q`/`BW` orders, unusable bandwidth, a glued
  unit, the inline-comment message, a long skipped line, the size cap, UTF-16 and a lowercase
  ` parametricEq` stem.


- Third review pass: the precedence note now records Equalizer APO's own rule
  (`BiQuadFilterFactory` reads `Q` and then `BW Oct`, so a bandwidth wins whatever the text
  order); this parser keeps the later of the two for a hand-written line and warns when both
  appear, which is a deliberate, reported difference rather than an implied match.

## Rollback

Revert the phase commits; no settings schema change (presets already exist as
`peqPresets`), so nothing persists past a revert.

<!-- slug: peq-import-export -->
