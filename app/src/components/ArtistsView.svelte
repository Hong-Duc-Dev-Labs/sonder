<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { commands, type Artist } from "../lib/bindings";
  import { plural } from "../lib/format";
  import Icon from "./Icon.svelte";
  import RecordAvatar from "./RecordAvatar.svelte";

  let artists = $state<Artist[] | null>(null);
  let filter = $state("");
  let shown = $derived.by(() => {
    const f = filter.trim().toLocaleLowerCase();
    const list = artists ?? [];
    return f ? list.filter((a) => a.name.toLocaleLowerCase().includes(f)) : list;
  });
  /** Artists grouped under their first letter (anything else under "#"). */
  let groups = $derived.by(() => {
    const out: { letter: string; artists: Artist[] }[] = [];
    for (const a of shown) {
      const first = a.name.trim().charAt(0).toLocaleUpperCase();
      const letter = /\p{L}/u.test(first) ? first : "#";
      if (out.at(-1)?.letter !== letter) out.push({ letter, artists: [] });
      out.at(-1)!.artists.push(a);
    }
    return out;
  });

  $effect(() => {
    app.pageWash = null;
    void app.libraryVersion;
    app.run(commands.libraryArtists()).then((r) => {
      if (r) artists = r;
    });
  });

  function count(a: Artist): string {
    return a.albumCount ? plural(a.albumCount, "album") : plural(a.trackCount, "track");
  }
</script>

<div class="page scroll">
  <header class="head">
    <div>
      <h1 tabindex="-1" data-view-heading>Artists</h1>
      {#if artists}<span class="dim small">{plural(artists.length, "artist")}</span>{/if}
    </div>
    <div class="field filter">
      <Icon name="search" size={18} />
      <input placeholder="Filter artists" aria-label="Filter artists" bind:value={filter} />
    </div>
  </header>

  {#if artists && !artists.length}
    <p class="dim">No artists yet. Add a music folder in Settings.</p>
  {:else if artists && !shown.length}
    <p class="dim">No artist matches “{filter.trim()}”.</p>
  {/if}

  {#each groups as group (group.letter)}
    <h2 class="letter">{group.letter}</h2>
    <div class="list">
      {#each group.artists as artist (artist.id)}
        <button class="row" onclick={() => app.go({ kind: "artist", id: artist.id })}>
          <span class="avatar"><RecordAvatar art={artist.art} label={0.5} /></span>
          <span class="info">
            <span class="name">{artist.name}</span>
            <span class="dim small">{count(artist)}</span>
          </span>
        </button>
      {/each}
    </div>
  {/each}
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
  .filter {
    width: min(320px, 100%);
  }
  .filter input {
    height: 42px;
  }
  .small {
    font-size: var(--fs-small);
  }
  .letter {
    position: sticky;
    top: -28px;
    z-index: 1;
    margin: 18px 0 8px;
    padding: 4px 6px;
    font-size: 15px;
    color: var(--ink-2);
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
  .avatar {
    width: 52px;
    flex: none;
    transition: transform var(--dur-slow) var(--ease-spring);
  }
  .row:hover .avatar {
    transform: rotate(35deg);
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
