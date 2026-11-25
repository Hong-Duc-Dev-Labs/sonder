<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { commands, type TrackItem } from "../lib/bindings";
  import { plural, shuffled } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import Icon from "./Icon.svelte";
  import TrackList from "./TrackList.svelte";

  let tracks = $state<TrackItem[] | null>(null);

  $effect(() => {
    app.pageWash = null;
    void app.libraryVersion;
    void app.favoritesVersion;
    app.run(commands.libraryFavorites()).then((r) => {
      if (r) tracks = r;
    });
  });

  let ids = $derived((tracks ?? []).map((tr) => tr.id));
</script>

<div class="page scroll">
  <header class="head">
    <div>
      <h1 tabindex="-1" data-view-heading>{t("common.favorites")}</h1>
      {#if tracks}
        <span class="dim small">{plural(tracks.length, "track")}</span>
      {/if}
    </div>
    <div class="tools">
      <button class="btn primary" disabled={!ids.length} onclick={() => app.run(commands.playerPlayTracks(ids, 0))}>
        <Icon name="play" size={18} /> {t("common.playAll")}
      </button>
      <button class="btn" disabled={!ids.length} onclick={() => app.run(commands.playerPlayTracks(shuffled(ids), 0))}>
        <Icon name="shuffle" size={18} /> {t("common.shuffle")}
      </button>
      <button
        class="icon-btn filled"
        aria-label={t("common.addToQueue")}
        disabled={!ids.length}
        onclick={() => app.run(commands.playerEnqueueTracks(ids))}
      >
        <Icon name="queue-add" size={20} />
      </button>
    </div>
  </header>

  {#if tracks && !tracks.length}
    <div class="empty soft">
      <span class="empty-icon"><Icon name="heart-outline" size={40} /></span>
      <h2>{t("favorites.empty")}</h2>
      <p class="dim">{t("favorites.emptyHint")}</p>
    </div>
  {:else if tracks}
    <TrackList {tracks} variant="search" onplay={(i) => app.run(commands.playerPlayTracks(ids, i))} />
  {/if}
</div>

<style>
  .head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 16px;
    margin-bottom: 20px;
  }
  h1 {
    font-size: 40px;
    font-weight: 800;
    letter-spacing: -0.035em;
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }
  .small {
    font-size: var(--fs-small);
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 56px 24px;
    border-radius: var(--r-lg);
    text-align: center;
  }
  .empty-icon {
    color: var(--ink-2);
  }
  .empty h2 {
    font-size: var(--fs-title);
    margin: 0;
  }
  .empty p {
    margin: 0;
    max-width: 400px;
  }
</style>
