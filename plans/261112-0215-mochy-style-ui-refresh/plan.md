---
title: "Mochy-style glass UI refresh"
description: "Black canvas, frosted dark glass chrome, white ink and cover colour washing the glass, ported at the idea level from Mochy-Music; plus the backend and accessibility work from the five UI reviews."
status: in-progress
priority: P1
effort: "~5 days"
tags: [frontend, ui, backend, accessibility]
created: 2026-11-12
---

# Mochy-style glass UI refresh

## Overview

Sonder's UI moves from flat tinted panels to Mochy-Music's current look: a black canvas, soft
pools of the playing cover's colours drifting behind everything, floating frosted-glass
chrome (sidebar, player pill, queue, menus, chips, buttons), white ink, and covers that melt
into the controls through a growing blur.

Direction changed on 2026-11-12: the five reviews
([reports](../261112-0144-vinyl-turntable-ui/reports/), `ui-review-1…5`) looked at Mochy's
older Material screenshots. Their structural, accessibility and performance findings still
apply. Their *visual* proposals (tonal Material surfaces, amber accent) are replaced by this
glass system.

## Source manifest

- `realidkroo/Mochy-Music` @ `01b06ba4842e10c3d048d2ca6709b3b65d679786` (Kotlin, Compose).
- Studied: screenshots in `docs/assets/img/` (`app-home`, `app-library`, `app-player`,
  `app-styles`) and the design comments in `ui/component/{GlassKit,FrostedGlass,
  ProgressiveBlur}.kt`, `ui/player/flagship/{CoverFrost,AmbientBackdrop}.kt`.
- **GPL-3.0; Sonder has no licence.** Behaviour and look only. No code, shaders, constants or
  assets are copied; every value below is Sonder's own.

## Source anatomy (re-expressed)

| Piece | What it does |
|---|---|
| Canvas | Pure black; content and heroes fade into it |
| Chrome glass | Floating bars: dark tint over the blurred app, thin white hairline just inside the edge, faint shadow, fine grain so dark blur does not band |
| Page wash | The open page's colour eases over the chrome glass |
| Blur budget | Blur a downscaled picture; cache; a quality setting; solid fallback when blur is off |
| Damage isolation | Things that animate are clipped to their own box so glass above does not re-blur |
| Cover frost | Blur grows from sharp cover to full frost under the title: no hard edge |
| Ambient backdrop | Four soft colour pools from the cover, drifting slowly, drawn small and scaled up, stepped ~30 times a second |
| Ink | White text and icons; controls are bare white glyphs or glass pills |

## Dependency matrix

| Need | Local | Status |
|---|---|---|
| Backdrop blur | CSS `backdrop-filter` (WebView2, WebKit; slow on WebKitGTK) | NEW (`.glass` system) |
| Growing blur | `backdrop-filter` + `mask-image` gradient, stacked bands | NEW |
| Cover colours | `seed_color` in `sonder-library/src/art.rs` (written, not yet stored) | PARTIAL → Phase 1 |
| Ambient pools | canvas painted small, CSS-scaled | NEW |
| Grain | inline SVG noise as a data URI (CSP allows `data:`) | NEW |
| Quality setting | `Settings` | NEW field `glass` |
| Navigation | sidebar | EXISTS → becomes glass |
| Player pill, queue, toasts | flat bars | EXISTS → become glass |

## Decision matrix

| Decision | Mochy | Sonder |
|---|---|---|
| Navigation | Bottom glass pill | **Glass sidebar on the left** (user) |
| Accent | White on black | **White; colour only from the cover washing the glass** (user); Bit-perfect/ok/warn stay fixed |
| Canvas | Black | Black with app-wide ambient pools from the playing cover, strong on Now Playing |
| Blur cost | Downscale, cache, quality levels | `backdrop-filter` only on chrome (≤ 6 surfaces), never on list rows or cards; `glass: full / reduced / off`; off = solid tint; `prefers-reduced-transparency` forces off |
| Per-frame re-blur | Clip animating content | Spinning disc sits on top of glass, not under; `contain: paint` on animated boxes under glass; pill hidden on Now Playing |
| Now Playing | Cover frost into controls | Own hero controls (earlier decision), bare white transport glyphs, frost band between stage and controls |
| Earlier decisions kept | — | Floating pill; Now Playing hides the pill; artist cover and tracks; queue remove; row menu with Play next |

## Phases

| # | Phase | Status |
|---|-------|--------|
| 1 | [Backend: art colours, artist art and tracks, queue edits](./phase-01-backend.md) | Completed |
| 2 | [Glass design system](./phase-02-design-system.md) | In progress (code done; perf trace open) |
| 3 | [Shell: glass sidebar, pill, queue, toasts, settings](./phase-03-shell.md) | In progress (code done; visual check open) |
| 4 | [Library screens](./phase-04-library-screens.md) | In progress (code done; visual check open) |
| 5 | [Now Playing](./phase-05-now-playing.md) | In progress (code done; visual check open) |

Order: 1 → 2 → (3, 4, 5).

## Risk score: Medium–High

- **Blur performance on Linux/WebKitGTK and weak GPUs** (high impact): quality setting with a
  solid fallback, blur limited to chrome, measured in Phase 2 before the screens are built.
- **Legibility of white text on glass over bright covers** (medium): tint strong enough that
  text keeps ≥ 4.5:1 over a white backdrop; checked with a white cover.
- **Queue edits in the engine's gapless bookkeeping** (medium): one index-remap function,
  byte-exact tests.
- **Licence** (controlled): clean-room rule above.

## Success Criteria

- [ ] Sidebar, player pill, queue, menus and toasts are frosted glass over a black canvas
      with cover-coloured ambient light; no flat bordered panels remain.
- [ ] `glass: off` and `prefers-reduced-transparency` give solid surfaces with the same layout.
- [ ] White text on every glass surface keeps ≥ 4.5:1 over a white cover.
- [ ] Scrolling the album grid under the glass holds 60 fps on the dev PC (DevTools trace).
- [ ] Every P1 finding in the five reports is fixed or deferred with a reason.
- [x] `pnpm check`, `pnpm test`, `pnpm build`, `cargo test --workspace`, clippy pass.
- [ ] Layout holds at 900×600 and 1280×820.

## Rollback

Revert the branch. The art-colour migration only adds a nullable column; a database opened by
the new build needs the migrator's backup to go back to an older build.

<!-- slug: mochy-style-ui-refresh -->
