<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { commands, type PlaylistEntry, type PlaylistInfo } from "../lib/bindings";
  import { durationShort, plural, shuffled } from "../lib/format";
  import { fadeQuick } from "../lib/transitions";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";
  import Popover from "./Popover.svelte";
  import TrackList from "./TrackList.svelte";

  let { id }: { id: number } = $props();
  let info = $state<PlaylistInfo | null | undefined>(undefined);
  let menuOpen = $state(false);
  let menuButton: HTMLButtonElement | undefined = $state();

  let tracks = $derived(info?.entries.map((e) => e.track) ?? []);
  let ids = $derived(tracks.map((t) => t.id));

  /** Edits run one after another; the page reloads once the last has finished. */
  let chain: Promise<unknown> = Promise.resolve();
  let inflight = 0;
  function edit(work: () => Promise<unknown>) {
    inflight++;
    chain = chain.then(work).finally(() => {
      if (--inflight === 0) app.playlistsChanged();
    });
  }

  $effect(() => {
    void app.libraryVersion;
    void app.playlistsVersion;
    const want = id;
    app.run(commands.libraryPlaylist(want)).then((r) => {
      // An edit still running would be undone on screen by this older state.
      if (want === id && r !== undefined && inflight === 0) info = r;
    });
  });

  $effect(() => {
    app.pageWash = tracks[0]?.artColors[0] ?? null;
    return () => {
      app.pageWash = null;
    };
  });

  /** Positions after moving the entry at position `from` to `to`, as the library does. */
  function shifted(p: number, from: number, to: number): number {
    if (p === from) return to;
    if (from < p && p <= to) return p - 1;
    if (to <= p && p < from) return p + 1;
    return p;
  }

  function move(from: number, to: number) {
    if (!info) return;
    const a = info.entries[from].position;
    const b = info.entries[to].position;
    const entries: PlaylistEntry[] = info.entries.map((e) => ({ ...e, position: shifted(e.position, a, b) }));
    entries.splice(to, 0, ...entries.splice(from, 1));
    info = { ...info, entries };
    edit(() => app.run(commands.playlistMoveEntry(id, a, b)));
  }

  function remove(index: number) {
    if (!info) return;
    const gone = info.entries[index];
    const entries = info.entries
      .filter((_, i) => i !== index)
      .map((e) => ({ ...e, position: e.position > gone.position ? e.position - 1 : e.position }));
    const playlist = {
      ...info.playlist,
      itemCount: info.playlist.itemCount - 1,
      durationMs: info.playlist.durationMs - gone.track.durationMs,
    };
    info = { ...info, playlist, entries };
    edit(() => app.run(commands.playlistRemoveEntry(id, gone.position)));
  }

  async function rename() {
    menuOpen = false;
    if (!info) return;
    const name = await app.ask({ title: "Rename playlist", input: info.playlist.name, confirm: "Rename" });
    if (!name || name === info.playlist.name) return;
    edit(() => app.run(commands.playlistRename(id, name)));
  }

  async function deletePlaylist() {
    menuOpen = false;
    if (!info) return;
    const sure = await app.ask({
      title: `Delete “${info.playlist.name}”?`,
      message: "The playlist is removed. Its tracks stay in your library.",
      confirm: "Delete",
      danger: true,
    });
    if (sure == null) return;
    await app.run(commands.playlistDelete(id));
    await app.playlistsChanged();
    app.go({ kind: "playlists" });
  }
</script>

<div class="page-full scroll">
  {#if info === null}
    <p class="missing dim">This playlist no longer exists.</p>
  {:else if info}
    {@const playlist = info.playlist}
    <section class="hero">
      <div class="art"><Cover hash={playlist.art} size={1000} alt="" /></div>
      <div class="meta">
        <span class="kicker">Playlist</span>
        <h1 tabindex="-1" data-view-heading>{playlist.name}</h1>
        <div class="facts">
          <span>
            {[plural(tracks.length, "track"), tracks.length ? durationShort(playlist.durationMs) : null]
              .filter(Boolean)
              .join(" • ")}
          </span>
          {#if info.missing}
            <span class="badge" title="Moved or deleted files; they return when a rescan finds them at the same path">
              {plural(info.missing, "track")} not in library
            </span>
          {/if}
        </div>
        <div class="actions">
          <button class="btn primary" disabled={!ids.length} onclick={() => app.run(commands.playerPlayTracks(ids, 0))}>
            <Icon name="play" size={18} /> Play
          </button>
          <button
            class="btn"
            disabled={!ids.length}
            onclick={() => app.run(commands.playerPlayTracks(shuffled(ids), 0))}
          >
            <Icon name="shuffle" size={18} /> Shuffle
          </button>
          <button
            class="icon-btn filled"
            aria-label="Play next"
            disabled={!ids.length}
            onclick={() => app.run(commands.playerPlayNextTracks(ids))}
          >
            <Icon name="play-next" size={20} />
          </button>
          <button
            class="icon-btn filled"
            aria-label="Add to queue"
            disabled={!ids.length}
            onclick={() => app.run(commands.playerEnqueueTracks(ids))}
          >
            <Icon name="queue-add" size={20} />
          </button>
          <button
            bind:this={menuButton}
            class="icon-btn filled"
            aria-label="More for {playlist.name}"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onclick={() => (menuOpen = !menuOpen)}
          >
            <Icon name="more" size={20} />
          </button>
          <Popover anchor={menuButton} bind:open={menuOpen} menu label="Playlist actions">
            <button role="menuitem" onclick={rename}><Icon name="edit" size={18} /> Rename</button>
            <button role="menuitem" onclick={deletePlaylist}><Icon name="trash" size={18} /> Delete playlist</button>
          </Popover>
        </div>
      </div>
    </section>

    <div class="body" in:fadeQuick={{ duration: 200 }}>
      {#if tracks.length}
        <TrackList
          {tracks}
          variant="playlist"
          onplay={(i) => app.run(commands.playerPlayTracks(ids, i))}
          onremove={remove}
          onmove={move}
        />
      {:else}
        <p class="dim">Empty. Use “Add to playlist” in any track’s ⋯ menu to fill it.</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .page-full {
    height: 100%;
    container-type: inline-size;
  }
  .missing {
    padding: 96px var(--page-x) 96px var(--page-x);
  }
  .hero {
    display: flex;
    align-items: flex-end;
    gap: 40px;
    padding: 84px var(--page-x) 36px var(--page-x);
  }
  .art {
    flex: none;
    width: clamp(180px, 22cqw, 240px);
  }
  .art :global(.cover) {
    border-radius: var(--r-lg);
    box-shadow: 0 24px 60px rgb(0 0 0 / 0.6);
  }
  .meta {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }
  h1 {
    font-size: var(--fs-display);
    line-height: 1.02;
    font-weight: 800;
    letter-spacing: -0.035em;
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .facts {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    color: var(--ink-2);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 14px;
  }
  .icon-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .body {
    padding: 8px var(--page-x) var(--dock-reserve) var(--page-x);
  }

  @container (max-width: 760px) {
    .hero {
      flex-direction: column;
      align-items: center;
      text-align: center;
      gap: 22px;
      padding-top: 76px;
    }
    .meta {
      align-items: center;
    }
    .facts,
    .actions {
      justify-content: center;
    }
  }
</style>
