# UI review 1: visual design system vs Mochy-Music

Scope: tokens, colour, type, shape, elevation and component styling. Review only; no source edited.
References viewed: Mochy `app-player.webp`, `app-library.webp`, `app-home.webp`, phone shots `1.png`, `2.png`, `3.png`, `4.png`, `7.png`. No Mochy source opened.

## Diagnosis

Now Playing is already the strongest screen. The blurred-cover backdrop (`CoverBackdrop.svelte:39`) and the vinyl stage are close in spirit to Mochy's cover-tinted player (`4.png`). The rest of the app reads as a generic dark admin UI. Four things cause that:

1. **Shapes are too small and uneven.** Corner radii are 2/3/4/5/6/8px plus 999px, and most containers sit at 6px. Mochy uses a few large radii (16–28px) and pills.
2. **There is no tonal surface system.** There are only three greys (`--bg-raised/hover/active`, `app.css:3-5`), with 1px borders doing the separation work. M3 separates with tinted container levels instead.
3. **There is no type scale.** Sizes are ad hoc (30/34/17/16/13/12.5/12/11.5/10.5px), and the declared "Inter" is never bundled, so Windows silently renders Segoe UI (`app.css:18`).
4. **The accent is static and some hues are hard-coded.** Amber is hard-coded, `#1a1306`, `rgba(14,16,24,…)` and the placeholder gradients bypass tokens, so a dynamic accent cannot reach them.

## Findings

### P1

**P1-1. Faint text fails WCAG AA.** `--text-faint: #6b6f85` on `--bg: #0e1018` has a contrast ratio of about 3.8:1 (`app.css:9`). It is used for 12px text: track numbers (`TrackList.svelte:81`), disc headers (`TrackList.svelte:59`), the "ALBUM" overline (`AlbumView.svelte:99`) and the separator (`NowPlayingView.svelte:137`).
Proposal: `--on-surface-faint: #918b80` (about 5.3:1 on the new surface). Use faint only for text 12px and larger.

**P1-2. Introduce tonal surfaces and stop using borders as the primary separator.** Evidence: the sidebar `border-right` (`App.svelte:143`), the transport `border-top` (`TransportBar.svelte:105`), the queue `border-left` (`QueuePanel.svelte:44`), and input borders (`app.css:87`). Mochy motivation: `app-library.webp` and `7.png`, where panels, the search field and grouped rows are filled tonal containers with no strokes.
Proposal: use the token set below. The sidebar sits on `--surface`, the main pane on `--surface`, and the queue and transport on `--surface-container`. Delete those three borders and keep `--outline-variant` only for focus-adjacent dividers such as the Settings section rule.

**P1-3. Make the transport a floating pill with an expressive Play button.** Evidence: a full-bleed bar (`TransportBar.svelte:97-107`), a 38px white circular play button (`:163-169`), and prev/next drawn as bare icons (`:151-155`). Mochy motivation: the floating mini-player in `app-home.webp` and `app-library.webp`, and the big filled pill flanked by tonal pills in `4.png`.
```css
.bar { margin: 0 12px 12px; border-radius: var(--r-2xl); background: var(--surface-container-high);
       box-shadow: var(--elev-3); border: 0; }               /* --transport stays 84px; add 12px to toast offset */
.ctl.main { width: 56px; height: 40px; border-radius: var(--r-full);
            background: var(--primary); color: var(--on-primary); }
.ctl.prev, .ctl.next { width: 44px; height: 36px; border-radius: var(--r-full);
            background: var(--secondary-container); color: var(--on-secondary-container); }
.thumb :global(.cover) { border-radius: var(--r-md); }       /* MiniDisc is already round */
```
Recolour the seek fill to `--primary` on `--surface-container-highest`. Make the track 6px tall with a 3px radius, and keep the thumb visible on `:focus-visible`, not only on hover (`:212-216`).

**P1-4. Bundle a real font.** `"Inter"` is listed first but no `@font-face` exists anywhere in `src/`, so the app falls back to the OS font (`app.css:18`). Mochy's headings are heavy, tight grotesk (`3.png` title, `app-library.webp` "Library").
Proposal: ship **Inter Variable** (OFL, Latin plus Vietnamese subsets) as woff2 in `app/src/assets/fonts/`. Load it with `@font-face { font-family: "Inter"; src: url(./assets/fonts/InterVariable.woff2) format("woff2"); font-weight: 100 900; font-display: swap; }`. Turn on `font-feature-settings: "cv11", "ss01"` for display sizes. Vite fingerprints the file, it is served from `'self'`, and it adds no runtime npm dependency. As a build-time alternative, `@fontsource-variable/inter` can be a devDependency.

### P2

**P2-1. Use pills for navigation.** `.nav` has a 6px radius and 9px padding, and its active state is only a darker grey (`App.svelte:160-177`). Mochy motivation: the pill active indicator in the bottom nav of `app-library.webp`.
Proposal: `height: 44px; border-radius: var(--r-full); padding: 0 16px;`. Active state: `background: var(--secondary-container); color: var(--on-secondary-container)`. Keep the icon in `--primary`. Make the back button (`App.svelte:207-221`) a 40px tonal circle.

**P2-2. Enlarge the album header and its actions.** Evidence: the title is 34px at weight 650 (`AlbumView.svelte:101-104`), the meta line is `·`-joined dim text (`:50-54`), and buttons are 8×14 padding (`app.css:100-108`). Mochy motivation: the `3.png` hero, with a large bold title, a filled "Play" pill between tonal pills, and "Album • 2019 • 5 Tracks • 17m".
Proposal: keep the side-by-side layout on desktop, but move the art to 248px with `--r-xl`. Set the title in `--type-display` (44px/800). Join the meta line with " • " and render it in `--type-title-md` in `--on-surface-variant`. Make `.btn` `height: 44px; padding: 0 22px; border-radius: var(--r-full)`, with `.btn.primary` using `--primary/--on-primary` and the plain `.btn` using `--secondary-container`. Show the album's best-quality badge (for example `FLAC 24/96`) in the meta row, because this is an audiophile product.

**P2-3. Group track rows into cards.** Rows are flat with a 6px radius and a hover tint only (`TrackList.svelte:61-75`). Mochy motivation: the grouped list in `2.png` and `7.png`, where each row is a tonal block with 2–4px gaps, the outer corners are large and the inner corners small.
```css
.list { gap: 2px; }
.row { background: var(--surface-container-low); border-radius: var(--r-xs); padding: 10px 16px; min-height: 52px; }
.row:first-of-type { border-top-left-radius: var(--r-xl); border-top-right-radius: var(--r-xl); }
.row:last-of-type  { border-bottom-left-radius: var(--r-xl); border-bottom-right-radius: var(--r-xl); }
.row:hover { background: var(--surface-container); }
.row.playing { background: var(--primary-container); }      /* .t/.no -> --on-primary-container */
```
For multi-disc albums, restart the group at each `.disc` header.

**P2-4. Make the format badges first-class chips.** Badges have a 4px radius (`app.css:139`), and the compact variant drops to 10.5px (`FormatBadges.svelte:39-42`), which is the smallest text in the app and sits in the most-seen place. Mochy motivation: the outlined "OPUS" chip placed right after the title in `app-player.webp`.
Proposal: `.badge { border-radius: var(--r-sm); padding: 3px 10px; font-size: 12px; }`, with a compact minimum of 11.5px. Give "Bit-perfect" a leading 6px dot or check icon and `--ok-container` fill, so it reads as a status rather than a label. On Now Playing, place the source codec chip inline after the `h1` (Mochy pattern) and keep the full source → device → bit-perfect row below it.

**P2-5. Tokenise the Now Playing backdrop and drive it from the seed colour.** The shade hard-codes `rgba(14,16,24,…)` (`CoverBackdrop.svelte:47-52`). Mochy motivation: the `4.png` vertical wash, cover-tinted at the top and fading to near-black at the controls.
```css
.shade { background:
  radial-gradient(ellipse at 30% 40%, transparent, color-mix(in oklab, var(--surface) 55%, transparent) 70%),
  linear-gradient(to bottom, color-mix(in oklab, var(--seed) 22%, transparent), color-mix(in oklab, var(--surface) 88%, transparent)); }
```
Keep `prefers-reduced-transparency` hiding the image (`:54-58`); the seed gradient alone still gives the tint.

**P2-6. Increase cover radii and replace heavy shadows with tonal elevation.** `Cover` uses `--radius: 8px` and applies a 0 6px 18px shadow everywhere, including grid cells (`Cover.svelte:27-30`). The queue overrides it to 4px (`QueuePanel.svelte:95`) and the transport to 5px (`TransportBar.svelte:119`). Mochy motivation: the roughly 20px rounded cards in `app-home.webp`.
Proposal: grid covers `--r-lg` (16px), hero covers `--r-xl` (24px), list thumbnails `--r-sm` (8px). No shadow at rest; apply `--elev-2` on hover only (`AlbumCard.svelte:18-20` already does this).

### P3

- **Inputs and search.** Inputs have a 6px radius and a 1px border (`app.css:82-90`; `SearchView.svelte:56-57`). Make search a pill field at 48px height on `--surface-container-high` with no border (`app-library.webp`).
- **Settings choices.** The option cards have a 6px radius, and the selected state is a barely visible `--accent-dim` border (`SettingsView.svelte:304-317, 339-355`). Make them a segmented control or selectable cards at `--r-lg`, with the selected state on `--secondary-container` plus a check icon (`7.png`, "Auto | High | Low"). The progress track radius of 3px (`SettingsView.svelte:263`) should become `--r-full`.
- **Toasts.** Toasts use an 8px radius with a coloured left stripe (`App.svelte:232-244`). Use an M3 snackbar instead: `--r-md`, `--surface-container-highest`, and a leading status icon in place of the stripe.
- **Hard-coded hex.** The placeholder gradients (`Cover.svelte:44`, `VinylDisc.svelte:162`) and the on-accent `#1a1306` (`app.css:114`, `AlbumCard.svelte:29`) should become tokens (`--surface-container-high` to `--surface-container-low`, and `--on-primary`).
- **Duration format.** `format.ts` only has `duration()` (h:mm:ss). Add `durationShort(ms)` returning "17 min" / "1 h 12 min" for album meta (`3.png` "17m").
- **Type roles.** Headings use a single weight, 650 (`app.css:96`). Move them to roles: display 800, headline 750, title 650.

## Proposed tokens (dark default; amber seed, hue ≈ 75)

```css
:root {
  /* seed: replaced at runtime by the cover colour (see below) */
  --seed: #e8b04a;
  /* tonal surfaces: near-black neutrals with a whisper of seed hue */
  --surface-lowest: #0c0b0a;  --surface: #121110;
  --surface-container-low: #1a1917;  --surface-container: #1f1d1b;
  --surface-container-high: #292724; --surface-container-highest: #34322e;
  --outline-variant: #4a463f;  --outline: #958f84;
  --on-surface: #ebe6df;  --on-surface-variant: #cdc6ba;  --on-surface-faint: #918b80;
  /* roles */
  --primary: #e8b04a;  --on-primary: #2a1a00;
  --primary-container: #5c4100;  --on-primary-container: #ffdea6;
  --secondary-container: #4d4535;  --on-secondary-container: #f0e0c8;
  --ok: #6fcf97; --ok-container: #1d3b2a; --warn: #f2c94c; --warn-container: #3d3212; --error: #ffb4ab;
  /* shape */
  --r-xs: 4px; --r-sm: 8px; --r-md: 12px; --r-lg: 16px; --r-xl: 24px; --r-2xl: 28px; --r-full: 999px;
  /* elevation (dark M3 = tone first, shadow only for floating) */
  --elev-1: 0 1px 2px rgb(0 0 0 / .30);
  --elev-2: 0 2px 6px rgb(0 0 0 / .30), 0 8px 20px rgb(0 0 0 / .25);
  --elev-3: 0 10px 30px rgb(0 0 0 / .45), 0 2px 6px rgb(0 0 0 / .30);
}
```
Keep the old names as aliases during migration (`--bg: var(--surface)`, `--accent: var(--primary)`, and so on) so the change can land file by file.

**Type scale** (14px base; Inter Variable; tabular numbers on times and badges):

| Role | Size / line height | Weight | Tracking |
|---|---|---|---|
| display | 44 / 1.05 | 800 | -0.03em |
| headline-lg | 34 / 1.1 | 750 | -0.02em |
| headline-md | 24 / 1.2 | 700 | -0.015em |
| title-lg | 18 / 1.3 | 650 | — |
| title-md | 15 / 1.35 | 600 | — |
| body | 14 / 1.5 | 400 | — |
| body-sm | 12.5 / 1.45 | 400 | — |
| label | 12 / 1.3 | 600 | +0.02em |
| overline | 11 / 1.2 | 700 | +0.08em, uppercase |

## Dynamic accent from the cover

The obvious front-end path is blocked. Covers come from `http://sonder-art.localhost` (`lib/art.ts:5-7`), which is a different origin from the app page. `art.rs:38-42` sends no `Access-Control-Allow-Origin` header, so drawing the image to a canvas taints it and `getImageData` throws. CSP itself is not the obstacle, because `img-src` already allows the scheme.

- **Recommended: compute the colour in Rust.** At art-cache time, `art.rs` already decodes the image to write the 300/1000 thumbnails. Downscale to 32×32 and pick the most chromatic of the populous colours, or run a simple k-means with k=4 and score each cluster by population × chroma. Store `seed: u32` beside the hash and expose it as `art_color` in the DTO (`dto.rs:65`). This costs nothing at runtime, needs no new dependency (the `image` crate is already used for thumbnails), and the colour is available before the image loads, so the colour can transition while the cover fades in.
- **Fallback: sample in the front end.** Add `Access-Control-Allow-Origin: *` to the art response, set `crossorigin="anonymous"` on a hidden 300px `<img>`, and draw it to a 32×32 canvas. This is cheaper to build, but it runs on every track change and the cross-origin header loosens isolation slightly.
- **Deriving the roles.** Set `--seed` on `.shell`. WebView2 (Chromium) and Safari 18+ support relative colour syntax:
  `--primary: oklch(from var(--seed) clamp(0.74, l, 0.84) clamp(0.06, c, 0.15) h);`
  `--primary-container: oklch(from var(--seed) 0.36 min(c, 0.09) h);`
  `--surface-container: oklch(from var(--seed) 0.22 0.012 h);` (apply the same pattern to the other surface levels).
  The lightness clamp keeps `--primary` at 4.5:1 or better on surfaces. If the seed chroma is below 0.04 (a greyscale cover), keep the amber brand seed. Wrap the derived values in `@supports (color: oklch(from red l c h))` so older WebKitGTK keeps the static tokens.
- **Motion.** Use `@property --seed { syntax: "<color>"; inherits: true; initial-value: #e8b04a; }` with `transition: --seed var(--dur-slow)`. Under reduced motion the existing 1ms override already applies.
- **Scope.** Apply the seed app-wide at low intensity (surfaces and primary) and at full strength on Now Playing (backdrop wash, tonearm and ring-text accent, seek fill). Leave the semantic badge colours (ok/warn) static so bit-perfect status never changes hue with the artwork.

## Unresolved questions

1. Should the dynamic accent recolour the whole app, or only Now Playing and the transport? Recommendation: whole app at low intensity.
2. Is moving the transport into a floating pill acceptable, given it costs about 12px of vertical space at the 600px minimum height?
3. Should Now Playing get its own large expressive control row (as in Mochy `4.png`), or keep controls only in the transport? That is a layout question for the layout reviewer.
4. Is the Rust-side palette extraction in scope for this plan? It touches `art.rs`, the DB schema and the DTO.
