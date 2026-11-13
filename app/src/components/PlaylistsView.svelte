<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { durationShort, plural } from "../lib/format";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";

  let loaded = $state(false);

  // A rescan can change lengths and covers; a playlist edit changes the list itself.
  $effect(() => {
    app.pageWash = null;
    void app.libraryVersion;
    void app.playlistsVersion;
    app.refreshPlaylists().then(() => (loaded = true));
  });

  async function create() {
    const created = await app.createPlaylist();
    if (created) app.go({ kind: "playlist", id: created.id });
  }
</script>

<div class="page scroll">
  <header class="head">
    <div>
      <h1 tabindex="-1" data-view-heading>Playlists</h1>
      {#if loaded}<span class="dim small">{plural(app.playlists.length, "playlist")}</span>{/if}
    </div>
    <button class="btn" onclick={create}><Icon name="add" size={18} /> New playlist</button>
  </header>

  {#if loaded && !app.playlists.length}
    <p class="dim">No playlists yet. Create one here, or use “Add to playlist” in any track’s ⋯ menu.</p>
  {/if}

  <div class="list">
    {#each app.playlists as playlist (playlist.id)}
      <button class="row" onclick={() => app.go({ kind: "playlist", id: playlist.id })}>
        <span class="art"><Cover hash={playlist.art} alt="" /></span>
        <span class="info">
          <span class="name">{playlist.name}</span>
          <span class="dim small">
            {[plural(playlist.itemCount, "track"), playlist.durationMs ? durationShort(playlist.durationMs) : null]
              .filter(Boolean)
              .join(" • ")}
          </span>
        </span>
      </button>
    {/each}
  </div>
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
  .list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 6px 14px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 8px 10px;
    border-radius: var(--r-md);
    text-align: left;
    min-width: 0;
    transition: background var(--dur-fast) var(--ease-standard);
  }
  .row:hover {
    background: var(--soft-hover);
  }
  .art {
    width: 56px;
    flex: none;
  }
  .info {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    font-weight: 650;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
