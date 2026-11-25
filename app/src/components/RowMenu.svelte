<script lang="ts">
  // The ⋯ button on a track row and its glass menu. "Add to playlist" swaps the menu for the
  // list of playlists in place, with a way back. `openAt` opens the same menu at a point, for
  // a right-click on the row.
  import { tick } from "svelte";
  import { app } from "../lib/app.svelte";
  import { commands, type TrackItem } from "../lib/bindings";
  import { t } from "../lib/i18n.svelte";
  import Icon from "./Icon.svelte";
  import Popover from "./Popover.svelte";

  let {
    track,
    showAlbum = true,
    onremove,
  }: {
    track: TrackItem;
    showAlbum?: boolean;
    /** Shown as "Remove from playlist" when set. */
    onremove?: () => void;
  } = $props();
  let open = $state(false);
  /** Where a right-click opened the menu; null when it hangs off the ⋯ button. */
  let at = $state<{ x: number; y: number } | null>(null);
  let picking = $state(false);
  let fav = $derived(app.isFavorite(track.path));
  let button: HTMLButtonElement | undefined = $state();
  let items: HTMLElement | undefined = $state();

  $effect(() => {
    if (!open) picking = false;
  });

  export function openAt(x: number, y: number) {
    at = { x, y };
    picking = false;
    open = true;
  }

  function act(work: () => void) {
    open = false;
    work();
  }

  /** Switches between the actions and the playlists, focusing the first item shown. */
  async function pick(on: boolean) {
    picking = on;
    await tick();
    items?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }

  async function addToNew() {
    open = false;
    const created = await app.createPlaylist();
    if (created) await app.addToPlaylist(created, [track.id]);
  }
</script>

<button
  bind:this={button}
  class="more icon-btn"
  aria-label={t("common.moreFor", { name: track.title })}
  aria-haspopup="menu"
  aria-expanded={open}
  onclick={() => {
    at = null;
    open = !open;
  }}
>
  <Icon name="more" size={18} />
</button>

<Popover anchor={button} {at} bind:open menu label={picking ? t("common.addToPlaylist") : t("menu.trackActions")}>
  <div class="items" bind:this={items}>
    {#if picking}
      <button role="menuitem" onclick={() => pick(false)}>
        <Icon name="back" size={18} /> {t("common.addToPlaylist")}
      </button>
      <span class="sep" role="separator"></span>
      <button role="menuitem" onclick={addToNew}>
        <Icon name="add" size={18} /> {t("menu.newPlaylist")}
      </button>
      <div class="list">
        {#each app.playlists as playlist (playlist.id)}
          <button role="menuitem" onclick={() => act(() => app.addToPlaylist(playlist, [track.id]))}>
            <Icon name="playlist" size={18} /> <span class="name">{playlist.name}</span>
          </button>
        {/each}
      </div>
    {:else}
      <button role="menuitem" onclick={() => act(() => app.run(commands.playerPlayNextTracks([track.id])))}>
        <Icon name="play-next" size={18} /> {t("common.playNext")}
      </button>
      <button role="menuitem" onclick={() => act(() => app.run(commands.playerEnqueueTracks([track.id])))}>
        <Icon name="queue-add" size={18} /> {t("common.addToQueue")}
      </button>
      <button role="menuitem" aria-haspopup="menu" onclick={() => pick(true)}>
        <Icon name="playlist" size={18} /> <span class="name">{t("common.addToPlaylist")}</span>
        <Icon name="chevron" size={16} />
      </button>
      <button role="menuitem" onclick={() => act(() => app.editTags(track))}>
        <Icon name="edit" size={18} /> {t("menu.editTags")}
      </button>
      <button role="menuitem" onclick={() => act(() => app.checkQuality(track))}>
        <Icon name="spectrum" size={18} /> {t("menu.checkQuality")}
      </button>
      <button role="menuitem" onclick={() => act(() => void app.toggleFavorite(track.path))}>
        <Icon name={fav ? "heart" : "heart-outline"} size={18} />
        {fav ? t("favorites.remove") : t("favorites.add")}
      </button>
      <button role="menuitem" onclick={() => act(() => void app.revealInFolder(track.path))}>
        <Icon name="folder-open" size={18} /> {t("menu.revealInFolder")}
      </button>
      {#if onremove}
        <button role="menuitem" onclick={() => act(onremove)}>
          <Icon name="trash" size={18} /> {t("menu.removeFromPlaylist")}
        </button>
      {/if}
      {#if showAlbum && track.albumId > 0}
        <button role="menuitem" onclick={() => act(() => app.go({ kind: "album", id: track.albumId }))}>
          <Icon name="albums" size={18} /> {t("common.goToAlbum")}
        </button>
      {/if}
    {/if}
  </div>
</Popover>

<style>
  .more {
    width: 34px;
    height: 34px;
  }
  .items {
    display: contents;
  }
  .list {
    max-height: 280px;
    overflow-y: auto;
    scrollbar-width: thin;
  }
  .name {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sep {
    display: block;
    height: 1px;
    margin: 4px 8px;
    background: var(--hairline);
  }
</style>
