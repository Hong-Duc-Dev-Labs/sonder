# UI review 4: app shell and controls

Scope: sidebar, transport bar, queue panel, toasts, Settings, buttons, inputs and sliders. This is a review only, and no source was changed. The Mochy references were viewed as screenshots only.

## Diagnosis

The shell works, but it looks like a 2018 admin layout. It has three full-height slabs (sidebar, content, queue) separated by 1px `--border` lines, plus a docked footer strip. Mochy reads as premium for four reasons. Every chrome element floats as a rounded tonal surface instead of being cut out by hairlines. Controls are pills and circles. Headers are very bold (around 800 weight, 36 to 44 px). Settings are grouped into stacked tonal cards with round icon chips (`app-behaviour.webp`, `app-settings.webp`, `app-audiolab.webp`). Sonder already has the right ingredients: an amber accent, a spring easing token, `.btn` pills, the MiniDisc, and a strong audiophile badge story. The work is mostly restyling surfaces and fixing a few interaction bugs, not adding features.

## Findings

### P1

1. **The docked footer strip is the main reason the app looks flat.** Evidence: `TransportBar.svelte:97-107` (full width, `border-top`, `--bg-raised`), `App.svelte:113`. Reference: bottom of `2.png`, where a floating pill holds a round disc with a progress ring. Also see `app-home.webp`. Proposal: a floating pill (design below).
2. **The transport's left column overflows at narrow widths.** Evidence: `TransportBar.svelte:51-55` stacks title, artist and `FormatBadges compact` (three rows) inside an 84 px bar. `FormatBadges.svelte:33` uses `flex-wrap: wrap`, so at 900 px (left column about 245 px) the source → device → Bit-perfect badges wrap to a fourth row and get clipped. Proposal: badges become one line, `flex-wrap: nowrap`, and below 1100 px they collapse into a single status chip (see the pill design).
3. **The queue is not keyboard-operable, and its rows are keyed by index.** Evidence: `QueuePanel.svelte:21-25` has a `<button>` with only `ondblclick`, so Enter and Space do nothing. The `title="Double-click to play"` hint is the only affordance. `{#each app.queue as item, i (i)}` keys by position, so inserts and removals re-use DOM nodes and the current-row highlight jumps. Proposal: `onclick` selects and `ondblclick` or Enter plays (`onkeydown` Enter → `playerPlayIndex`). Key rows by a stable id: use `item.trackId` if it is unique per queue entry, otherwise add `entryId` on the Rust side. Add `aria-current="true"` on the playing row.
4. **The seek slider is invisible to keyboard and screen-reader users.** Evidence: `TransportBar.svelte:206-216`. The thumb has `opacity: 0` unless `.seek:hover`, so a keyboard-focused slider shows no thumb. The track is a 4 px hit target. The range has no `aria-valuetext`, so NVDA reads raw milliseconds ("123400"). Proposal:
   ```css
   .seek:is(:hover,:focus-within) input::-webkit-slider-thumb{opacity:1}
   input[type=range]{height:16px;background:transparent}            /* hit area */
   input[type=range]::-webkit-slider-runnable-track{height:4px;border-radius:999px;
     background:linear-gradient(to right,var(--accent) var(--pct),var(--surface-4) var(--pct))}
   ```
   Also add `aria-valuetext={`${duration(position)} of ${duration(total)}`}`.
5. **The global Space handler blocks button activation.** Evidence: `App.svelte:44-47` only excludes `input, select, textarea`. With focus on any `<button>` (nav, queue, Clear), Space toggles playback and `preventDefault` cancels the button's own activation. This breaks the WCAG 2.1.1 keyboard pattern. Proposal: also skip `button, [role=button], a, [role=radio], [role=switch]`, and only fire on `!e.repeat`.

### P2

6. **Sidebar information architecture mixes three kinds of entry.** Evidence: `App.svelte:29-35` lists library pages (Albums, Artists), a tool (Search), a mode (Now Playing) and app config (Settings) in one flat list. The active state is a low-contrast `--bg-active` box with a 6 px radius (`App.svelte:160-180`). There is no `aria-current="page"`, and Ctrl+F is not shown anywhere. Proposal: see the sidebar design.
7. **The back button is an absolute overlay plus a padding hack.** Evidence: `App.svelte:82-85,204-218` (`.with-back { padding-top: 28px }`, button at `top:14px; left:26px`), which overlaps view headers. Reference: `app-behaviour.webp`, where a round tonal back chip sits in the page header flow. Proposal: render a 40 px circular chip inside a shared `.page-head` row that each detail view owns. Add `aria-label="Back (Alt+Left)"`.
8. **Transport hit targets are small and the CSS has duplicate rules.** Evidence: `.ctl` (`TransportBar.svelte:151-155`) has no size, so prev, next and queue are about 24 px. The play button is 38 px. `.ctl.on` and `.ctl.main` are each declared twice (`156-162`, `163-179`). The queue toggle has no `aria-pressed`, and the icon-only buttons rely on `title` with no `aria-label`. Proposal: `.ctl{width:40px;height:40px;border-radius:999px}`, `.ctl:hover{background:var(--surface-3)}`, play button 48 px, and merge the duplicate rules.
9. **Toasts are visually off-system and not assertive for errors.** Evidence: `App.svelte:115-122,222-256` (8 px radius, `border-left` stripe, `aria-live="polite"` for errors). `app.svelte.ts:121` dismisses on a timer that keeps running while the user is reading or hovering. Proposal: pill toasts (`border-radius:999px` when one line, 18 px when multi-line, `--surface-3`, a leading 8 px status dot instead of the stripe). Errors get `role="alert"`, the timer pauses on hover and focus, and the stack is anchored above the pill, centred in `main`.
10. **Settings is a stack of hairline sections with native form controls.** Evidence: `SettingsView.svelte:203-208` (`border-top` sections), `:157-160` (Bit-perfect is a checkbox with a 12-word label), `:144-150,181-192` (native radios). The `h1` uses the browser default size. Reference: `app-settings.webp`, `app-behaviour.webp`, `app-audiolab.webp`. Proposal: tonal cards (design below).

### P3

11. **Tokens lack a tonal ramp and radius scale.** Evidence: `app.css:2-19` has one `--radius: 8px` and no pill or large radius. Proposal: add `--surface-1:#131624; --surface-2:#191c2c; --surface-3:#21253a; --surface-4:#2b3049; --on-accent:#1a1306; --accent-container:rgba(232,176,74,.18); --r-sm:10px; --r-md:16px; --r-lg:24px; --r-pill:999px; --dock-h:72px; --dock-reserve:104px`. Keep the old names as aliases so the other views do not break.
12. **Inputs and `.badge` use a 4 to 6 px radius** (`app.css:82-90,134-146`). Text inputs and the search field become `--r-pill`, 40 px high, `--surface-2`, with no border (focus ring only). Badges keep the mono face, which works well for format strings, but should move to a 999 px radius.
13. **The brand mark is a CSS ring** (`App.svelte:154-159`). Replace it with a 20 px inline SVG record (ring plus spindle) in the accent colour so it matches MiniDisc.
14. **The scan indicator is plain text** (`App.svelte:74-78`). Make it a pill chip with a 2 px circular progress, `role="status"`.

## Proposed design

### Transport: floating pill (`TransportBar.svelte`)
The pill is placed absolutely inside `main` (not over the sidebar or queue): `bottom:16px`, centred, `width:min(100% - 32px, 1040px)`, `height:72px`, `border-radius:36px`, `background:color-mix(in srgb,var(--surface-3) 88%,transparent)`, `backdrop-filter:blur(20px) saturate(1.2)`, `box-shadow:0 12px 32px rgb(0 0 0/.45), inset 0 1px 0 rgb(255 255 255/.04)`. View scroll containers get `padding-bottom:var(--dock-reserve)` so content scrolls under the pill. This replaces `--transport` in the `.shell` flow.

At 1280 px (main is about 1000 px wide, pill about 968 px):
```
 .------------------------------------------------------------------------------------.
(  (@)  Title of the track            |<<  ( > )  >>|   1:23 ====o------------ 4:56   ≡  ⤢ )
 '-ring Artist · FLAC 24/96 → 24/96 [Bit-perfect] -------------------------------------'
  56px  min 200 / flex 1               3 ctl 40/48/40      flex 1.2 (min 220)        40 40
```
At 900 px (sidebar collapses to a 72 px rail, main is about 828 px, pill about 796 px):
```
 .--------------------------------------------------------------------------.
(  (@)  Title of the track…   [● Bit-perfect ▾]   |<<  ( > )  >>|  -3:33   ≡  )
 '------------------------------------------------------------------------'  <- 3px seek line on inner bottom edge
```
- **Disc:** MiniDisc at 52 px inside an SVG progress ring (r=27, `stroke-width:2.5`, accent on `--surface-4`, `stroke-dashoffset` driven by the same `onFrame` clock as `--pct`). Clicking it opens Now Playing. The ring is decorative (`aria-hidden`), and the range input remains the accessible seek control.
- **Badges:** at 1280 px they stay inline after the artist. Below 1100 px they collapse into one chip coloured by state (ok, warn, accent). It opens a popover showing source → device and the volume state, so the audiophile information is never dropped.
- **Seek:** inline at 1100 px and wider. Below that, a 3 px line inset 28 px along the pill's bottom edge, with a 14 px invisible hit area, and the time shown as remaining.
- **Play:** a 48 px accent circle (`--accent`, `--on-accent`) using the spring scale that already exists. Prev and next are 40 px ghost circles.
- **Right side:** queue toggle (`aria-pressed`, accent-container fill when on), and a software-volume chip only when `volumeDb != null`.

### Sidebar (`App.svelte`)
`--sidebar:232px` at 1100 px and wider. Below that it becomes a 72 px icon rail with tooltips (`@media (max-width:1099px)`). The surface is `--surface-1`, floating with `margin:12px 0 12px 12px` and `border-radius:var(--r-lg)`, and the right border is removed. The order is: brand, then a "Library" overline (Albums, Artists), then Search with a `Ctrl F` kbd hint on the right, then Now Playing showing a 20 px live MiniDisc when something plays. A spacer follows, then the scan chip, then Settings pinned to the bottom. Nav items are 40 px tall with `--r-pill` radius. Hover uses `--surface-3`. Active uses `--accent-container` with `--accent` icon and text at weight 650, plus `aria-current="page"`.

### Queue panel (`QueuePanel.svelte`)
The panel floats like the sidebar: `margin:12px 12px 12px 0`, `width:340px`, `--surface-1`, `--r-lg`. The header reads "Queue" at 22/800, then a `n tracks · 1h 12m` subline, then the Clear pill. Sections: "Now playing" (one large row with a 48 px cover and an animated EQ glyph), then "Up next", then a dimmed "Played" group. Rows are 56 px with `--r-md` radius, and hover shows a trailing remove button (needs a `queueRemove` command, which may not exist yet). The empty state is a centred disc icon, one sentence and a "Browse albums" pill button.

### Settings (`SettingsView.svelte`)
Header `h1` is 40px/800, `letter-spacing:-.03em`. Below it, a column (`max-width:760px`, `gap:16px`) of cards using `.card{background:var(--surface-2);border-radius:var(--r-lg);padding:8px}`. Rows use `.row{display:grid;grid-template-columns:40px 1fr auto;gap:14px;align-items:center;min-height:60px;padding:10px 12px;border-radius:var(--r-md)}`, with an icon chip `{width:40px;height:40px;border-radius:50%;background:var(--surface-4)}`. A group title (15/700) sits above each card, as in `app-behaviour.webp`.
1. **Output hero card** (Audio Lab style, `app-audiolab.webp`). A status line (`● BIT-PERFECT` in mono, or `● SOFTWARE VOLUME −6.0 dB`), then the device name at 28/800 and a faint large disc glyph on the right. Below it, a "Bit-perfect output" row with a **switch** (`input[type=checkbox][role=switch]`, 44×26 pill track). When the switch is off, a volume slider row appears using the same track style as seek.
2. **Output device**: one row per device, using a radio-card with a trailing check circle instead of a native radio. The capability summary is the description line. Refresh is a ghost icon button in the group title.
3. **Library**: one row per folder (folder chip, mono path, trailing "Remove" ghost pill), then an action row with "Add folder…" (primary pill), a "Follow links" switch and a "Rescan" pill. Scan progress is a full-width 6 px pill bar inside the card.
4. **Appearance**: the two cover styles as side-by-side preview tiles (a 96 px mini vinyl versus a square), with a 2 px accent ring when selected and `role="radiogroup"` kept.

## Unresolved questions
- Does the queue have a stable per-entry id and a remove or reorder command? Without them the row keying fix and remove button need backend work.
- Shuffle and repeat are absent from the bindings. Should the pill reserve space for them, or keep them out of scope?
- Should the pill also float over the Now Playing view, or hide there, since that view already has the large disc and tonearm?
