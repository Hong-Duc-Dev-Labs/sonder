# UI review 3: library browsing (Albums, Album, Artists, Artist, Search, track lists)

## Diagnosis

The browsing screens are correct and fast (virtualised grid, cover morph, debounced search) but read as a
utility skeleton, not a record shop. Hierarchy is flat: every page opens with the same 12px kicker plus a
28-34px heading, the Artist page has no hero at all, and track lists are borderless rows whose only
affordance is a tooltip saying "double-click". The audiophile signal (codec/bits/rate) is repeated on every
row instead of summarised once. Loading states are mostly blank, and the only vinyl reference in these
screens is the `disc` placeholder icon. Mochy's language (hero that bleeds into the page, heavy display type,
pill actions, grouped rounded track cards with thumbnail and a dimmed "artist • 3:50" line) can be adopted
almost entirely from data Sonder already has; only artist artwork needs a workaround.

## Findings

### P1 (shape of the screens)

**1. Album header is small and quiet.** `AlbumView.svelte:39-64, 84-104`: 232px cover, 34px title, one dim
meta line, two pills. There is no quality summary, which is the first thing an audiophile looks for. Ref:
`3.png` (large cover, bold title, filled "Play" with a tonal pill and a round icon button, then a centred
"Album • 2019 • 5 Tracks • 17m" line).
Proposal: use the hero layout in "Album page" below. Cover `width: clamp(220px, 28vw, 300px)` keeps the
`.art` element and its `view-transition-name`. Title `font-size: clamp(32px, 4vw, 48px); font-weight: 800;
letter-spacing: -0.025em; line-clamp: 2`. Meta line `2019 · 12 tracks · 54 min · Rock` (genre from the first
track). Add a quality chip from `info.tracks`: `FLAC 24/96` if every track matches, otherwise `Mixed · up to
24/192`. Actions: filled `Play`, tonal `Shuffle` (client-side: shuffle `info.tracks` ids, then
`playerPlayTracks`), and a round 40px tonal icon button for "Add to queue" with a `title`/`aria-label`.
Backdrop: an absolutely positioned `<img>` of the same cover (`size 300`, already cached) with
`filter: blur(60px) saturate(1.4); opacity: .35`, masked by `linear-gradient(to bottom, #000 30%, transparent)`
across about 420px. That is the dark-theme counterpart to Mochy's tinted top area.

**2. TrackList has no affordance and too much noise.** `TrackList.svelte:26-44`: play is double-click only
(`:29`, with `title="Double-click to play"` at `:31`), there is no hover play glyph and no overflow, and the
badge at `:42` repeats an identical `FLAC 16/44.1` on every row of a typical album. Rows (`:61-72`) are flat
6px-radius strips with no grouping. Ref: `5.png` / `2.png` (grouped cards: the container is rounded, rows
are separated by 2px gaps, bold title, dim "artist • 3:50", and a kebab menu).
Proposal:
- Add a `variant: "album" | "search"` prop (replacing `showAlbum`). In `album`, show the number column,
  which turns into a play icon on row hover/focus. A single click on that icon plays. Double-click
  anywhere on the row still plays.
- Show the per-row quality badge only when the track differs from the album's dominant format; make it
  `.badge.accent` when the track is hi-res (bits > 16 or rate > 48k).
- Grouped surface: `.list{background:var(--bg-raised);border-radius:14px;padding:4px;gap:2px}`. Rows become
  `padding:10px 14px;border-radius:10px`, and hover uses `--bg-hover`.
- Playing row: replace the number with a 16px spinning `MiniDisc` (or three CSS EQ bars), paused = still.
- Add a trailing 32px overflow button (`⋯`) that opens a small menu with Play next / Add to queue /
  Go to album / Go to artist. This needs a new `more` icon path in `Icon.svelte`.

**3. Artist page has no hero and no actions.** `ArtistView.svelte:31-40` renders a kicker, a 34px h1, then
grids. There is no count, no Play/Shuffle, and the page looks unfinished next to `2.png`. See the "Artist
page" layout below for the hero proposal. Actions are `Play all` and `Shuffle`. `ArtistInfo` has no
track list, so a small `libraryArtistTracks(id)` command is the clean route; looping `libraryAlbum` per album
on the client also works.

**4. Loading = blank page, and the Search empty state is wrong.** `AlbumView.svelte:65-69` shows nothing until
tracks arrive. Its header is also missing whenever no preview exists (for example, navigating in from
outside a card). `ArtistView.svelte:30` and `ArtistsView.svelte:30` are blank until load. `SearchView.svelte:43`
shows "No matches." during the 150ms debounce, because results are cleared or stale while `searchQuery` is
already non-empty. Proposal: render `header.trackCount` skeleton rows (36px, `--bg-raised`, shimmer
disabled under reduced motion) so the list never jumps. Track a `pending` flag in Search and show
"No matches" only once the request for the current query has resolved.

### P2 (consistency and density)

**5. Search is a bare track dump.** `SearchView.svelte:35-42`. Ref: `app-search.webp` (large title, full-pill
search field, sectioned results). Proposal: give the search field a leading search icon,
`border-radius:999px;height:44px;padding-left:42px`, and `width:min(640px,100%)`. Group the results into
**Albums** first (dedupe `results` by `albumId` into a horizontal row of up to 8 small cards using
`TrackItem.art`), then **Tracks** using `variant="search"`: a 40px cover thumbnail, bold title, and dim
`artist • album`. When the query is empty, show a centred hint ("Search your library: tracks, albums,
artists" plus a `Ctrl+F` keycap) instead of whitespace.

**6. Artists list is a wall of initials.** `ArtistsView.svelte:33, 69-79`: a 36px letter avatar in accent on
`--bg-active` for every artist, with no section anchors. Proposal: add `art: string | null` (newest album
cover hash) to `Artist` in the backend, and show a 44px round cover avatar. As a fallback, put the letter
on a hue hashed from the name at low chroma. Also add alphabetical sticky headers (`A`, `B`, ...) and
empty/no-match messages for the filter. Make the filter the same pill field as Search.

**7. Sort control is a native `<select>`.** `AlbumsView.svelte:22-29`. Ref: chip row in `app-library.webp`.
Proposal: four pill chips (`Artist · Title · Recently added · Year`) with `aria-pressed`. The active chip
is `--bg-active` with `--text`; the rest are `--text-dim`. Put a dim subtitle `1,284 albums` under the
h1 (from `total`, which would need to be lifted out of `AlbumGrid` via a bindable prop).

**8. Duplicated page scaffolding and type scale.** `.page{padding:8px 32px 32px}` is copied in four files
(`AlbumView:74`, `ArtistView:45`, `ArtistsView:42`, `SearchView:49`), and `.kind` is copied at
`AlbumView:94`/`ArtistView:49`. The h1 is 34px on detail pages and the browser default 28px on list
pages. Proposal: add `.page`, `.kicker`, `--fs-display: clamp(32px,4vw,48px)`, `--fs-title: 28px`,
`--radius-lg: 14px` to `app.css`, and use them everywhere.

**9. Album card text is thin.** `AlbumCard.svelte:79-93`. Proposal: add the year to the artist line
(`Artist · 2019`, which stays on one line so `TEXT_HEIGHT` 52 in `AlbumGrid.svelte:14` stays valid). Raise
the cover radius to `--radius-lg` to match Mochy's softer corners. Give the skeleton (`AlbumGrid:120`) a
slow shimmer, and add two text bars so loading rows have the final shape.

### P3 (polish)

**10.** The multi-disc divider (`TrackList.svelte:23-25, 53-60`) should get the `disc` icon plus a per-disc
duration, e.g. "Disc 2 · 41 min". **11.** The `Cover` placeholder (`Cover.svelte:38-45`) should look like
a vinyl label: a radial gradient with a 6% spindle hole, instead of a flat blue gradient plus an icon.
**12.** The artist link in the album header (`AlbumView:46`) should get an underline on hover, not only the
accent colour, so it does not rely on colour alone (WCAG 1.4.1). **13.** In the album list, the dim
"artist" sub-line (`TrackList:36`) appears only for guest artists. That is good; keep it in the
`artist • 3:50` Mochy style, but keep the duration in its own right-aligned column so it scans on desktop.

## Proposed layouts (desktop, content width 690-1400px)

Album page. The cover stays the morph target; the record is a sibling so it is not part of the snapshot.
```
 [<-]  .backdrop: same cover, blur 60px, opacity .35, fades to --bg by ~420px
 ┌───────────────┐_                ALBUM
 │               │ ╲  record       Title In Heavy Display Type
 │  .art (vt)    │  ) peeks 22%    Artist Name  (link, 17px/600)
 │  clamp 220-300│ ╱  right        2019 · 12 tracks · 54 min · Rock
 └───────────────┘‾                [FLAC 24/96]  (accent chip if hi-res)
                                   ( ▶ Play )( ⤮ Shuffle )( ≡+ )( ⋯ )
 ╭──────────────────────────────────────────────────────────────────╮
 │  1  Track title                                        3:50   ⋯ │
 │  2  Track title · feat. Guest                 [24/192] 4:12   ⋯ │
 │ (◉) Now playing title (accent)                         5:01   ⋯ │
 ╰──────────────────────────────────────────────────────────────────╯
```
Below 760px of content width (900px window with the queue panel open), stack the layout: a centred cover
(240px), then a centred title, meta and actions. That is Mochy's `3.png` arrangement, done with a
container query on `.page` (`container-type: inline-size`).

Artist page. There is no artist photo, so the hero is built from the artist's own covers.
```
 ┌────────────────────────────────────────────────────────────────┐
 │ [<-]  mosaic: up to 6 covers (albums then appearsOn), 3x2 grid │
 │       rotate(-6deg) scale(1.25), blur(28px) saturate(1.3),     │
 │       opacity .55; mask fades to --bg over the bottom 55%      │
 │  ◉ 96px vinyl avatar (newest cover as label)                   │
 │  ARTIST                                                        │
 │  Artist Name  (clamp(40px,6vw,72px), 800, -0.03em)             │
 │  8 albums · 3 appearances · 112 tracks                         │
 │  ( ▶ Play all )( ⤮ Shuffle )                                   │
 └────────────────────────────────────────────────────────────────┘
   Albums                              (sorted by year, newest first)
   [card][card][card][card][card]
   Appears on
   [card][card]
```
Hero height `clamp(260px, 36vh, 360px)`. With a single cover, use it full-bleed (`object-fit:cover`,
blur 40px) instead of the mosaic. With none, use a radial accent glow on `--bg-raised`. All of the
mosaic images are `size 300` URLs already cached by the grid, so the cost is one extra composited layer.
The back button keeps its absolute position and floats over the hero; give it
`background: rgb(0 0 0 / .35); backdrop-filter: blur(8px)`.

## Vinyl touches (cost estimates)

| Touch | How | Cost |
|---|---|---|
| Record peeks from sleeve on card hover | In `AlbumCard`, a `.record` sibling *behind* `.art` (not inside, so the view-transition snapshot excludes it). CSS-only disc: `repeating-radial-gradient` grooves, with a label of `background-image: url(cover@300)` at 34%. On `.card:hover` apply `translateX(18%) rotate(25deg)` with `--ease-spring`. 18% of a 170px card is about 31px, so let it run under the neighbour or cap it at `GAP`. | ~1.5h, CSS only, no JS. One extra layer per visible card. |
| Half-out record in album header | Same disc behind the header cover, static at 22%. It rotates (label only, per phase 02) while this album is playing. Reuse `VinylDisc`/`MiniDisc` once phase 02 lands. | ~2h after phase 02 |
| Spinning mini-disc as playing-row marker | 16px `MiniDisc` in the number column, with `animation-play-state` tied to paused. | ~0.5h |
| Vinyl artist avatars | Round avatar drawn as a record whose label is the newest cover (needs `Artist.art`). | ~1h UI + small backend change |
| Vinyl-label cover placeholder | Radial-gradient label with a spindle hole in `Cover.svelte`. | ~0.5h |
| Grain overlay on hero backdrops | A 2% SVG `feTurbulence` noise layer to kill banding on blurred covers. | ~0.5h |

No new npm dependencies are needed; all of it is CSS plus existing components. Everything must respect
`prefers-reduced-motion`, which the global rule in `app.css:34-43` already shortens. The peek should also
become an opacity-only fade there.

## Unresolved questions

1. Is a backend change acceptable for `Artist.art` and `libraryArtistTracks(id)`, or should the artist page
   derive everything client-side?
2. Should the overflow menu ship with this work (it needs "Play next", which has no command yet), or stay
   out of scope with only the hover-play button?
3. Should the hi-res badge use the accent (amber) colour, or a separate token, so it does not compete with
   the now-playing highlight, which also uses `--accent-dim`?
