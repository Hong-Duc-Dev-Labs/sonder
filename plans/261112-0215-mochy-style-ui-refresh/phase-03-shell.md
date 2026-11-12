---
phase: 3
title: "Phase 3: Shell — glass sidebar, pill, queue, toasts, settings"
status: in-progress
priority: P1
effort: "1d"
dependencies: [2]
---

# Phase 3: Shell

## Requirements

- [x] **Layout**: content fills the window and scrolls under the chrome; the sidebar and the
      player pill float over it with 12 px margins. Scroll containers reserve padding so the
      last rows clear the pill.
- [x] **Glass sidebar** (left, user decision): `.glass.strong`, 232 px, radius 24; brand mark
      as a small record; "Library" group (Albums, Artists), Search with a `Ctrl F` hint, Now
      Playing with a live mini disc while something plays, Settings pinned to the bottom; the
      lit item is a lighter glass pill with white ink (`aria-current="page"`); collapses to a
      72 px icon rail below 1100 px.
- [x] **Player pill**: `.glass.strong`, centred over the content, 72 px tall; MiniDisc with a
      progress ring; bare white prev / play / next glyphs with round press feedback (40 / 48 /
      40 px targets); accessible seek (20 px hit area, visible focus thumb, ±5 s keys,
      `aria-valuetext`, composited fill); format badges inline at ≥ 1100 px and one status
      chip with a popover below that; queue toggle with `aria-expanded`; hidden (slides away)
      on Now Playing.
- [x] **Queue**: glass card that overlays the content on the right (no reflow); Now playing
      and Up next sections; rows keyed by `entryId`; click or Enter plays; remove button;
      Escape closes and returns focus.
- [x] **Row menu / popovers**: one glass popover component (used by the ⋯ menu and the format
      chip), closes on Escape and outside click, arrow-key navigation.
- [x] **Toasts**: glass pills above the player pill; 28 px dismiss target; timers pause on
      hover and focus; errors in a `role="alert"` region; flip honours reduced motion.
- [x] **Back button**: round glass chip in the page head flow; after navigation, focus moves
      to the new view's heading.
- [x] **Settings** as glass cards: output hero (Bit-perfect switch, volume when off), device
      radio cards, library folders with scan progress (visible under reduced motion),
      Appearance (cover style tiles; glass quality full / reduced / off).

## Success Criteria

- No element in the shell uses a 1 px border as a panel separator.
- At 900×600 nothing in the pill wraps or clips; badges remain reachable through the chip.
- Keyboard only: reach and operate every shell control; Space on a focused button activates it.
