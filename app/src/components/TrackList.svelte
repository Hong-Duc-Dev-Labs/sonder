<script lang="ts">
  // Tracks as one grouped soft card. A row plays on double-click, or through its play button
  // (shown in place of the number on hover and focus, and always reachable by keyboard and
  // screen readers). The quality badge only shows where a track differs from the album.
  import { app } from "../lib/app.svelte";
  import type { TrackItem } from "../lib/bindings";
  import { duration, rate } from "../lib/format";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";
  import MiniDisc from "./MiniDisc.svelte";
  import RowMenu from "./RowMenu.svelte";

  let {
    tracks,
    onplay,
    variant = "album",
  }: { tracks: TrackItem[]; onplay: (index: number) => void; variant?: "album" | "search" } = $props();

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

<div class="list soft" role="list">
  {#each tracks as track, i (track.id)}
    {#if multiDisc && (i === 0 || (tracks[i - 1].discNo ?? 1) !== (track.discNo ?? 1))}
      <div class="disc" role="presentation">
        <Icon name="disc" size={14} /> Disc {track.discNo ?? 1} · {discLength(track.discNo ?? 1)}
      </div>
    {/if}
    {@const playing = track.id === playingId}
    <div
      class="row"
      class:playing
      class:search={variant === "search"}
      role="listitem"
      aria-current={playing ? "true" : undefined}
      ondblclick={() => onplay(i)}
    >
      <span class="lead">
        {#if playing}
          <span class="spin"><MiniDisc art={track.art} paused={app.player.state !== "playing"} /></span>
        {:else if variant === "search"}
          <span class="thumb"><Cover hash={track.art} alt="" /></span>
        {:else}
          <span class="no">{track.trackNo ?? ""}</span>
        {/if}
        <button
          class="play"
          aria-label="Play {track.title}{track.artist ? ` by ${track.artist}` : ''}"
          onclick={() => onplay(i)}
        >
          <Icon name="play" size={16} />
        </button>
      </span>
      <span class="main">
        <span class="t">{track.title}</span>
        {#if variant === "search"}
          <span class="sub">{track.artist ?? track.albumArtist ?? ""} • {track.album}</span>
        {:else if track.artist && track.artist !== track.albumArtist}
          <span class="sub">{track.artist}</span>
        {/if}
      </span>
      {#if key(track) !== common}
        <span class="badge" class:accent={isHiRes(track)}>{track.codec.toUpperCase()} {quality(track)}</span>
      {:else}
        <span></span>
      {/if}
      <span class="dur">{duration(track.durationMs)}</span>
      <RowMenu {track} showAlbum={variant === "search"} />
    </div>
  {/each}
</div>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px;
    border-radius: var(--r-lg);
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
    grid-template-columns: 40px minmax(0, 1fr) auto 52px 34px;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    padding: 4px 6px 4px 8px;
    border-radius: var(--r-sm);
    transition: background var(--dur-fast) var(--ease-standard);
  }
  .row.search {
    grid-template-columns: 44px minmax(0, 1fr) auto 52px 34px;
  }
  .row:hover,
  .row:focus-within {
    background: var(--soft-hover);
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
  .play {
    position: absolute;
    inset: 2px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--ink);
    color: #050506;
    opacity: 0;
    transform: scale(0.8);
    transition:
      opacity var(--dur-fast) var(--ease-standard),
      transform var(--dur-med) var(--ease-spring);
  }
  .row:hover .play,
  .play:focus-visible {
    opacity: 1;
    transform: none;
  }
  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
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
    font-variant-numeric: tabular-nums;
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
