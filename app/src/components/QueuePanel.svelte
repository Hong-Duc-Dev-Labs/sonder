<script lang="ts">
  // The queue in the inspector column: beside the page on wide windows, a card over it
  // otherwise (see App.svelte). Rows are keyed by
  // their queue entry, so the highlight never jumps when entries move. Click or Enter plays a
  // row, the × removes it, Escape closes the card and returns focus to the queue button.
  import { onMount, tick } from "svelte";
  import { app } from "../lib/app.svelte";
  import { commands, type QueueItem } from "../lib/bindings";
  import { duration, plural } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";
  import MiniDisc from "./MiniDisc.svelte";

  let panel: HTMLElement;
  let current = $derived(app.player.index);
  let entries = $derived(app.queue.map((item, index) => ({ item, index })));
  let nowEntry = $derived(current != null ? entries[current] : undefined);
  let upNext = $derived(entries.filter((e) => current == null || e.index > current));
  let played = $derived(current == null ? [] : entries.filter((e) => e.index < current));
  let remainingMs = $derived(upNext.reduce((sum, e) => sum + e.item.durationMs, 0));
  let canShuffle = $derived((current == null ? app.queue.length : upNext.length) >= 2);

  let saving = $state(false);
  let playlistName = $state("");
  let saveInput = $state<HTMLInputElement | undefined>(undefined);
  let dragIndex = $state<number | null>(null);
  let dropIndex = $state<number | null>(null);

  function close() {
    app.closeInspector();
  }

  async function toggleSave() {
    saving = !saving;
    playlistName = "";
    if (saving) {
      await tick();
      saveInput?.focus();
    }
  }

  async function submitSave(e: SubmitEvent) {
    e.preventDefault();
    const created = await app.saveQueueAsPlaylist(playlistName);
    if (created) {
      saving = false;
      playlistName = "";
    }
  }

  function onDragStart(e: DragEvent, index: number) {
    dragIndex = index;
    dropIndex = index;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", String(index));
    }
  }

  function onDragOver(e: DragEvent, index: number) {
    if (dragIndex == null) return;
    e.preventDefault();
    dropIndex = index;
  }

  function onDrop(e: DragEvent, to: number) {
    e.preventDefault();
    const from = dragIndex;
    dragIndex = null;
    dropIndex = null;
    if (from != null && from !== to) {
      void app.run(commands.queueMove(from, to));
    }
  }

  function onDragEnd() {
    dragIndex = null;
    dropIndex = null;
  }

  async function gripKey(e: KeyboardEvent, index: number, entryId: number) {
    const step = e.key === "ArrowUp" ? -1 : e.key === "ArrowDown" ? 1 : 0;
    const to = index + step;
    if (!step || to < 0 || to >= app.queue.length) return;
    e.preventDefault();
    await app.run(commands.queueMove(index, to));
    await tick();
    panel.querySelector<HTMLElement>(`[data-queue-grip="${entryId}"]`)?.focus();
  }

  onMount(() => {
    panel.querySelector<HTMLElement>("[aria-current='true']")?.scrollIntoView({ block: "center" });
  });
</script>

{#snippet row(item: QueueItem, index: number, kind: "now" | "next" | "played")}
  <div
    class="row {kind}"
    class:dragging={dragIndex === index}
    class:drop-before={dropIndex === index && dragIndex != null && dragIndex > index}
    class:drop-after={dropIndex === index && dragIndex != null && dragIndex < index}
    role="listitem"
    aria-current={kind === "now" ? "true" : undefined}
    draggable="true"
    ondragstart={(e) => onDragStart(e, index)}
    ondragover={(e) => onDragOver(e, index)}
    ondrop={(e) => onDrop(e, index)}
    ondragend={onDragEnd}
  >
    <button
      class="grip"
      data-queue-grip={item.entryId}
      aria-label={t("queue.move", { title: item.title, position: index + 1, total: app.queue.length })}
      aria-keyshortcuts="ArrowUp ArrowDown"
      title={t("queue.reorderHint")}
      onkeydown={(e) => gripKey(e, index, item.entryId)}
    >
      <Icon name="grip" size={16} />
    </button>
    <button
      class="play"
      aria-label={item.artist
        ? t("common.playItemBy", { title: item.title, artist: item.artist })
        : t("common.playItem", { title: item.title })}
      onclick={() => app.run(commands.playerPlayIndex(index))}
    >
      <span class="thumb">
        {#if kind === "now"}
          <MiniDisc art={item.art} paused={app.player.state === "paused"} />
        {:else}
          <Cover hash={item.art} alt="" />
        {/if}
      </span>
      <span class="main">
        <span class="t">{item.title}</span>
        <span class="a">{item.artist ?? item.album}</span>
      </span>
      <span class="d">{duration(item.durationMs)}</span>
    </button>
    <button class="remove icon-btn" aria-label={t("queue.removeItem", { title: item.title })} onclick={() => app.run(commands.queueRemove(item.entryId))}>
      <Icon name="close" size={16} />
    </button>
  </div>
{/snippet}

<!-- svelte:window in App.svelte closes it on Escape, so focus outside the panel is covered. -->
<aside class="panel glass" bind:this={panel} aria-label={t("common.queue")}>
  <div class="head">
    <div>
      <h2>{t("common.queue")}</h2>
      <span class="sub">
        {plural(app.queue.length, "track")}{remainingMs ? ` · ${t("queue.left", { time: duration(remainingMs) })}` : ""}
      </span>
    </div>
    <div class="tools">
      {#if app.queue.length}
        <button
          class="icon-btn"
          disabled={!canShuffle}
          aria-label={t("queue.shuffleUpcoming")}
          title={t("queue.shuffleUpcoming")}
          onclick={() => app.run(commands.queueShuffle())}
        >
          <Icon name="shuffle" size={16} />
        </button>
        <button
          class="icon-btn"
          aria-label={t("queue.savePlaylist")}
          title={t("queue.savePlaylist")}
          aria-expanded={saving}
          onclick={toggleSave}
        >
          <Icon name="save" size={16} />
        </button>
        <button class="btn small" onclick={() => app.run(commands.queueClear())}>{t("queue.clear")}</button>
      {/if}
      <button class="icon-btn" aria-label={t("queue.close")} onclick={close}><Icon name="close" size={18} /></button>
    </div>
  </div>
  {#if saving && app.queue.length}
    <form class="save-bar" onsubmit={submitSave}>
      <input
        bind:this={saveInput}
        type="text"
        bind:value={playlistName}
        placeholder={t("queue.playlistNamePlaceholder")}
        aria-label={t("queue.savePlaylist")}
      />
      <button type="submit" class="btn small primary" disabled={!playlistName.trim()}>{t("common.save")}</button>
      <button type="button" class="icon-btn" aria-label={t("common.cancel")} onclick={() => (saving = false)}>
        <Icon name="close" size={16} />
      </button>
    </form>
  {/if}

  <div class="items scroll" role="list">
    {#if nowEntry}
      <h3 class="kicker">{t("common.nowPlayingKicker")}</h3>
      {@render row(nowEntry.item, nowEntry.index, "now")}
    {/if}
    {#if upNext.length}
      <h3 class="kicker">{t("common.upNext")}</h3>
      {#each upNext as e (e.item.entryId)}
        {@render row(e.item, e.index, "next")}
      {/each}
    {/if}
    {#if played.length}
      <h3 class="kicker">{t("queue.played")}</h3>
      {#each played as e (e.item.entryId)}
        {@render row(e.item, e.index, "played")}
      {/each}
    {/if}
    {#if !app.queue.length}
      <div class="empty">
        <Icon name="disc" size={40} />
        <p>{t("queue.empty")}</p>
        <button class="btn small" onclick={() => { app.queueOpen = false; app.go({ kind: "albums" }); }}>{t("common.browseAlbums")}</button>
      </div>
    {/if}
  </div>
</aside>

<style>
  .panel {
    width: 100%;
    height: 100%;
    border-radius: var(--r-lg);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 20px 16px 12px 20px;
  }
  h2 {
    font-size: 22px;
    font-weight: 800;
  }
  .sub {
    font-size: var(--fs-small);
    color: var(--ink-2);
  }
  .tools {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .save-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 16px 10px 20px;
  }
  .save-bar input {
    flex: 1;
    min-width: 0;
    height: 32px;
    padding: 0 10px;
    border-radius: var(--r-sm);
    background: rgb(0 0 0 / 0.25);
    color: var(--ink);
    box-shadow: inset 0 0 0 1px var(--hairline);
    font: inherit;
    font-size: var(--fs-small);
  }
  .items {
    flex: 1;
    padding: 0 10px 14px;
  }
  .kicker {
    display: block;
    margin: 14px 10px 6px;
    color: var(--ink-2);
  }
  .row {
    position: relative;
    display: flex;
    align-items: center;
    border-radius: var(--r-sm);
    transition: background var(--dur-fast) var(--ease-standard);
  }
  .row.dragging {
    opacity: 0.45;
  }
  .row.drop-before {
    box-shadow: inset 0 2px 0 0 var(--ink);
  }
  .row.drop-after {
    box-shadow: inset 0 -2px 0 0 var(--ink);
  }
  .grip {
    width: 24px;
    height: 32px;
    flex: none;
    margin-left: 4px;
    display: grid;
    place-items: center;
    border-radius: var(--r-xs);
    color: var(--ink-3);
    cursor: grab;
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-standard);
  }
  .row:hover .grip,
  .row:focus-within .grip {
    opacity: 1;
  }
  .grip:hover,
  .grip:focus-visible {
    color: var(--ink);
  }
  .row:hover,
  .row:focus-within {
    background: var(--soft-hover);
  }
  .row.now {
    background: var(--soft);
  }
  .row.played {
    opacity: 0.55;
  }
  .play {
    flex: 1;
    min-width: 0;
    display: grid;
    grid-template-columns: 44px 1fr auto;
    gap: 12px;
    align-items: center;
    padding: 8px 10px;
    text-align: left;
    border-radius: inherit;
  }
  .row.now .play {
    grid-template-columns: 52px 1fr auto;
  }
  .thumb :global(.cover) {
    border-radius: var(--r-xs);
    box-shadow: none;
  }
  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .t,
  .a {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .t {
    font-weight: 600;
  }
  .a {
    font-size: var(--fs-small);
    color: var(--ink-2);
  }
  .d {
    font-size: var(--fs-small);
    color: var(--ink-2);
    font-family: var(--font-figures);
    font-variant-numeric: tabular-nums;
  }
  .remove {
    width: 32px;
    height: 32px;
    margin-right: 6px;
    opacity: 0;
  }
  .row:hover .remove,
  .row:focus-within .remove {
    opacity: 1;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 48px 16px;
    color: var(--ink-2);
    text-align: center;
  }
  .empty p {
    margin: 0;
  }
</style>
