---
phase: 1
title: "Phase 1: Docked inspector"
status: in-progress
priority: P2
effort: "1d"
dependencies: []
---

# Phase 1: Docked inspector

## Context

`App.svelte` renders `QueuePanel` in `.queue-slot`, an absolutely positioned overlay
(`z-index 15`, slides in with `slidePanel`). `LyricsPanel` is only rendered inside
`NowPlayingView` when `app.lyricsOpen`. Pages keep text clear of the sidebar with
`--inset-left` (defined on `.shell`, used in `AlbumGrid`, `AlbumsView`, `AlbumView`,
`ArtistView`, `NowPlayingView`, `PlaylistView`, `App.svelte`, `app.css`). Backdrops run to the
window edge.

## Requirements

- [x] New derived state in `lib/app.svelte.ts`: `inspector: "queue" | "lyrics" | null` —
      `"queue"` when `queueOpen`, else `"lyrics"` when `lyricsOpen` and the view is not `now`,
      else `null`. Opening one from outside Now Playing closes the other (toggle helpers
      `toggleQueue()` / `toggleLyrics()`), so the inspector holds one panel at a time.
- [x] On Now Playing: `lyricsOpen` keeps its in-page meaning; the queue keeps the overlay
      card (no docking there — the stage needs the width).
- [x] `.shell` gets `--inset-right`: `0px` when closed; `calc(var(--inspector-w) + var(--gutter) * 2)`
      when docked. `--inspector-w: 340px` token in `app.css` (QueuePanel is 360 px today;
      make the panel fill its slot instead of fixing its own width).
- [x] Docked mode only when the window is ≥ 1280 px wide (`matchMedia`, reactive). Below that,
      the slot keeps today's overlay behaviour and `--inset-right` stays `0px`.
- [x] Every page that pads by `--inset-left` also pads its right side by `--inset-right`
      (content only; backdrops/heroes still bleed to the window edge). Grids re-flow by
      container width, not window width — check `AlbumGrid` column maths.
- [x] The player pill centres within the space between sidebar and inspector.
- [x] Width change animates `--inset-right` with the existing spring easing; under
      `reducedMotion()` it snaps. Avoid animating layout per frame on every grid card:
      transition the inset on the scroll container's padding only.
- [x] Inspector slot renders `QueuePanel` or `LyricsPanel` (one `.glass` surface). Escape
      closes it and returns focus to the toggle that opened it (QueuePanel already does this —
      extend to lyrics).
- [x] Toasts sit left of the inspector when it is docked.
- [x] Glass budget: sidebar, pill, inspector, back button, toasts ≤ 6 surfaces at once.

## Files

- Modify: `app/src/lib/app.svelte.ts`, `app/src/App.svelte`, `app/src/app.css`,
  `app/src/components/QueuePanel.svelte`, `app/src/components/LyricsPanel.svelte`,
  `app/src/components/NowPlayingView.svelte` (use the toggle helpers),
  pages using `--inset-left` listed above, `app/src/components/TransportBar.svelte`
  (pill centring only).

## Steps

1. Add `inspector`, `toggleQueue`, `toggleLyrics`, and a `wide` flag from `matchMedia`.
2. Introduce `--inspector-w` / `--inset-right`; set on `.shell` from `inspector` + `wide`.
3. Replace `.queue-slot` with an inspector slot: docked (in flow on the right, full height
   minus gutters) vs overlay (current styles). Render the chosen panel.
4. Add `--inset-right` padding to each page; verify grids reflow.
5. Re-centre the pill and move toasts.
6. Lyrics panel: confirm it works without NowPlayingView's wrapper (heading, current line
   scroll, no meta duplication) and gets a close button.

## Validation

- `pnpm check`, `pnpm test`, `pnpm build` in `app/`.
- `pnpm tauri dev`: at 1500 px open queue then lyrics on Albums, Album, Artist, Playlist,
  Search, Settings — content reflows, nothing hidden. At 1200 px both are overlays. On Now
  Playing, lyrics in-page, queue overlay. Keyboard: toggle, Escape, focus return.
- DevTools performance trace while toggling on a 500-album grid: no long frames from layout.

## Risks

- Grid reflow during the spring causes jank → animate padding on one container; fall back to
  a fade with an instant width change if the trace shows long frames.
- `lyricsOpen` persisting across views surprises users → lyrics left open on Now Playing
  show as an inspector when leaving it; accepted (it matches "lyrics from any page").

## Rollback

Revert the phase commit; the overlay queue and Now-Playing-only lyrics return unchanged.
