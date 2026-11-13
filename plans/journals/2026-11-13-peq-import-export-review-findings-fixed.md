---
title: "PEQ import/export: review findings fixed"
date: 2026-11-13
summary: "Error detail from the parser, a Windows-safe export file name, an honest import notice, duplicate-key warnings and a preamp slider that reaches -60 dB."
---

# PEQ import/export: review findings fixed

## What happened

`/ak-code-review` on the uncommitted PEQ import/export work produced ten findings, three
medium. Verified each before touching code: a throwaway crate showed `Filter 1: PK Fc 100 Hz …`
(no ON) erroring with "no Preamp line and no Filter line" though the line was right there, and
the vendored `rfd-0.17.2` source showed `save_file` folding every builder error into
`run(self).ok()`, so a rejected default file name is indistinguishable from Cancel.

Equalizer APO's own sources settled the review's open question: `filters/BiQuadFilterFactory.cpp`
reads `Gain` for every filter type and explicitly ignores it for low/high-pass, notch and
all-pass, and unmatched keywords are never searched — so exporting `Gain`/`Q` on pass lines is
APO-valid. The same file showed a fidelity gap the plan accepted unknowingly: APO's no-Q shelf
fallback is slope S = 0.9/12 = 0.075, much gentler than the 0.707 Sonder stores, and APO's `LS`
is a corner frequency while `LSC` is a centre.

## Fixed

- `export_file_name` (dto.rs) replaces the characters Windows forbids, strips trailing dots and
  spaces, caps the length and falls back to "Sonder"; the command uses it for `set_file_name`.
- `ApoError` gained an optional detail: a file whose lines were present but unusable now says
  "no usable Preamp or Filter line" and the first three skip reasons; `NOT_PARAMETRIC` is
  reserved for files with no such line at all.
- Repeated `Preamp`/`Fc`/`Gain`/`Q`/`BW` keys warn and keep the last value instead of changing
  the EQ silently.
- The import notice waits for `saveSettings` to return the saved settings (the helper now
  returns them); the replace/keep prompt names the name actually used; long warning lists show
  the first three plus "and N more"; the preamp slider spans −60…+12 dB like the model, so an
  imported −30 dB preamp is not moved to −24 on first touch.
- The `,` message now covers thousands separators, and the plan's "fixed decimal places" row
  matches the shortest round-trip form the code writes (fixed decimals would break the round
  trip).

## Verified

`cargo test -p sonder-dsp` 80 pass (10 new parser cases incl. error detail and duplicates),
`cargo test -p sonder-app` 16 pass (new `export_file_name` test), clippy `-D warnings` and
`fmt --check` clean, `pnpm check`/`test`/`build` pass. Not run: the native-dialog click-through,
still the plan's open item.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
