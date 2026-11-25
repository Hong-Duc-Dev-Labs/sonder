<script lang="ts">
  // Tracks as one grouped soft card. A row plays on double-click, or through its play button
  // (shown in place of the number on hover and focus, and always reachable by keyboard and
  // screen readers). The format badge only shows where a track differs from the album; a
  // checked track also shows its quality verdict.
  // With `onmove`, each row has a grip: drag it, or press Up/Down on it, to reorder.
  // Dragging a row anywhere else carries the track to a playlist in the sidebar, and a
  // right-click opens the row's ⋯ menu at the pointer.
  import { tick } from "svelte";
  import { app } from "../lib/app.svelte";
  import type { TrackItem } from "../lib/bindings";
  import { duration, rate } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { trackDrag } from "../lib/trackDrag.svelte";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";
  import MiniDisc from "./MiniDisc.svelte";
  import QualityBadge from "./QualityBadge.svelte";
  import RowMenu from "./RowMenu.svelte";

  let {
    tracks,
    onplay,
    variant = "album",
    onremove,
    onmove,
  }: {
    tracks: TrackItem[];
    onplay: (index: number) => void;
    /** "search" and "playlist" rows show the cover, artist and album of each track. */
    variant?: "album" | "search" | "playlist";
    /** Adds "Remove from playlist" to each row's menu. */
    onremove?: (index: number) => void;
    /** Moves the row at `from` to index `to`; the caller reorders `tracks` right away. */
    onmove?: (from: number, to: number) => void;
  } = $props();

  let mixed = $derived(variant !== "album");
  /** Row keys that stay unique when a track appears more than once (playlists). */
  let keys = $derived.by(() => {
    const seen = new Map<number, number>();
    return tracks.map((t) => {
      const n = seen.get(t.id) ?? 0;
      seen.set(t.id, n + 1);
      return `${t.id}#${n}`;
    });
  });

  let rows: HTMLElement[] = $state([]);
  let menus: RowMenu[] = $state([]);

  /** A press on the row itself, not on one of its buttons, may start a drag to a playlist. */
  function rowDown(e: PointerEvent, track: TrackItem) {
    if ((e.target as HTMLElement).closest("button")) return;
    trackDrag.press(e, track);
  }

  function rowMenu(e: MouseEvent, i: number) {
    e.preventDefault();
    menus[i]?.openAt(e.clientX, e.clientY);
  }
  /** The row being dragged by its grip, where it would land, and how far it has moved. */
  let drag = $state<{ from: number; to: number; dy: number; height: number; startY: number } | null>(null);
  /** Each row's middle when the drag started, before any row moved aside. */
  let middles: number[] = [];

  function gripDown(e: PointerEvent, i: number) {
    if (e.button !== 0 || !onmove) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    middles = tracks.map((_, j) => {
      const r = rows[j]?.getBoundingClientRect();
      return r ? r.top + r.height / 2 : Number.NaN;
    });
    const height = rows[i]?.getBoundingClientRect().height ?? 0;
    drag = { from: i, to: i, dy: 0, height, startY: e.clientY };
  }

  function gripMove(e: PointerEvent) {
    if (!drag) return;
    // The landing index counts the other rows whose resting middle is above the pointer.
    const from = drag.from;
    const to = middles.filter((m, j) => j !== from && e.clientY > m).length;
    drag = { ...drag, to, dy: e.clientY - drag.startY };
  }

  function gripUp() {
    if (!drag) return;
    const { from, to } = drag;
    drag = null;
    if (from !== to) onmove?.(from, to);
  }

  async function gripKey(e: KeyboardEvent, i: number) {
    const step = e.key === "ArrowUp" ? -1 : e.key === "ArrowDown" ? 1 : 0;
    const to = i + step;
    if (!step || !onmove || to < 0 || to >= tracks.length) return;
    e.preventDefault();
    onmove(i, to);
    await tick();
    rows[to]?.querySelector<HTMLElement>(".grip")?.focus();
  }

  /** How far row `j` moves aside to make room for the dragged row. */
  function shift(j: number): number {
    if (!drag || j === drag.from) return 0;
    if (drag.from < j && j <= drag.to) return -drag.height;
    if (drag.to <= j && j < drag.from) return drag.height;
    return 0;
  }

  let playingId = $derived(app.current?.trackId ?? null);
  let multiDisc = $derived(variant === "album" && new Set(tracks.map((t) => t.discNo ?? 1)).size > 1);

  function quality(t: TrackItem): string {
    if (t.codec === "dsf" || t.codec === "dff") return `DSD${Math.round(t.rate / 44100)}`;
    return `${t.bits ?? "?"}/${rate(t.rate).replace(" kHz", "")}`;
  }
  const key = (t: TrackItem) => `${t.codec}|${quality(t)}`;

  /** The album's most common format; rows in it carry no badge. */
  let common = $derived.by(() => {
    if (variant !== "album") return null;
    const counts = new Map<string, number>();
    for (const t of tracks) counts.set(key(t), (counts.get(key(t)) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? null;
  });

  function isHiRes(t: TrackItem): boolean {
    return (t.bits ?? 0) > 16 || t.rate > 48_000 || t.codec === "dsf" || t.codec === "dff";
  }

  function discLength(disc: number): string {
    return duration(tracks.filter((t) => (t.discNo ?? 1) === disc).reduce((s, t) => s + t.durationMs, 0));
  }
</script>

<div class="list soft" class:sorting={drag} role="list">
  {#each tracks as track, i (keys[i])}
    {#if multiDisc && (i === 0 || (tracks[i - 1].discNo ?? 1) !== (track.discNo ?? 1))}
      <div class="disc" role="presentation">
        <Icon name="disc" size={14} /> {t("tracks.disc", { n: track.discNo ?? 1 })} · {discLength(track.discNo ?? 1)}
      </div>
    {/if}
    {@const playing = track.id === playingId}
    {@const fav = app.isFavorite(track.path)}
    <div
      bind:this={rows[i]}
      class="row"
      class:playing
      class:mixed
      class:movable={onmove}
      class:dragging={drag?.from === i}
      class:carried={trackDrag.active?.track.id === track.id}
      style:transform={drag ? `translateY(${drag.from === i ? drag.dy : shift(i)}px)` : undefined}
      role="listitem"
      aria-current={playing ? "true" : undefined}
      ondblclick={() => onplay(i)}
      oncontextmenu={(e) => rowMenu(e, i)}
      onpointerdown={(e) => rowDown(e, track)}
      onpointermove={(e) => trackDrag.move(e)}
      onpointerup={() => trackDrag.release()}
      onpointercancel={() => trackDrag.cancel()}
    >
      {#if onmove}
        <button
          class="grip"
          aria-label={t("tracks.move", { title: track.title, position: i + 1, total: tracks.length })}
          aria-keyshortcuts="ArrowUp ArrowDown"
          title={t("tracks.reorderHint")}
          onpointerdown={(e) => gripDown(e, i)}
          onpointermove={gripMove}
          onpointerup={gripUp}
          onpointercancel={() => (drag = null)}
          onkeydown={(e) => gripKey(e, i)}
        >
          <Icon name="grip" size={18} />
        </button>
      {/if}
      <span class="lead">
        {#if playing}
          <span class="spin"><MiniDisc art={track.art} paused={app.player.state !== "playing"} /></span>
        {:else if mixed}
          <span class="thumb"><Cover hash={track.art} alt="" /></span>
        {:else}
          <span class="no">{track.trackNo ?? ""}</span>
        {/if}
        <button
          class="play"
          aria-label={track.artist
            ? t("common.playItemBy", { title: track.title, artist: track.artist })
            : t("common.playItem", { title: track.title })}
          onclick={() => onplay(i)}
        >
          <Icon name="play" size={16} />
        </button>
      </span>
      <span class="main">
        <span class="t">{track.title}</span>
        {#if mixed}
          <span class="sub">{track.artist ?? track.albumArtist ?? ""} • {track.album}</span>
        {:else if track.artist && track.artist !== track.albumArtist}
          <span class="sub">{track.artist}</span>
        {/if}
      </span>
      <span class="tags">
        <QualityBadge {track} />
        {#if key(track) !== common}
          <span class="badge" class:accent={isHiRes(track)}>{track.codec.toUpperCase()} {quality(track)}</span>
        {/if}
      </span>
      <button
        type="button"
        class="fav icon-btn"
        class:on={fav}
        aria-label={fav ? t("favorites.remove") : t("favorites.add")}
        aria-pressed={fav}
        onclick={(e) => {
          e.stopPropagation();
          void app.toggleFavorite(track.path);
        }}
      >
        <Icon name={fav ? "heart" : "heart-outline"} size={16} />
      </button>
      <span class="dur">{duration(track.durationMs)}</span>
      <RowMenu bind:this={menus[i]} {track} showAlbum={mixed} onremove={onremove ? () => onremove(i) : undefined} />
    </div>
  {/each}
</div>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px;
    /* Concentric with the rows inside: their radius plus the padding between them. */
    border-radius: calc(var(--r-sm) + 6px);
  }
  .disc {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 14px 14px 6px;
    font-size: var(--fs-label);
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--ink-3);
  }
  .disc:first-child {
    margin-top: 8px;
  }
  .row {
    display: grid;
    grid-template-columns: 40px minmax(0, 1fr) auto 32px 52px 34px;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    padding: 4px 6px 4px 8px;
    border-radius: var(--r-sm);
    transition: background var(--dur-fast) var(--ease-standard);
  }
  .row.mixed {
    grid-template-columns: 44px minmax(0, 1fr) auto 32px 52px 34px;
  }
  .row.mixed.movable {
    grid-template-columns: 24px 44px minmax(0, 1fr) auto 32px 52px 34px;
    padding-left: 2px;
  }
  /* While dragging, rows making room slide and the dragged row follows the pointer. On drop
     the list reorders at once with no transition, so nothing slides back. */
  .sorting .row {
    transition:
      background var(--dur-fast) var(--ease-standard),
      transform var(--dur-med) var(--ease-standard);
  }
  .sorting .row.dragging {
    position: relative;
    z-index: 1;
    background: var(--soft-active);
    box-shadow: 0 10px 28px rgb(0 0 0 / 0.45);
    transition: none;
  }
  .grip {
    display: grid;
    place-items: center;
    height: 40px;
    border-radius: var(--r-xs);
    color: var(--ink-3);
    cursor: grab;
    touch-action: none;
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-standard);
  }
  /* The grip only shows on the row in hand, so a resting list reads as plain tracks. */
  .row:hover .grip,
  .row:focus-within .grip,
  .sorting .grip {
    opacity: 1;
  }
  .grip:hover,
  .grip:focus-visible {
    color: var(--ink);
  }
  .dragging .grip {
    cursor: grabbing;
  }
  .row:hover,
  .row:focus-within {
    background: var(--soft);
  }
  .row.carried {
    opacity: 0.45;
  }
  .row.playing {
    background: var(--soft-active);
  }
  .lead {
    position: relative;
    display: grid;
    place-items: center;
    height: 40px;
  }
  .no {
    color: var(--ink-3);
    font-family: var(--font-figures);
    font-variant-numeric: tabular-nums;
  }
  .spin {
    width: 30px;
  }
  .thumb {
    width: 40px;
  }
  .thumb :global(.cover) {
    border-radius: var(--r-xs);
    box-shadow: none;
  }
  /* On hover the play icon takes the lead's place: over the number it stands alone, over a
     cover or the spinning disc it sits on a dark scrim, so the art stays visible beneath. */
  .play {
    position: absolute;
    inset: 0;
    width: 40px;
    height: 40px;
    margin: auto;
    display: grid;
    place-items: center;
    border-radius: var(--r-xs);
    color: var(--ink);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-standard);
  }
  .play :global(svg) {
    transform: scale(0.8);
    transition: transform var(--dur-med) var(--ease-spring);
  }
  .mixed .play {
    background: rgb(0 0 0 / 0.55);
  }
  .playing .play {
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: rgb(0 0 0 / 0.55);
  }
  .row:hover .play,
  .play:focus-visible {
    opacity: 1;
  }
  .row:hover .play :global(svg),
  .play:focus-visible :global(svg) {
    transform: none;
  }
  .no {
    transition: opacity var(--dur-fast) var(--ease-standard);
  }
  .row:hover .no,
  .row:has(.play:focus-visible) .no {
    opacity: 0;
  }
  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .tags {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
  }
  .t,
  .sub {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .t {
    font-weight: 600;
  }
  .playing .t {
    font-weight: 700;
  }
  .sub {
    font-size: var(--fs-small);
    color: var(--ink-2);
  }
  .dur {
    text-align: right;
    color: var(--ink-2);
    font-family: var(--font-figures);
    font-variant-numeric: tabular-nums;
  }
  .fav {
    width: 30px;
    height: 30px;
    opacity: 0;
    color: var(--ink-2);
    transition:
      opacity var(--dur-fast) var(--ease-standard),
      color var(--dur-fast) var(--ease-standard);
  }
  .fav.on {
    opacity: 1;
    color: #ff7b89;
  }
  .row:hover .fav,
  .row:focus-within .fav {
    opacity: 1;
  }
  .row :global(.more) {
    opacity: 0;
  }
  .row:hover :global(.more),
  .row:focus-within :global(.more),
  .row :global(.more[aria-expanded="true"]) {
    opacity: 1;
  }
</style>
