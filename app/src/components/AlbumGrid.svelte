<script lang="ts">
  // Virtualised album grid: only the rows in view (plus a margin) are in the DOM, and albums
  // are fetched a page at a time as their rows come into view.
  import { untrack } from "svelte";
  import { app } from "../lib/app.svelte";
  import { commands, type Album, type SortOrder } from "../lib/bindings";
  import { t } from "../lib/i18n.svelte";
  import AlbumCard from "./AlbumCard.svelte";

  let { sort, total = $bindable(null) }: { sort: SortOrder; total?: number | null } = $props();

  const PAGE = 200;
  const MIN_CARD = 176;
  const GAP = 28;
  /** Title and artist lines under each cover, with their top margin. */
  const TEXT_HEIGHT = 56;
  const OVERSCAN_ROWS = 2;

  let viewport: HTMLDivElement;
  let width = $state(0);
  let height = $state(0);
  let scrollTop = $state(0);
  let albums = $state<(Album | undefined)[]>([]);
  let generation = 0;
  const loading = new Set<number>();

  // The spacer is measured inside the viewport's padding, so this is the cards' row width
  // whatever the page margins are.
  let inner = $derived(Math.max(0, width));
  let columns = $derived(Math.max(1, Math.floor((inner + GAP) / (MIN_CARD + GAP))));
  let cardWidth = $derived(inner > 0 ? (inner - GAP * (columns - 1)) / columns : MIN_CARD);
  let rowHeight = $derived(cardWidth + TEXT_HEIGHT + GAP);
  let rows = $derived(total == null ? 0 : Math.ceil(total / columns));
  let firstRow = $derived(Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN_ROWS));
  let lastRow = $derived(Math.min(rows, Math.ceil((scrollTop + height) / rowHeight) + OVERSCAN_ROWS));
  let visible = $derived.by(() => {
    const out: number[] = [];
    for (let i = firstRow * columns; i < Math.min(total ?? 0, lastRow * columns); i++) out.push(i);
    return out;
  });

  async function loadPage(page: number, gen: number) {
    if (loading.has(page)) return;
    loading.add(page);
    const result = await app.run(commands.libraryAlbums(sort, page * PAGE, PAGE));
    loading.delete(page);
    if (!result || gen !== generation) return;
    total = result.total;
    if (albums.length < result.total) albums.length = result.total;
    result.albums.forEach((a, i) => (albums[result.offset + i] = a));
  }

  // Start over when the sort order or the library changes.
  $effect(() => {
    void sort;
    void app.libraryVersion;
    untrack(() => {
      generation++;
      loading.clear();
      albums = [];
      total = null;
      loadPage(0, generation);
    });
  });

  // Fetch pages for rows in view that are not loaded yet.
  $effect(() => {
    const gen = generation;
    const pages = new Set(visible.filter((i) => !albums[i]).map((i) => Math.floor(i / PAGE)));
    pages.forEach((p) => loadPage(p, gen));
  });
</script>

<div
  class="viewport scroll"
  bind:this={viewport}
  bind:clientHeight={height}
  onscroll={() => (scrollTop = viewport.scrollTop)}
>
  {#if total === 0}
    <div class="empty">
      <h2>{t("common.libraryEmpty")}</h2>
      <p class="dim">{t("albums.emptyHint")}</p>
      <button class="btn primary" onclick={() => app.go({ kind: "settings" })}>{t("albums.openSettings")}</button>
    </div>
  {:else}
    <div class="spacer" bind:clientWidth={width} style:height="{rows * rowHeight}px">
      {#each visible as i (i)}
        {@const album = albums[i]}
        <div
          class="cell"
          style:width="{cardWidth}px"
          style:transform="translate({(i % columns) * (cardWidth + GAP)}px, {Math.floor(i / columns) *
            rowHeight}px)"
        >
          {#if album}
            <AlbumCard {album} />
          {:else}
            <div class="skeleton" aria-hidden="true">
              <div class="sk-cover" style:height="{cardWidth}px"></div>
              <div class="sk-line"></div>
              <div class="sk-line short"></div>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .viewport {
    height: 100%;
    padding: 4px var(--page-x) var(--dock-reserve) var(--page-x);
    contain: strict;
  }
  .spacer {
    position: relative;
  }
  .cell {
    position: absolute;
    top: 0;
    left: 0;
  }
  .sk-cover {
    border-radius: var(--r-md);
    background: var(--soft);
  }
  .sk-line {
    height: 12px;
    margin-top: 14px;
    width: 80%;
    border-radius: var(--r-pill);
    background: var(--soft);
  }
  .sk-line.short {
    margin-top: 8px;
    width: 50%;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    height: 70%;
    text-align: center;
  }
  .empty p {
    margin: 0 0 8px;
  }
</style>
