---
title: English and Vietnamese interface
date: 2026-11-14
summary: In-house i18n (no library), language setting in Rust settings, full Vietnamese catalog
---

# English and Vietnamese interface

## What happened
- The UI had every string inline in English across ~30 Svelte components, `format.ts`,
  `greeting.ts` and `app.svelte.ts`.

## Decision
- No i18n dependency: `app/src/lib/i18n.svelte.ts` holds a `$state` locale and `t(key, params)`.
  Components call `t` in markup or `$derived`, so switching redraws without a reload. Option
  lists that used to be `const` arrays became `$derived` for the same reason.
- `locales/en.ts` is the reference; `vi.ts` is `Record<MessageKey, Message>`, so a missing or
  extra key fails `pnpm check`. Plurals are `{ one, other }` picked by `Intl.PluralRules`;
  Vietnamese uses plain strings. Number params go through `Intl.NumberFormat` (keeps the old
  `toLocaleString` thousands separators in the album count).
- The choice is `Settings.language` (`system | en | vi`, default `system`) in `settings.rs`,
  persisted with the other settings; `system` picks the first of `navigator.languages` that is
  `vi` or `en`. Applied in `AppState.init` and after every successful `saveSettings`; also sets
  `<html lang>`.
- `plural()` now takes a fixed noun set instead of a free English word.

## Limits
- Backend text stays English: Rust notices, command errors, signal-path stage names/details,
  native dialog filter names. Translating them needs the language on the Rust side.
- Scan and loudness summaries are stored as text when the event arrives, so they keep the
  language active at that moment.

## Next steps
- Try the app in both languages for layout overflow (longer Vietnamese labels in segmented
  chips, the sidebar and the pill).

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
