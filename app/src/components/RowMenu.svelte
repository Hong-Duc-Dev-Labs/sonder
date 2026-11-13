<script lang="ts">
  // The ⋯ button on a track row and its glass menu. "Add to playlist" swaps the menu for the
  // list of playlists in place, with a way back.
  import { tick } from "svelte";
  import { app } from "../lib/app.svelte";
  import { commands, type TrackItem } from "../lib/bindings";
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
  let picking = $state(false);
  let button: HTMLButtonElement | undefined = $state();
  let items: HTMLElement | undefined = $state();

  $effect(() => {
    if (!open) picking = false;
  });

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
  aria-label="More for {track.title}"
  aria-haspopup="menu"
  aria-expanded={open}
  onclick={() => (open = !open)}
>
  <Icon name="more" size={18} />
</button>

<Popover anchor={button} bind:open menu label={picking ? "Add to playlist" : "Track actions"}>
  <div class="items" bind:this={items}>
    {#if picking}
      <button role="menuitem" onclick={() => pick(false)}>
        <Icon name="back" size={18} /> Add to playlist
      </button>
      <span class="sep" role="separator"></span>
      <button role="menuitem" onclick={addToNew}>
        <Icon name="add" size={18} /> New playlist…
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
        <Icon name="play-next" size={18} /> Play next
      </button>
      <button role="menuitem" onclick={() => act(() => app.run(commands.playerEnqueueTracks([track.id])))}>
        <Icon name="queue-add" size={18} /> Add to queue
      </button>
      <button role="menuitem" aria-haspopup="menu" onclick={() => pick(true)}>
        <Icon name="playlist" size={18} /> <span class="name">Add to playlist</span>
        <Icon name="chevron" size={16} />
      </button>
      {#if onremove}
        <button role="menuitem" onclick={() => act(onremove)}>
          <Icon name="trash" size={18} /> Remove from playlist
        </button>
      {/if}
      {#if showAlbum}
        <button role="menuitem" onclick={() => act(() => app.go({ kind: "album", id: track.albumId }))}>
          <Icon name="albums" size={18} /> Go to album
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
