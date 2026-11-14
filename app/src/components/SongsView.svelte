<script lang="ts">
  // Every track in the library, by title. The list can run to tens of thousands of rows, so
  // it is drawn a batch at a time as the end of what is shown scrolls into view.
  import { app } from "../lib/app.svelte";
  import { commands, type TrackItem } from "../lib/bindings";
  import { plural, shuffled } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import Icon from "./Icon.svelte";
  import TrackList from "./TrackList.svelte";

  const BATCH = 300;

  let tracks = $state<TrackItem[] | null>(null);
  let filter = $state("");
  let limit = $state(BATCH);
  let sentinel: HTMLDivElement | undefined = $state();
  let scroller: HTMLDivElement | undefined = $state();

  $effect(() => {
    app.pageWash = null;
    void app.libraryVersion;
    app.run(commands.libraryTracks()).then((r) => {
      if (r) tracks = r;
    });
  });

  /** Lower case without diacritics, so "viet" finds "Việt". */
  function fold(text: string): string {
    return text.normalize("NFD").replace(/\p{M}/gu, "").replace(/đ/gi, "d").toLocaleLowerCase();
  }

  /** Each track's searchable text, folded once per load rather than on every keystroke. */
  let haystacks = $derived(
    (tracks ?? []).map((tr) => fold([tr.title, tr.artist, tr.albumArtist, tr.album].filter(Boolean).join(" "))),
  );
  /** Tracks holding every word of the filter. */
  let matching = $derived.by(() => {
    const terms = fold(filter).split(/\s+/).filter(Boolean);
    const all = tracks ?? [];
    if (!terms.length) return all;
    return all.filter((_, i) => terms.every((term) => haystacks[i].includes(term)));
  });
  let shown = $derived(matching.slice(0, limit));
  let ids = $derived(matching.map((tr) => tr.id));

  // A new filter starts again from the top batch.
  $effect(() => {
    void filter;
    limit = BATCH;
  });

  // Draw the next batch when the end of the list comes within 800 px. The page scrolls inside
  // its own container, which clips the sentinel before a viewport margin would apply, so the
  // container is the root.
  $effect(() => {
    if (!sentinel || !scroller) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) limit += BATCH;
      },
      { root: scroller, rootMargin: "800px 0px" },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  });
</script>

<div class="page scroll" bind:this={scroller}>
  <header class="head">
    <div>
      <h1 tabindex="-1" data-view-heading>{t("common.songs")}</h1>
      {#if tracks}<span class="dim small">{plural(tracks.length, "track")}</span>{/if}
    </div>
    <div class="tools">
      <div class="field filter">
        <Icon name="search" size={18} />
        <input placeholder={t("songs.filter")} aria-label={t("songs.filter")} bind:value={filter} />
      </div>
      <button class="btn primary" disabled={!ids.length} onclick={() => app.run(commands.playerPlayTracks(ids, 0))}>
        <Icon name="play" size={18} /> {t("common.playAll")}
      </button>
      <button class="btn" disabled={!ids.length} onclick={() => app.run(commands.playerPlayTracks(shuffled(ids), 0))}>
        <Icon name="shuffle" size={18} /> {t("common.shuffle")}
      </button>
      <button
        class="icon-btn filled"
        aria-label={t("quality.checkAll")}
        title={t("quality.checkAll")}
        disabled={!tracks?.length}
        onclick={() => tracks && app.checkQualityBatch(tracks.map((tr) => tr.id))}
      >
        <Icon name="spectrum" size={20} />
      </button>
    </div>
  </header>

  {#if tracks && !tracks.length}
    <p class="dim">{t("songs.none")}</p>
  {:else if tracks && !matching.length}
    <p class="dim">{t("songs.noMatch", { filter: filter.trim() })}</p>
  {:else if shown.length}
    <TrackList tracks={shown} variant="search" onplay={(i) => app.run(commands.playerPlayTracks(ids, i))} />
    {#if shown.length < matching.length}
      <div class="sentinel" bind:this={sentinel} aria-hidden="true"></div>
    {/if}
  {/if}
</div>

<style>
  .head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 16px;
    margin-bottom: 18px;
  }
  h1 {
    font-size: 40px;
    font-weight: 800;
    letter-spacing: -0.035em;
  }
  .small {
    font-size: var(--fs-small);
  }
  .tools {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
  }
  .filter {
    width: min(320px, 100%);
  }
  .filter input {
    height: 42px;
  }
  .sentinel {
    height: 1px;
  }
</style>
