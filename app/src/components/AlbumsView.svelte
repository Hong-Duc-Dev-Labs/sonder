<script lang="ts">
  import { app } from "../lib/app.svelte";
  import type { SortOrder } from "../lib/bindings";
  import AlbumGrid from "./AlbumGrid.svelte";

  const sorts: { value: SortOrder; label: string }[] = [
    { value: "artist", label: "Artist" },
    { value: "title", label: "Title" },
    { value: "added", label: "Recently added" },
    { value: "year", label: "Year" },
  ];
  let sort = $derived<SortOrder>(app.settings.albumSort ?? "artist");
  let total = $state<number | null>(null);

  $effect(() => {
    app.pageWash = null;
  });
</script>

<div class="view">
  <header class="head">
    <div class="titles">
      <h1 tabindex="-1" data-view-heading>Albums</h1>
      {#if total}<span class="count">{total.toLocaleString()} {total === 1 ? "album" : "albums"}</span>{/if}
    </div>
    <div class="sorts" role="group" aria-label="Sort albums by">
      {#each sorts as s (s.value)}
        <button
          class="chip"
          aria-pressed={sort === s.value}
          onclick={() => app.saveSettings({ ...app.settings, albumSort: s.value })}
        >
          {s.label}
        </button>
      {/each}
    </div>
  </header>
  <div class="grid"><AlbumGrid {sort} bind:total /></div>
</div>

<style>
  .view {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 16px;
    padding: 32px var(--page-x) 22px calc(var(--inset-left, 0px) + var(--page-x));
  }
  .titles {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  h1 {
    font-size: 40px;
    font-weight: 800;
    letter-spacing: -0.035em;
  }
  .count {
    color: var(--ink-2);
    font-size: var(--fs-small);
  }
  .sorts {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .grid {
    flex: 1;
    min-height: 0;
  }
</style>
