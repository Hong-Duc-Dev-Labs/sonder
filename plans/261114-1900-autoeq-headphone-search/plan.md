---
title: "Find a headphone's AutoEQ correction by name"
description: "Search AutoEQ's ~8,800 headphone corrections from Settings and apply one as a PEQ preset in one click, without sending the search anywhere."
status: completed
priority: P2
effort: "~1 day"
tags: [dsp, eq, settings, tauri, network]
created: 2026-11-14
---

# AutoEQ headphone search

## Outcome

`Settings → Parametric EQ → Find headphone…` searches AutoEQ by headphone name, lists each
match with its measurement source (and rig), and a click downloads that result's
`ParametricEQ.txt` and imports it like **Import…**: saved as a preset named after the
headphone (asking before replacing one) and applied at once.

Source idea: the bundled AutoEQ catalogue in Synq 1.5.1
(`plans/reports/xia-261114-synq-feature-comparison.md`). Sonder downloads instead of bundling.

## Constraints

- AutoEQ (github.com/jaakkopasanen/AutoEq) is MIT. Its data is fetched at run time from
  raw.githubusercontent.com and never shipped in the installer.
- Privacy: only AutoEQ's `results/INDEX.md` and the picked file are requested. The query is
  matched locally and never sent.
- Reuse the existing APO parser (`sonder_dsp::Peq::from_apo_text`) and the UI's import flow.

## Non-goals

Bundling the catalogue; GraphicEQ or per-target variants; a response graph; choosing a
target curve.

## Design

- `crates/sonder-autoeq`: parses `INDEX.md` (`- [name](./path) by source[ on rig]`, where the
  path may contain parentheses), searches locally (case and punctuation ignored; exact, then
  model matches, then word matches; oratory1990 and crinacle first, as AutoEQ recommends),
  builds the file URL only from safe relative paths, and caches the index in
  `<data>/autoeq/INDEX.md` for 7 days, falling back to a stale copy offline.
- App: commands `autoeq_search(query)` (≤ 50 results) and `autoeq_import(result)` returning a
  `PeqImport`; `AppPaths.autoeq`; one shared `USER_AGENT`.
- UI: a search panel under the PEQ actions with a 250 ms debounce. `applyImport` is shared
  with file import.

## Acceptance criteria

1. Index lines with parentheses, `&` and rigs parse; unsafe paths yield no URL.
2. "hd 600", "HD-600" and "hd600" find the HD 600; oratory1990 ranks first.
3. The index downloads once, is reused from disk, and a stale copy works offline; no copy and
   no network is a clear error.
4. Picking a result applies its EQ as a named preset; a 404 or non-APO reply changes nothing.
5. `cargo test --workspace`, clippy, `pnpm check`, `pnpm test` pass; the live test
   (`cargo test -p sonder-autoeq -- --ignored`) finds the HD 600 on the real repository.

## Result (2026-11-14)

All rows verified by the tests named above, including the live network test. Not done: a
click-through in the running app.
