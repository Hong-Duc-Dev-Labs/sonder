---
phase: 1
title: "Phase 1: Parser and formatter in sonder-dsp"
status: pending
priority: P2
effort: "0.4d"
dependencies: []
---

# Phase 1: Equalizer APO text ↔ `Peq`

## Requirements

- [ ] `Peq::from_apo_text(text: &str) -> Result<ApoImport, ApoError>` where
      `ApoImport { peq: Peq, warnings: Vec<String> }` (already passed through `sanitized()`,
      with a warning for each clamp or truncation).
- [ ] `Peq::to_apo_text(&self) -> String`.
- [ ] Every row of the plan's format table, including `OFF`, `BW Oct`, `kHz`, missing Q,
      comments/BOM/CRLF, unknown lines → warnings, `,` decimal → error.
- [ ] `ApoError` has user-facing messages (no Rust debug text).

## Files

- Modify: `crates/sonder-dsp/src/peq.rs` (or a new `crates/sonder-dsp/src/peq_apo.rs` if
  `peq.rs` grows past a screen; re-export from `lib.rs`).
- Test fixtures inline as string constants (one AutoEQ-shaped file, one hand-written APO file
  with every type, one non-APO text).

## Validation

- `cargo test -p sonder-dsp` with tests: AutoEQ fixture → exact bands; each type mapping;
  BW→Q value; warnings listed; error cases; round trip `from(to(x)) == x` for a sanitized EQ
  using every kind.

## Risk / rollback

Pure functions, no callers yet; revert the commit.
