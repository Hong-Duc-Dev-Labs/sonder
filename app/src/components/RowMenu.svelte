<script lang="ts">
  // The ⋯ button on a track row and its glass menu.
  import { app } from "../lib/app.svelte";
  import { commands, type TrackItem } from "../lib/bindings";
  import Icon from "./Icon.svelte";
  import Popover from "./Popover.svelte";

  let { track, showAlbum = true }: { track: TrackItem; showAlbum?: boolean } = $props();
  let open = $state(false);
  let button: HTMLButtonElement | undefined = $state();

  function act(work: () => void) {
    open = false;
    work();
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

<Popover anchor={button} bind:open menu label="Track actions">
  <button role="menuitem" onclick={() => act(() => app.run(commands.playerPlayNextTracks([track.id])))}>
    <Icon name="play-next" size={18} /> Play next
  </button>
  <button role="menuitem" onclick={() => act(() => app.run(commands.playerEnqueueTracks([track.id])))}>
    <Icon name="queue-add" size={18} /> Add to queue
  </button>
  {#if showAlbum}
    <button
      role="menuitem"
      onclick={() =>
        act(() => {
          app.go({ kind: "album", id: track.albumId });
        })}
    >
      <Icon name="albums" size={18} /> Go to album
    </button>
  {/if}
</Popover>

<style>
  .more {
    width: 34px;
    height: 34px;
  }
</style>
