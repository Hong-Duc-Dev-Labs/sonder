<script lang="ts">
  import { app } from "../lib/app.svelte";
  import type { SortOrder } from "../lib/bindings";
  import { plural } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import AlbumGrid from "./AlbumGrid.svelte";

  let sorts = $derived<{ value: SortOrder; label: string }[]>([
    { value: "artist", label: t("sort.artist") },
    { value: "title", label: t("sort.title") },
    { value: "added", label: t("sort.added") },
    { value: "year", label: t("sort.year") },
  ]);
  let sort = $derived<SortOrder>(app.settings.albumSort ?? "artist");
  let total = $state<number | null>(null);

  $effect(() => {
    app.pageWash = null;
  });
</script>

<div class="view">
  <header class="head">
    <div class="titles">
      <h1 tabindex="-1" data-view-heading>{t("common.albums")}</h1>
      {#if total}<span class="count">{plural(total, "album")}</span>{/if}
    </div>
    <div class="sorts" role="group" aria-label={t("albums.sortBy")}>
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
    padding: 32px var(--page-x) 22px var(--page-x);
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
