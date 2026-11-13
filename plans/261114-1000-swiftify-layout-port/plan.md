---
title: "Swiftify-style layout: docked inspector, Home, sidebar and pill"
description: "Port Swiftify's shell layout ideas (docked lyrics/queue inspector, a Home page, grouped sidebar with inline playlists, a fuller player pill) into Sonder's existing glass look."
status: in-progress
priority: P2
effort: "~2.5 days"
tags: [frontend, ui, layout]
created: 2026-11-14
---

# Swiftify-style layout port

## Outcome

Sonder keeps its black canvas, frosted glass and white ink, and gains Swiftify's
layout structure:

1. Lyrics and the queue open as a **docked inspector column** on the right that pushes the
   page narrower on wide windows, and lyrics can be opened from any page.
2. A **Home** page becomes the landing view: greeting, a now-playing hero, shortcuts and
   horizontal shelves.
3. The **sidebar** gains Home and a Playlists group listing playlists inline with a create
   button; the **player pill** gains a Lyrics toggle and, when software volume is on, a
   volume slider.

## Source manifest

- `swiftify-org/swiftify` @ `608860cb5d0013c4328701bd6b33f2c879bcabba` (Qt Quick/QML, Rust core).
- Studied: `assets/home.png`, `assets/immersive.png`, `qml/Main.qml` (sidebar · sheet ·
  inspector), `qml/shell/Sidebar.qml`, `qml/views/HomeView.qml`, `qml/player/PlayerBar.qml`.
- **GPL-3.0; Sonder has no licence.** Layout and behaviour only. No code, shaders,
  constants, strings or assets are copied; every value is Sonder's own.

## Source anatomy (re-expressed)

| Piece | What it does |
|---|---|
| Shell | Floating sidebar · page sheet with a floating player bar centred in it · inspector column on the right |
| Inspector | Lyrics or queue, fixed width; opening it narrows the page instead of covering it; only when the window has room |
| Home | Time-of-day greeting, hero card (now playing / continue), two shortcut tiles, shelves with "See all" |
| Sidebar | Home, Search, labelled Library group, labelled Playlists group with `+` and playlists inline, Settings at the bottom |
| Player bar | Transport left, track + seek centre, lyrics · queue · volume right |

## Dependency matrix

| Need | Local | Status |
|---|---|---|
| Inspector slot | `App.svelte` `.queue-slot` overlay | EXISTS → becomes docked/overlay slot |
| Right inset for pages | only `--inset-left` (16 uses) | NEW `--inset-right` |
| Lyrics outside Now Playing | `LyricsPanel` (rendered only in `NowPlayingView`) | EXISTS, reused |
| Home data | `libraryAlbums(sort Added)`, `libraryPlaylists`, `libraryArtists`, `app.current`, queue | EXISTS |
| Play history, pins, favourites, shuffle | — | ABSENT → out of scope |
| Create playlist | `app` create flow around `playlistCreate` (`lib/app.svelte.ts:336`) | EXISTS |
| Pill volume | `playerSetVolume(db \| null)`, `settings.volumeDb` (null = bit-perfect) | EXISTS; CONFLICT with bit-perfect → gated |

## Decision matrix

| Decision | Swiftify | Sonder |
|---|---|---|
| Look | M3 Expressive tonal purple, wavy lines, shape morphs | **Keep glass / black / white** (accepted in `../261112-0215-mochy-style-ui-refresh/`) |
| Queue placement | Docked inspector | **Docked at ≥ 1280 px, overlay below** (user chose this 2026-11-14; replaces the earlier "overlay, no reflow" queue) |
| Lyrics | Inspector on every page | Inspector on every page **except Now Playing**, which keeps its in-page lyrics |
| Landing view | Home | **Home** (new default) |
| Home shelves | Pinned, playlists, recently played, artists, albums, genres | **Recently added albums, Playlists, Artists** (no new backend) |
| Sidebar playlists | Pinned group + all | **Inline list capped at 8 + "All playlists"**; no pinning |
| Pill volume | Always shown | **Only when software volume is on**; otherwise the bit-perfect status chip stays |
| Shuffle | Yes | No (no backend) |
| Now Playing panel switcher | Segmented Artwork/Lyrics/Queue/Sound | **Not in scope** (user) |

## Risk

Medium (6/10). Shell insets touch every page; one more glass surface on screen when docked
(budget ≤ 6 `backdrop-filter` surfaces); `lyricsOpen` changes meaning outside Now Playing.

## Phases

| # | Phase | Depends | Status |
|---|---|---|---|
| 1 | [Docked inspector](./phase-01-docked-inspector.md) | — | In progress (code done; visual check open) |
| 2 | [Home view](./phase-02-home-view.md) | 1 (insets) | In progress (code done; visual check open) |
| 3 | [Sidebar and player pill](./phase-03-sidebar-and-pill.md) | 1, 2 | In progress (code done; visual check open) |

## Acceptance criteria

- [ ] At ≥ 1280 px, opening queue or lyrics from any library page docks a glass column on the
      right and the page content reflows narrower; nothing is hidden under it.
- [ ] Below 1280 px the same toggles show the existing overlay card.
- [ ] Lyrics toggle works from every page; on Now Playing lyrics stay in-page and no inspector
      appears.
- [ ] App opens on Home; Home shows greeting, hero (or empty state), 2 shortcuts and 3 shelves
      whose "See all" go to Albums / Playlists / Artists.
- [ ] Sidebar shows Home, Library (Albums, Artists), Playlists with `+` and up to 8 playlists,
      Search, Now Playing, Settings; the < 1100 px icon rail still works.
- [ ] Pill has Lyrics and Queue toggles with `aria-pressed`/`aria-expanded`; a volume slider
      appears only when `volumeDb !== null`.
- [ ] `pnpm check`, `pnpm test`, `pnpm build` pass in `app/`; visual check in `pnpm tauri dev`
      at 1500 px, 1200 px and 1000 px widths.

## Rollback

Each phase is one commit on a feature branch; revert the phase commit. No backend, settings
schema or storage changes, so nothing persists past a revert.

## Implementation notes (2026-11-14)

- Code for all three phases is in; `pnpm check`, `pnpm test` (13 tests, incl. the greeting
  boundaries) and `pnpm build` pass. **Open:** the visual check in `pnpm tauri dev` at 1500 /
  1200 / 1000 px and the performance trace while toggling the inspector over a large grid.
- The page's right inset changes at once (no animated reflow); only the inspector slides.
  Chosen up front from phase 1's risk note, because the album grid is virtualised and would
  recompute its columns every frame.
- Shelves page with arrow buttons and Left/Right focus moves instead of turning the mouse
  wheel sideways: hijacking the wheel would trap vertical scrolling of Home. Shift+wheel and
  trackpads already scroll sideways.
- "See all" on Recently added saves the Albums sort as "Recently added", the same as pressing
  that chip on the Albums page.
- Follow-up from the first visual check (user, 2026-11-14): the page is now its own rounded
  sheet between the sidebar and the inspector (three blocks, as in the source), clipping its
  page, with the player pill inside its bottom edge. Now Playing still spans the window.

## Review fixes (2026-11-14)

- **One inspector panel at a time, on Now Playing too.** Two open flags used to be legal there,
  and leaving the page then let the queue win while the pill's Lyrics toggle still reported
  pressed — one click did nothing and the second showed the lyrics. `toggleQueue`/`toggleLyrics`
  now always close the other, so every toggle tells the truth.
- **Escape closes the inspector from anywhere** — except on Now Playing, where the lyrics are
  part of the page rather than the inspector and stay governed by the Lyrics toggle. The panels'
  own keydown only fired with focus inside them; opening from the pill left focus on the toggle.
  `App.svelte` handles Escape on the window (a control that already consumed it keeps it, via
  `defaultPrevented`), `app.closeInspector()` closes the panel and returns focus to its visible
  toggle, and the duplicated per-panel handler is gone.
- **The page-level `--inset-left`/`--inset-right` terms are gone.** Every library page renders
  inside `.content`, which already zeroes both because the sheet is inset as a whole, so the
  terms were always `0px`; padding is `--page-x` plus `--dock-reserve` now. `NowPlayingView`
  keeps `--inset-left` because it lives under `main.full`, where the sheet's inset is not
  applied.
- HomeView defines the `.small` class it was using (the other pages each define their own).
- `Shelf` also observes its row's children, so a card added while Home is open (a new
  playlist) makes the arrows and edge fades appear.
- "See all" on Recently added awaits the sort save before navigating, so Albums does not load
  twice in two different orders.
- Back with an empty history is a no-op, which is the phase-2 sentence in practice: history is
  empty only while the initial Home view shows, so Back has never had anywhere else to go.
  (A fallback in `back()` would have been dead code.)
- `app/src/lib/shell.test.ts` covers the inspector's panel exclusivity and the docked/overlay
  switch; `motion.ts` only registers its visibility listener when a DOM exists, so the state
  module can be imported by tests.

