---
title: Parametric EQ import/export as Equalizer APO text
date: 2026-11-13
summary: "Read and write AutoEQ's ParametricEQ.txt; tolerant parser with warnings, exact round trip, two Tauri commands and Settings buttons."
---

# Parametric EQ import/export as Equalizer APO text

## What happened

`Settings → Parametric EQ` gained **Import…** and **Export…** for the Equalizer APO parametric
text AutoEQ publishes. The work was three slices: a parser/formatter with no callers, two Tauri
commands with a DTO, then the UI buttons and README bullet.

The parser (`crates/sonder-dsp/src/peq_apo.rs`) shares `Peq` and `sanitized()` with the rest of
the EQ rather than introducing a second model: `Peq::from_apo_text` returns
`ApoImport { peq, warnings }` and `Peq::to_apo_text` writes `Preamp:` plus `Filter N:` lines.
The first test run caught a real gap: APO writes the bandwidth unit *before* the value
(`BW Oct 1.0`), while `Fc`/`Gain` put it after. `number_at` now accepts a unit on either side,
or glued to the number (`3.4dB`).

Two smaller choices that tests now pin: a `,` decimal fails the whole import (a `,` would
otherwise be read as two values), and export always writes `Gain`/`Q` even for passes so the
round trip keeps the stored `gain_db`/`q` and stays byte-for-byte stable through `sanitized()`.
Rust's shortest round-trip float `Display` gives fixed notation with no exponent inside
Sonder's ranges, so no manual decimal formatting was needed.

`PeqImport::read` (app crate) caps the file at 256 KB, rejects non-UTF-8 with a message, and
names the preset from the file stem minus a trailing ` ParametricEQ`.

## Decision

- Parser next to the model but in its own module: `peq.rs` stays the data + stage, the text
  format does not grow it.
- Unknown lines, types and keys are skipped line by line with warnings; only clamps, truncation
  and skips warn, so a clean AutoEQ file imports without a notice.
- Import is one `saveSettings` call in the UI (`peq`, `peqPresets[name]`, `peqPreset`), so a
  cancel or a failed command leaves settings untouched. A name collision asks replace-or-keep
  and falls back to `name (2)`.

## Verified

- `cargo test -p sonder-dsp` (78 pass) including 8 new parser tests: AutoEQ shape, every type
  and unit spelling, `OFF`, `BW`→Q, warnings, clamps, truncation, non-APO rejection, comma
  rejection, and an exact round trip through every kind.
- `cargo test -p sonder-app` (15 pass) including naming/UTF-8/rejection for `PeqImport::read`
  and the regenerated `bindings.ts` check; `cargo clippy … -D warnings` and `cargo fmt --check`
  clean.
- `pnpm check`, `pnpm test` (18), `pnpm build` pass.

## Open

The native-dialog click-through (pick a real `ParametricEQ.txt`, replace prompt, export, reopen)
was not run: the dialogs need a real window and a human click, and the dev app currently running
was built before these commands existed. Parse, naming and round-trip halves are covered by
tests; the plan marks that step as not run.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
