# UI review 2: Now Playing and the turntable

Scope: `NowPlayingView`, `TurntableStage`, `VinylDisc`, `Tonearm`, `CoverBackdrop`, `FormatBadges`, `TransportBar`. Reference: Mochy screenshots `phoneScreenshots/4.png` (player), `8.png` (player with sheet), `docs/assets/img/app-player.webp`. Visual language only; no Mochy code was opened.

## Diagnosis

The turntable is well engineered: only the label turns, the arm is a pre-drawn layer rotated about its pivot, and reduced motion is honoured throughout. The problem is the screen around it. Now Playing is a hero visual with a small text column and no controls of its own. Play, skip and time all live in the 84 px global bar, where the play button is 38 px and the seek bar is 4 px (`TransportBar.svelte:163-169`, `:196-205`). Mochy's player is the opposite. The art dominates, the title is bold, the progress bar is thick, and the controls are big white pills you cannot miss (4.png, 1290-1740 px). Sonder also has three sizing problems. The cover-as-label is only about 211 px wide at 1280×820. The layout overflows at the 900×600 minimum. And the record's printed bands do not match where the arm actually travels.

## Ranked findings

### P1-1 The layout clips at the 900×600 minimum window
At a viewport of 1000 px or less, the layout collapses to one column (`NowPlayingView.svelte:151-167`) and the stage becomes `min(56vh, 80vw)`, which is 336 px (`:160`). The content area is about 690×516 (window minus the 210 px sidebar and the 84 px bar). The column adds up to 336 + 20 + 112 (`.meta-inner` min-height, `:112`) + badges and path (about 60) + 48 of padding, roughly 576 px. Because `.page` has `overflow: hidden` (`:73`), the badges and path line get cut off, which breaks the "badges always visible" constraint. A second issue is that `vh` and `vw` measure the window, not the pane, so the 210 px sidebar is never accounted for.
**Proposal:** set `container-type: size` on `.page` and size everything from `cqw`/`cqh`. Keep two columns down to a pane width of about 640 px, because a landscape pane wants the record on the left and the text on the right. Set `.visual { width: min(88cqh, calc(100cqw - 380px - 120px)); }`. Drop the `@media (max-width:1000px)` block and add `@container (max-width: 640px)` only as a fallback.

### P1-2 Now Playing has no hero controls and no visible time (compare 4.png)
The only seek control on this screen is the tonearm. It sweeps just 20° (START_ANGLE ≈ 98.9°, END_ANGLE ≈ 119.0°, from `turntable.ts:44-45`), and the time only appears while you drag (`Tonearm.svelte:297-303`). Elapsed and total time are never shown at rest.
**Proposal:** add a `NowPlayingControls` block to the meta column. On the `now` route, slide the global `TransportBar` down and hide it (transform plus opacity, 280 ms; fade only under reduced motion), so the controls are not duplicated and the stage gains 84 px of height. Specs:
- **Seek bar:** 8 px track with `border-radius: 4px`. The track is `rgba(255,255,255,.18)` and the fill is `#f4f4f6`. On hover or focus the track grows to 10 px and a 16 px thumb appears. Elapsed and total sit under the ends at 13 px with `tabular-nums`. Reuse the `--pct` per-frame paint from `TransportBar.svelte:22-33`. Keep the tonearm as a second, playful way to seek; both read `playbackMs()`.
- **Transport pills:** a flex row with a 14 px gap. Prev and next are 76×56. Play is 116×64. All use `border-radius: 999px`, `background:#f4f4f6`, `color:#0e1018`, with 26 px icons (30 px for play). Hover is `scale(1.04)`, active is `scale(.94)` with `--ease-spring`, and focus shows a 3 px ring in `--accent` at a 3 px offset.
- **Outlined icon row:** 44×44 buttons with `border:1.5px solid rgba(255,255,255,.28)`, `border-radius:14px` and a 10 px gap. Use only features that already exist: Queue (moved from the bar, `TransportBar.svelte:90`), Go to album, a volume / bit-perfect toggle (`playerSetVolume`, `bindings.ts:41`), and a cover style toggle (vinyl or square).

### P1-3 The cover is too small to carry the screen
At 1280×820 the stage is `min(68vh, 50vw)` = 558 px (`NowPlayingView.svelte:86`). The record is 86 % of that, 480 px (`TurntableStage.svelte:58`), and the label is 44 % of the record (`turntable.ts:12`), about 211 px. That label is the only colour on screen. Mochy's art takes about 80 % of the width.
**Proposal:** do not enlarge the label. At 44 % it is already bigger than a real label (about 33 %), and changing it moves END_FRACTION and the tested geometry. Instead, give the stage more room. Hiding the global bar (P1-2) and switching to container sizing (P1-1) brings the stage to about 610 px at 1280×820, with a 525 px record and a 231 px label. Also see P3-1, where the album sleeve becomes a second, large art surface.

### P2-1 The record's bands contradict the arm
The fake track gaps sit at 61.4 / 71.4 / 81.4 / 89.6 % of the radius (`VinylDisc.svelte:98-116`). The stylus starts at 93 % (`START_FRACTION`), outside the outermost "lead-in" band, and then crosses three gaps that mean nothing, because one record is one track.
**Proposal:** make the surface match the arm's travel and remove the three inner gaps. Use these rings:
- **Lead-in:** a glossy smooth band from 92 to 95 %, then a rim from 96.6 to 100 %.
- **Programme area:** grooves only, from 51 to 92 %.
- **Run-out:** a single bright spiral line at 51 % (END_FRACTION).
- **Dead wax:** glossier from 44 to 51 %, using `#121216` with no grooves.

### P2-2 The groove and sheen are flat
The groove texture is one uniform `repeating-radial-gradient` at 1 px / 2.6 px, alpha .028 (`:117-121`). On real vinyl the grooves are only visible where the light catches them.
**Proposal:** split this into a static `.grooves` layer. Use two frequencies: `rgba(255,255,255,.035) 0 .6px, transparent .6px 1.8px` plus a coarser `rgba(0,0,0,.25) 0 1px, transparent 1px 4.2px`. Then apply `mask-image: conic-gradient(from -70deg, #000 0 8deg, rgba(0,0,0,.35) 30deg, rgba(0,0,0,.15) 90deg, #000 180deg 188deg, rgba(0,0,0,.35) 210deg, rgba(0,0,0,.15) 270deg, #000 360deg)` so the grooves sparkle inside the light lobes. In the existing conic sheen, add a narrow core inside each lobe (`.22` alpha across ±3°) and a faint warm and cool fringe at the lobe edges (`rgba(255,210,170,.04)` and `rgba(170,200,255,.04)`). All of this paints once, so there is no per-frame cost.

### P2-3 The ring text uses `mix-blend-mode` on the rotating layer
`.ring text { mix-blend-mode: overlay }` (`VinylDisc.svelte:177`) sits inside `.spinner`, which turns every frame. The blend forces a re-composite against the record every frame, which works against the "only the label rotates" budget.
**Proposal:** remove the blend. Use `fill: rgba(255,255,255,.2)`, set the font to 15 px with `font-weight: 600` and `letter-spacing: .28em`, and keep the radius at 246 so the text reads as stamped into the dead wax.

### P2-4 The label and spindle look like a pasted circle
**Proposal:**
- **Paper edge:** in `.label-light`, add `inset 0 0 0 3px rgba(0,0,0,.28)` and `inset 0 0 0 4px rgba(255,255,255,.06)`.
- **Spindle:** make it chrome with `radial-gradient(circle at 36% 30%, #f4f5f7 0 10%, #a7a9ae 32%, #44464b 62%, #151517 100%)`, inset 46.2 %, plus a static contact shadow of `0 2px 3px rgba(0,0,0,.55)` and a 1 px dark collar.
- **Paused state:** keep the current desaturation (`:153`) and also dim `.label-light` by .06 so the label looks unlit.

### P2-5 The record floats with no platter
The record sits directly on the blurred backdrop. **Proposal:** add a static `.platter` to `TurntableStage`, positioned behind `.record-slot` with an inset of -1.8 %. Paint it with brushed metal: `repeating-conic-gradient(rgba(255,255,255,.025) 0 .4deg, transparent .4deg 1.1deg)` over `#2a2c31`. Give it a 1 px light top edge and a ring of 60 strobe dots on its rim, drawn once as SVG with `r=2.2` and `fill: rgba(255,255,255,.18)`. It clears the plinth, because the record edge at y=140 reaches only x≈685 against the plinth's left edge at x=836.

### P2-6 The tonearm drawing reads as rectangles
- **Proportions:** the tube is 14 units (about 8 px) (`Tonearm.svelte:223`), which is spindly next to a 525 px record. The counterweight is a 52×48 box (`:220`). The plinth is a 128×170 pill (`:252-253`).
- **Tube:** 18 units, with a 2-unit specular line `rgba(255,255,255,.55)` at y−4 and a darker 1-unit underside.
- **Headshell:** replace the rotated rect (`:228`) with a tapered path, 34 units wide at the tube and narrowing to 26. Add a matte-black cartridge body (40×24, `#101012`) with a 3-unit `--accent` stripe, plus a stylus cantilever dot at the tip. The finger lift (`:229-235`) becomes a curved path with round caps.
- **Counterweight:** a cylinder seen from above, 60×52 with `fill:url(#alu)`, three knurl lines at x−96/−88/−80 (`stroke: rgba(0,0,0,.35)`), and a dark decoupling ring of 6 units at the tube end.
- **Base:** replace the pill with a round bearing base (`circle r=62`, plinth gradient, 1.5 px rim light). Add a gimbal ring (`circle r=34`, stroke `#5a5c62`) around the existing cap. Add a static anti-skate knob (`circle r=15` at PIVOT + (−52, 86)) and a lift lever (60×10 at PIVOT + (30, 96)). These are decorative, `aria-hidden`.
- **Arm rest:** draw it as a post with a U cradle (path) instead of the plain rect at `:254`.
- **Constraint:** keep every shape inside `armShapes` so the shadow layer stays one copy. No change to `turntable.ts`.

### P2-7 Text hierarchy is weak against the visual
Title is 30 px (`NowPlayingView.svelte:121`); artist and album share one 16 px dim line (`:129-135`).
**Proposal:**
- **Title:** `font-size: clamp(26px, 2.2cqw + 12px, 40px)`, `font-weight: 750`, `letter-spacing: -0.02em`.
- **Artist and album:** artist on its own line at 18 px, `rgba(255,255,255,.82)`. Album below it as a 14 px link.
- **Header:** add Mochy's small centred header at the top of the pane: "Now Playing" at 13 px/600 over the album name at 13 px dim.
- **Badges:** move them directly under the artist; they stay visible and need no restyle.
- **Device path:** demote it to an 11.5 px faint line under the icon row.

### P3-1 Optional: the sleeve as a second art surface
Place the square cover as a sleeve behind and to the left of the record, at 70 % of the stage and offset (−18 %, 6 %), with a `0 30px 60px` shadow. On a track change, the record slides out of the sleeve. This shows the art large, as Mochy does, without touching the label geometry. It only fits at a pane width of 1100 px or more, so hide it below that.

### P3-2 Backdrop tint
In `CoverBackdrop.svelte:46-48`, Mochy's wash is cover colour at the top that deepens to near-black behind the controls (4.png). **Proposal:** `.shade` becomes `linear-gradient(to bottom, rgba(14,16,24,.12), rgba(14,16,24,.55) 55%, rgba(14,16,24,.92))`. Raise the image opacity to .7. This keeps white pills on dark at well over 4.5:1.

## Proposed layout: 1280×820 (pane 1070×820, global bar hidden)

```
+------------------------------------------------------------------------+
|                        Now Playing · Album name                        | 13px header
|                                                                        |
|  [platter+record 525px]      (arm) |   Title Title Title    (o) (o)    | 40px/750, round 44px: album, queue
|   label 231px                 \    |   Artist  (18px)                  |
|                                \   |   Album (14px link)               |
|                                 o  |   [FLAC 24/96] -> [24/96] [Bit-p] |
|          stage ~610px              |   ================o------------   | 8px seek bar
|                                    |   1:02                     4:31   |
|                                    |   ( |< )  (    >    )  ( >| )     | pills 76/116/76 x 56/64
|                                    |   [Q] [Album] [Vol/BP] [Style]    | 44px outlined
|                                    |   device path (faint)             |
+------------------------------------------------------------------------+
grid: minmax(0,610px) 48px minmax(340px,400px); align-items:center; padding 24px 48px 32px
```

## Proposed layout: 900×600 (pane 690×600)

```
+--------------------------------------------------+
|              Now Playing · Album name            |
| [record ~330px]  arm |  Title (26px, 2 lines)    |
|  label 145px         |  Artist                   |
|                      |  [badges wrap 2 lines]    |
|                      |  ============o-------     |
|                      |  0:42             3:10    |
|                      |  (|<) (  >  ) (>|) 64/96/64 x 52 |
|                      |  [Q][Alb][Vol][Sty] 40px  |
+--------------------------------------------------+
grid: minmax(0,1fr) 28px 300px; record = min(88cqh, 100cqw-300-28-56)
```

Below a pane width of 640 px (not reachable at the 900 px minimum unless the sidebar grows), stack the record above a compact block. The minimum touch target there is 44 px.

**Do full transport controls and a big seek bar belong here?** Yes. A dedicated player screen without play, skip and visible time forces the eye down to a 38 px button in a separate bar. Mochy's thick bar and pills are the clearest cue that this is the place you control music from. The tonearm stays as the delightful, imprecise seek; the bar is the precise one. To avoid duplicated controls, hide the global bar on this route.

## Motion and accessibility notes
- **Hero buttons:** use the existing `--ease-spring`. Under reduced motion, the global rule already shortens transitions.
- **Bar slide-out:** must use the fade fallback from `transitions.ts`.
- **Seek bar:** exposes `role=slider` natively through `<input type=range>`. The tonearm slider (`Tonearm.svelte:276`) and the bar must announce the same `aria-valuetext`.
- **Contrast:** white pills (#f4f4f6) with #0e1018 icons give about 17:1. Outlined buttons at `.28` borders pass 3:1 for non-text against the darkened bottom of the backdrop.

## Unresolved questions
1. Is hiding the global transport bar on the `now` route acceptable, or must it stay for muscle memory? If it stays, the hero controls should replace only its centre column on this route.
2. Should the sleeve (P3-1) ship, or is the record-only look the intended identity?
3. Should the outlined row include the cover style toggle, or should that stay in Settings only?
