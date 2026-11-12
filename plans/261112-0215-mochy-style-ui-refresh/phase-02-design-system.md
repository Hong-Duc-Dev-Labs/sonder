---
phase: 2
title: "Phase 2: Glass design system"
status: in-progress
priority: P1
effort: "1d"
dependencies: [1]
---

# Phase 2: Glass design system

## Requirements

- [x] **Canvas and ink tokens** in `app/src/app.css`: `--canvas` black; ink levels as white
      at fixed opacities resolved to solid colours where contrast is computed (primary,
      secondary, faint — faint ≥ 4.5:1 on canvas and on glass); semantic ok/warn/error fixed.
      Amber accent removed; old token names kept as aliases during migration.
- [x] **Shape and type**: radius scale (8 / 12 / 16 / 24 / 28 / pill), type scale (display 44
      to overline 11), Inter Variable bundled via `@fontsource-variable/inter` (dev dependency,
      built into `dist`).
- [x] **`.glass` surface** (one definition, used by all chrome):
      dark tint over `backdrop-filter: blur() saturate()`, a 1 px inner hairline of white at
      low opacity (inset box-shadow so a clip cannot shave it), a soft drop shadow, a grain
      overlay (`::after`, tiny SVG noise data URI, very low opacity) to stop banding, and a
      **wash** layer: `--wash` colour (the page's or the playing cover's seed) mixed over the
      tint at low strength, transitioned with `@property`.
      Variants: `.glass.strong` (chrome), `.glass.soft` (chips, inputs, buttons — tint only,
      **no backdrop-filter**, so lists stay cheap).
- [x] **Quality**: `data-glass="full | reduced | off"` on the shell from `Settings.glass`.
      `reduced` halves the blur radius and drops grain; `off` and
      `prefers-reduced-transparency: reduce` use an opaque tint with the same hairline.
      `@supports not (backdrop-filter: blur(1px))` behaves as `off`.
- [x] **Ambient backdrop** component (`AmbientBackdrop.svelte`), fixed behind the app: up to
      four soft colour pools from the playing track's `artColors`, painted into a small
      canvas (about 1/8 resolution) and scaled up by CSS; positions drift on slow loops
      stepped ~30 times a second through the shared frame loop; colours cross-fade on track
      change; still under reduced motion; low strength app-wide, raised on Now Playing; plain
      black when nothing plays or the cover is grey.
- [x] **Growing blur** utility (`.frost-fade`): two or three stacked `backdrop-filter` bands
      with increasing radius, each masked by a gradient, so a hero image melts into the
      content below without an edge; falls back to a gradient scrim when glass is off.
- [x] **Controls**: `.btn` glass pill (primary = solid white with black ink), `.chip`,
      `.icon-btn` round glass, pill inputs with a leading icon slot, press scale on the spring
      easing; `--ease-spring` defaults to a cubic-bezier with `linear()` under `@supports`.
- [x] **Accessibility basics**: global Space shortcut ignores focused buttons, links and
      sliders and key repeat; focus ring white with a dark outer ring so it shows on glass
      and on covers.
- [ ] **Measure first**: a throwaway page with the album grid scrolling under a `.glass.strong`
      bar; record a DevTools trace at `full` and `reduced`; write the numbers into this file
      before Phase 3 starts.

## Contrast (computed, WCAG 2.1)

| Text | On | Ratio |
|---|---|---|
| ink `#f4f4f6` | canvas `#000` | 19.3 |
| ink-2 `#b8b8c0` | canvas | 10.6 |
| ink-3 `#8f8f99` | canvas | 6.4 |
| ink | glass over a white cover (tint composites to ≈ `#404044`) | 9.5 |
| ink-2 | glass over a white cover | 5.3 |
| ink-3 | glass over a white cover | 3.2 — **not used on glass** |

## Open

- The "measure first" trace was not recorded: it needs the running app with DevTools
  (WebView2 remote debugging). The design keeps blur to chrome surfaces, never cards or
  rows, and the `glass` setting is the escape hatch; the trace is still owed before the
  phase closes.

## Success Criteria

- Contrast table (white ink levels × canvas, glass over black, glass over a white cover) in
  this file, all text pairs ≥ 4.5:1.
- The trace shows no dropped frames while scrolling under one strong glass bar at `full`.
- `pnpm check` and `pnpm build` pass; no network font requests.
