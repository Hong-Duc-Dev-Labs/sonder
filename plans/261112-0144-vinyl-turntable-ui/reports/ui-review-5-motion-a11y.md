# UI review 5: motion, interaction states, accessibility, render performance

Scope: the files named in the brief, read statically. No build or screen-reader run was done, so the items marked "verify" need a check on a real device.

**Outcome.** The motion foundation is solid: one shared rAF loop, a fixed-step spring, transform-only writes, and reduced-motion checks in the JS transitions. The serious problems are keyboard and screen-reader paths. Space toggles playback while a focused button has keyboard focus. Track and queue rows cannot be played from assistive tech. One token pair fails AA, and so does the seek bar. Then come four real bugs in the tonearm and the vinyl disc, and three per-frame costs that grow with screen size and refresh rate.

## P1: bugs and accessibility failures

1. **Space is taken over on focused controls.** `App.svelte:44-47` excludes only `input, select, textarea`. With focus on a nav button, a track row, "Add folder…", "Clear" or an album card, Space calls `preventDefault()` (so the button never activates) and toggles playback instead. Fix:
   `if (target.closest('input, select, textarea, button, a, [role="button"], [role="slider"], [contenteditable]')) return;`
   The focused play button still works because Space activates it natively.
2. **Track rows cannot be played from a screen reader, and queue rows cannot be played at all without a mouse.** `TrackList.svelte:26-31` acts only on `ondblclick` and Enter keydown. NVDA and VoiceOver send a synthetic `click`, which does nothing here. `QueuePanel.svelte:21-26` has no key path at all. Fix (both files): `onclick={(e) => e.detail === 0 && onplay(i)}` and drop the `onkeydown`. Enter and Space on a `<button>` produce a click with `detail 0`. Replace `title="Double-click to play"` with `aria-label="{title}, {artist}. Double-click or press Enter to play"`, and add `aria-current={playing ? "true" : undefined}`.
3. **The album card is a `role="button"` with a `<button>` nested inside it.** `AlbumCard.svelte:21-30`. Children of `role=button` are presentational, so screen readers may hide "Play album". Space does nothing, and with the global handler it toggles playback. The play overlay appears only on hover or on its own focus. Fix: make the card a real `<button class="card" aria-label="{title} by {artist}">` and make Play a sibling inside a positioned wrapper, not a child. Add `.card-wrap:focus-within .play { opacity:1; transform:none }` and an explicit `.card:focus-visible { outline: 2px solid var(--accent); outline-offset: 4px }`. The UA ring is used today because the global rule at `app.css:75` covers buttons only.
4. **`--text-faint` fails AA for text.** It is used for the `.kind` "ALBUM" label (`AlbumView.svelte:99`), the disc headers (`TrackList.svelte:59`), track numbers (`TrackList.svelte:81`, 3.22:1 on row hover) and `.sep`. Fix: `--text-faint: #868aa0`, which gives 4.67:1 even on `--bg-hover` (see the table).
5. **The seek bar fails non-text contrast and is hard to hit.** In `TransportBar.svelte:196-216` the unplayed track is `--bg-active` on `--bg-raised` (1.27:1, needs 3:1). The hit area is 4 px tall. The thumb shows only on `.seek:hover`, so a keyboard user sees no handle. `step="100"` makes each arrow key seek 0.1 s and send a `playerSeek`, and screen readers read the value as raw milliseconds. Fix:
   - Set `height: 20px; background: transparent` and paint the bar in `::-webkit-slider-runnable-track` with height 4 px. The unplayed colour becomes `rgba(255,255,255,.34)` (3.1:1).
   - Add `.seek:hover ::-webkit-slider-thumb, input:focus-visible::-webkit-slider-thumb { opacity: 1 }`.
   - Add an `onkeydown` handler that maps the arrow keys to ±5 s and PageUp/PageDown to ±30 s, then calls `preventDefault`.
   - Add `aria-valuetext="{duration(position)} of {duration(total)}"`.

## P2: bugs, polish gaps and per-frame cost

6. **The tonearm leaves a rAF subscriber running after it unmounts.** The 300 ms `setTimeout` in `Tonearm.svelte:162-165` is never cleared. If you drag the arm and then leave Now Playing within 300 ms while music plays, `wake()` starts a new `onFrame(frame)` after `onMount`'s cleanup has already run. The `idle` check at line 97 never becomes true while playing, so `render()` writes to detached nodes every frame until the next pause. Fix: keep the timer id, set a `destroyed` flag in the onMount cleanup, `clearTimeout` there, and have `wake()` return early when `destroyed` is set.
7. **Escape never cancels a mouse drag.** `onPointerDown` calls `e.preventDefault()` (line 133), which suppresses the mousedown that would focus the `<g>`. The Escape handler at line 175 therefore never fires unless the arm was already focused by keyboard. Fix: `(e.currentTarget as SVGGElement).focus({ preventScroll: true })` in `onPointerDown`. Also add `onlostpointercapture={() => endDrag(false)}`, because alt-tab or a window blur mid-drag leaves `dragging` stuck at true.
8. **Repeated arrow keys on the arm do not add up.** `seekBy` (line 170) reads `playbackMs()`, which has not yet picked up the previous seek, so three quick presses of → move 5 s, not 15 s. The transport range has the same problem once fix 5 lands. Fix: keep `keyTarget = { ms, until: now + 600 }`, base the next step on `keyTarget.ms` while it has not expired, and reuse `pendingSeek` so the arm holds still.
9. **`contain: paint` clips the record's drop shadow.** In `VinylDisc.svelte:76`, paint containment clips to the border-radius box, so the `box-shadow` on `.surface` (lines 84-86) never renders and the record looks pasted on. Fix: `contain: layout style` (or move the shadow to `.record` in TurntableStage).
10. **The indeterminate scan bar disappears under reduced motion.** `app.css:38-39` forces `iteration-count:1; duration:1ms`. The `slide` animation (`SettingsView.svelte:272-281`) jumps to `translateX(330%)`, outside the clipped track, so the bar looks empty while scanning. Fix: inside `@media (prefers-reduced-motion: reduce)` set `.fill.indeterminate { animation: none; width: 100% !important; opacity: .45 }`.
11. **Opening and closing the queue jumps the layout.** `.queue-slot` (`App.svelte:106-110`) is a flex item. When it opens, `main` narrows at once (the album grid reflows its columns and Now Playing re-centres) while the panel is still sliding. When it closes, `main` widens 280 ms late. Option A (recommended): make the slot `position:absolute; right:0; top:0; bottom:0` over `.body`, so `main` never reflows. Option B: keep the docked layout and move the slide to the inner `<aside>`, accepting one reflow. This is a product call: it decides whether the queue overlays content or pushes it aside.
12. **Every visible album card has a `view-transition-name`** (`AlbumCard.svelte:28`). Going from Albums to Settings captures 30-60 snapshot groups. They fade on their own default animation instead of sliding with `content`, and capture costs a frame at 4K. Fix: set the name only on the clicked card in `open()` (`node.style.viewTransitionName = "album-art"`), and use `"album-art"` in AlbumView. On `back()`, give the name to the matching card before the transition starts.
13. **Focus is lost after navigation.** Clicking a card or "Back" destroys the focused node, so focus drops to `<body>` and screen-reader users lose their place. Fix: in `app.go/back`, after `await tick()`, focus the new view's `h1` (`tabindex="-1"`, focus ring suppressed). Add `aria-current="page"` to `.nav.active` (`App.svelte:69`) and `aria-expanded={app.queueOpen}` to the queue toggle (`TransportBar.svelte:90`). Escape on the queue panel should close it and return focus to the toggle.
14. **Toasts.** The dismiss target is 14×14 px (`App.svelte:119`), below the 24 px minimum (WCAG 2.5.8). Toasts dismiss on a timer with no pause (`app.svelte.ts:121`, WCAG 2.2.1). Errors are only announced politely. Fix: `.toast button { width: 28px; height: 28px; display:grid; place-items:center; border-radius:50% }` plus a hover background. Pause the timer on `pointerenter`/`focusin` and resume on leave. Render errors in a separate `role="alert"` region.
15. **Small transport controls.** Prev, Next and Queue (`.ctl`, `TransportBar.svelte:151`) hit at the 20 px icon size, and the queue close button at 18 px. Fix: `.ctl { width: 36px; height: 36px; border-radius: 50% }` with a `--bg-hover` hover background and `:active { transform: scale(.92) }` on the spring easing. This gives the tactile pill feel of Mochy.
16. **The blurred backdrop costs work every frame while music plays.** `filter: blur(60px)` on a full-page image (`CoverBackdrop.svelte:39`) sits under the spinning label. Each frame the spinner's damage rect, inflated by about 3× the blur radius, forces the GPU to blur that region again. WebKitGTK often does this in software. Fix: drop the CSS filter. Request a 24-32 px thumbnail (`artUrl(art, 32)`) and let `image-rendering: auto` upscaling do the blur, or blur once into a canvas and use `drawImage`. Then `saturate` alone is cheap.
17. **The arm shadow is a blurred, full-stage layer that moves every frame.** `Tonearm.svelte:328-330` blurs a stage-sized layer whose transform changes every frame while playing (lines 60-65), so the compositor runs the blur on each frame. Fix: put an SVG `<filter><feGaussianBlur stdDeviation="6"/></filter>` inside the shadow `<svg>`; it is rasterised once and the layer transform stays composited. In `render()`, skip the write when |Δangle| < 0.004° and lift is unchanged. The arm creeps about 0.1°/s, so most frames write nothing.
18. **The seek fill repaints every frame.** `TransportBar.svelte:23-33` sets `--pct` per frame, which restyles and repaints the input at 144-240 Hz for about 2 px/s of movement. The effect also re-subscribes on every ~10 Hz `snap`. Fix: draw the fill as a child `<div>` with `transform: scaleX(f)` (composited), or skip writes whose rounded pixel width has not changed. Read `snap.state` through `untrack` except for the state itself.
19. **The record-change choreography** (`TurntableStage.svelte:20-26`, `transitions.ts:11-24`). The record starts sliding in the same frame the arm starts lifting, so the stylus is dragged across the record. With in=cubicOut and out=cubicIn, both records sit about 14% off centre at t=0.5, fully opaque, and collide. Retune with the values below: the arm lifts first, then out, then in.
20. **Dim text over a bright cover.** With a white or yellow cover, `--text-dim` in `.sub` and `.path` (`NowPlayingView.svelte:131,146`) drops to 3.6-4.0:1 against the backdrop. Fix: on the `.meta` column, set `color: var(--text)` for `.path`, or add `text-shadow: 0 1px 12px rgba(14,16,24,.9)`, or raise the bottom stop of the `.shade` gradient to `.9`.
21. **The focused tonearm slider may chatter (verify with NVDA).** `aria-valuenow` and `aria-valuetext` (lines 281-282) change every second during playback. NVDA announces value changes on a focused slider. Fix: while focused and not user-initiated, update the value text only on seek, or every 15 s.

## P3: polish

- `mix-blend-mode: overlay` on the ring text (`VinylDisc.svelte:177`) blends against a transparent backdrop inside the spinner's isolated group. It has no visible effect but costs an extra render surface, so remove it.
- `--ease-spring` uses `linear()`, which older WebKitGTK builds (before 2.42) reject. The whole `transition` then becomes invalid and hovers snap. Default to `cubic-bezier(.34,1.4,.64,1)` and override inside `@supports (transition-timing-function: linear(0, 1))`.
- `animate:flip={{ duration: 200 }}` (`App.svelte:117`) ignores reduced motion. Use `duration: reducedMotion() ? 0 : 200`.
- Card hover animates `box-shadow` (`AlbumCard.svelte:51-54`), which repaints. Move the shadow to a `::after` and animate its `opacity`.
- `.btn` has no press state. Add `transition: transform var(--dur-med) var(--ease-spring)` and `.btn:active { transform: scale(.96) }`.
- Both sliders are named "Seek". Rename the tonearm to "Seek (tonearm)".
- Preload the next queue item's 1000 px art, so the incoming label does not flash `#27303b` mid-slide.
- Add an opt-in polite live region "Now playing: {title} by {artist}" for user-initiated track changes.
- Track change and queue rows: scroll the current queue item into view when the panel opens.
- Icon-only buttons rely on `title` for their name. Add `aria-label` as well; VoiceOver on WebKit is inconsistent with `title`.

## Contrast (WCAG 2.1, computed)

| Foreground | Background | Ratio | AA normal / large |
|---|---|---|---|
| --text #e9eaf0 | --bg / --bg-raised / --bg-active | 15.82 / 14.69 / 11.61 | pass |
| --text-dim #9a9db0 | --bg / raised / hover / active | 7.07 / 6.57 / 5.94 / 5.19 | pass |
| --text-dim | accent-dim over --bg (playing row) | 5.28 | pass |
| --text-faint #6b6f85 | --bg / raised / hover | 3.83 / 3.56 / 3.22 | **fail** / pass (≥3) |
| --text-faint | accent-dim over --bg | 2.86 | **fail** |
| proposed #868aa0 | bg / raised / hover | 5.57 / 5.17 / 4.67 | pass |
| --accent #e8b04a | --bg / raised / active | 9.71 / 9.02 / 7.13 | pass |
| --accent | accent-dim over bg / raised | 7.24 / 6.62 | pass |
| #1a1306 | --accent (primary btn) | 9.42 | pass |
| --bg | --text (main play btn) | 15.82 | pass |
| --error / --warn / --ok | bg / bg / raised | 5.46 / 11.97 / 9.28 | pass |
| --ok / --warn on own 14% badge | — | 7.00 / 9.02 | pass |
| --text-dim on backdrop, white cover | worst case | 3.59 | **fail** |
| Non-text: seek track --bg-active | --bg-raised | 1.27 | **fail** (needs 3:1) |
| Non-text: proposed rgba(255,255,255,.34) | --bg-raised | 3.1 | pass |
| Focus ring --accent | --bg / --bg-active | 9.71 / 7.13 | pass |
| Ring text (decorative, aria-hidden) | #08080a | 3.00 | exempt |

## Motion parameters to retune

| Parameter | Now | Proposed | Why |
|---|---|---|---|
| Tonearm `angle` spring | k 40, ζ 0.9 | k 70, ζ 0.82 | Swings settle in about 0.55 s with a hint of overshoot, which feels tactile. |
| Tonearm `lift` spring | k 180, ζ 1 | k 260, ζ 0.9 | The lift reads before the swing starts. |
| Track change: arm lead | 0 ms | out slide `delay: 120` | The stylus clears the record first. |
| `slideRecord` out | 520 ms cubicIn, rotate 8° | 380 ms cubicIn, rotate 5° | Leaves before the new record arrives, so the records do not collide. |
| `slideRecord` in | 520 ms cubicOut | delay 220, 560 ms, spring easing (`backOut` with s=0.9, about 3% overshoot) | The record lands like a platter. |
| `SETTLE_MS` | 680 | 860 (220 + 560 + 80) | The arm drops once the record has landed. |
| `SPIN_UP` / `SPIN_DOWN` | 64 / 16 | 64 / 12 | A slightly longer coast (about 1.9 s) reads more like a real platter. |
| `.ctl.main:active` | scale .92 | scale .94, then spring back over 280 ms | Less jarring at 38 px. |
| `.btn`, `.ctl` press | none | scale .96 / .92 on `--ease-spring` | Matches the pill tactility of the reference. |
| CoverBackdrop crossfade | 900 ms | 650 ms `--ease-standard` | Shortens the time two filtered layers animate together. |
| Toast lifetime | 5 s / 9 s, no pause | 6 s / 10 s, paused on hover and focus | WCAG 2.2.1. |
| Queue panel | 280 ms with layout jump | 300 ms `--ease-out`, overlay (fix 11) | The layout stays put. |
| `vt-in` | 280 ms, no delay | 280 ms, `delay: 40ms` | Old and new content overlap less. |

## Unresolved questions

1. Should the queue overlay the content or keep pushing it aside (fix 11, Option A vs B)? This is a product decision.
2. Fixes 21 and 16 should be checked with NVDA and the WebKitGTK perf tools before they are prioritised.
3. Should Sonder get global keyboard shortcuts for next, previous and seek (for example Ctrl+→ / Ctrl+←)? Today only Space and Ctrl+F exist.
