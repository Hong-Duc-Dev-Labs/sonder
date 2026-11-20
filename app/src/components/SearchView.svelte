<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { commands, type TrackItem } from "../lib/bindings";
  import { t } from "../lib/i18n.svelte";
  import { resolveDesktopOs, shortcutKeys } from "../lib/platform";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";
  import TrackList from "./TrackList.svelte";

  const [modKey, actionKey] = shortcutKeys(resolveDesktopOs(), "search");

  let results = $state<TrackItem[]>([]);
  /** The query the shown results belong to; differs from the field while a search is pending. */
  let answered = $state("");
  let input: HTMLInputElement | undefined = $state();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let latest = 0;

  $effect(() => {
    input?.focus();
    app.pageWash = null;
  });

  // Debounced: search as the user types, keeping only the newest answer.
  $effect(() => {
    void app.libraryVersion;
    const q = app.searchQuery.trim();
    clearTimeout(timer);
    if (!q) {
      results = [];
      answered = "";
      return;
    }
    const n = ++latest;
    timer = setTimeout(async () => {
      const r = await app.run(commands.librarySearch(q));
      if (n !== latest) return;
      results = r ?? [];
      answered = q;
    }, 150);
  });

  let pending = $derived(app.searchQuery.trim() !== answered);
  let ids = $derived(results.map((t) => t.id));
  /** Albums among the results, in the order their first track ranks. */
  let albums = $derived.by(() => {
    const seen = new Map<number, TrackItem>();
    for (const t of results) if (!seen.has(t.albumId)) seen.set(t.albumId, t);
    return [...seen.values()].slice(0, 8);
  });
</script>

<div class="page scroll">
  <header class="page-head">
    <h1 tabindex="-1" data-view-heading>{t("common.search")}</h1>
  </header>

  <div class="field search">
    <Icon name="search" size={20} />
    <input
      bind:this={input}
      type="search"
      placeholder={t("search.placeholder")}
      aria-label={t("search.label")}
      bind:value={app.searchQuery}
    />
  </div>

  {#if !app.searchQuery.trim()}
    <div class="hint">
      <Icon name="search" size={36} />
      <p>{t("search.hint")}</p>
      <p class="dim small">{t("search.shortcutBefore")} <kbd>{modKey}</kbd> <kbd>{actionKey}</kbd> {t("search.shortcutAfter")}</p>
    </div>
  {:else if results.length}
    <div class:stale={pending}>
      {#if albums.length > 1}
        <h2>{t("common.albums")}</h2>
        <div class="albums">
          {#each albums as t (t.albumId)}
            <button class="album" onclick={() => app.go({ kind: "album", id: t.albumId })}>
              <Cover hash={t.art} alt="" />
              <span class="name">{t.album}</span>
              <span class="dim small">{t.albumArtist ?? t.artist ?? ""}</span>
            </button>
          {/each}
        </div>
      {/if}
      <div class="tracks-head">
        <h2>{t("search.tracks")}</h2>
        <div class="actions">
          <button class="btn small primary" onclick={() => app.run(commands.playerPlayTracks(ids, 0))}>
            <Icon name="play" size={14} /> {t("common.playAll")}
          </button>
          <button class="btn small" onclick={() => app.run(commands.playerEnqueueTracks(ids))}>{t("search.addAll")}</button>
        </div>
      </div>
      <TrackList tracks={results} variant="search" onplay={(i) => app.run(commands.playerPlayTracks(ids, i))} />
    </div>
  {:else if !pending}
    <p class="dim none">{t("search.noMatch", { query: answered })}</p>
  {/if}
</div>

<style>
  .search {
    max-width: 640px;
    margin-bottom: 28px;
  }
  .search input {
    height: 50px;
  }
  .hint {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 72px 16px;
    color: var(--ink-2);
    text-align: center;
  }
  .hint p {
    margin: 6px 0 0;
  }
  .small {
    font-size: var(--fs-small);
  }
  kbd {
    font-family: var(--font);
    font-size: 11px;
    font-weight: 650;
    padding: 2px 7px;
    border-radius: 6px;
    box-shadow: inset 0 0 0 1px var(--hairline);
    color: var(--ink);
  }
  h2 {
    font-size: 20px;
    margin: 0 0 14px;
  }
  .stale {
    opacity: 0.6;
    transition: opacity var(--dur-fast) var(--ease-standard);
  }
  .albums {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 148px;
    gap: 18px;
    overflow-x: auto;
    padding-bottom: 8px;
    margin-bottom: 26px;
    scrollbar-width: thin;
  }
  .album {
    display: flex;
    flex-direction: column;
    gap: 2px;
    text-align: left;
    min-width: 0;
  }
  .album :global(.cover) {
    border-radius: var(--r-md);
    margin-bottom: 8px;
    transition: transform var(--dur-med) var(--ease-spring);
  }
  .album:hover :global(.cover) {
    transform: translateY(-3px);
  }
  .name {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .album .small {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tracks-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 4px;
  }
  .tracks-head h2 {
    margin: 0;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  .none {
    padding: 24px 4px;
  }
</style>
