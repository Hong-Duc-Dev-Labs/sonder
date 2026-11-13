<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { commands, type AlbumInfo } from "../lib/bindings";
  import { durationShort, isHiRes, plural, qualitySummary, shuffled } from "../lib/format";
  import { fadeQuick } from "../lib/transitions";
  import BlurredArt from "./BlurredArt.svelte";
  import Cover from "./Cover.svelte";
  import FrostFade from "./FrostFade.svelte";
  import Icon from "./Icon.svelte";
  import RecordAvatar from "./RecordAvatar.svelte";
  import TrackList from "./TrackList.svelte";

  let { id }: { id: number } = $props();
  let info = $state<AlbumInfo | null | undefined>(undefined);
  let shownId: number | null = null;
  // The clicked card's summary draws the header at once (and lets its cover morph in).
  let header = $derived(info?.album ?? (info === undefined ? app.albumPreview.get(id) : undefined));
  let quality = $derived(info ? qualitySummary(info.tracks) : null);
  let hiRes = $derived(info ? info.tracks.some(isHiRes) : false);
  let genre = $derived(info?.tracks.find((t) => t.genre)?.genre ?? null);

  $effect(() => {
    void app.libraryVersion;
    const want = id;
    if (shownId !== want) {
      // Another album: drop the old one so its header never shows under the new id.
      info = undefined;
      shownId = want;
    }
    app.run(commands.libraryAlbum(want)).then((r) => {
      if (want === id) info = r ?? null;
    });
  });

  // The album's colour washes over the sidebar and player while the page is open.
  $effect(() => {
    app.pageWash = header?.artColors[0] ?? null;
    return () => {
      app.pageWash = null;
    };
  });

  function shuffle() {
    if (!info) return;
    app.run(commands.playerPlayTracks(shuffled(info.tracks.map((t) => t.id)), 0));
  }
</script>

<div class="page-full scroll">
  {#if info === null}
    <p class="missing dim">This album is no longer in the library.</p>
  {:else if header}
    {@const album = header}
    <section class="hero">
      <div class="backdrop" aria-hidden="true">
        <BlurredArt hashes={[album.art]} />
        <FrostFade height="60%" />
      </div>
      <div class="hero-inner">
        <div class="sleeve">
          <span class="record"><RecordAvatar art={album.art} label={0.36} /></span>
          <div class="art" style:view-transition-name={app.morphAlbumId === album.id ? "album-art" : undefined}>
            <Cover hash={album.art} size={1000} alt={album.title} />
          </div>
        </div>
        <div class="meta">
          <span class="kicker">Album</span>
          <h1 tabindex="-1" data-view-heading>{album.title}</h1>
          {#if album.artistId != null}
            {@const artistId = album.artistId}
            <button class="artist link" onclick={() => app.go({ kind: "artist", id: artistId })}>{album.artist}</button>
          {:else}
            <span class="artist">{album.artist ?? "Unknown artist"}</span>
          {/if}
          <div class="facts">
            <span>{[album.year, plural(album.trackCount, "track"), durationShort(album.durationMs), genre].filter(Boolean).join(" • ")}</span>
            {#if quality}<span class="badge" class:accent={hiRes}>{quality}</span>{/if}
          </div>
          <div class="actions">
            <button class="btn primary" onclick={() => app.run(commands.playerPlayAlbum(album.id, 0))}>
              <Icon name="play" size={18} /> Play
            </button>
            <button class="btn" disabled={!info} onclick={shuffle}><Icon name="shuffle" size={18} /> Shuffle</button>
            <button class="icon-btn filled" aria-label="Play next" onclick={() => app.run(commands.playerPlayNextAlbum(album.id))}>
              <Icon name="play-next" size={20} />
            </button>
            <button class="icon-btn filled" aria-label="Add to queue" onclick={() => app.run(commands.playerEnqueueAlbum(album.id))}>
              <Icon name="queue-add" size={20} />
            </button>
          </div>
        </div>
      </div>
    </section>

    <div class="body">
      {#if info}
        <div in:fadeQuick={{ duration: 200 }}>
          <TrackList tracks={info.tracks} onplay={(i) => app.run(commands.playerPlayAlbum(album.id, i))} />
        </div>
      {:else}
        <div class="skeleton soft" aria-hidden="true">
          {#each { length: Math.min(album.trackCount, 12) } as _, i (i)}
            <div class="sk-row"><span></span><span></span></div>
          {/each}
        </div>
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
    position: relative;
    padding: 84px var(--page-x) 36px var(--page-x);
    overflow: hidden;
  }
  .backdrop {
    position: absolute;
    inset: 0;
    opacity: 0.85;
    /* Fades out, frost and all, so the hero melts into the lit canvas below with no edge. */
    -webkit-mask-image: linear-gradient(to bottom, #000 55%, transparent 100%);
    mask-image: linear-gradient(to bottom, #000 55%, transparent 100%);
  }
  .hero-inner {
    position: relative;
    display: flex;
    align-items: flex-end;
    gap: 40px;
  }
  .sleeve {
    position: relative;
    flex: none;
    width: clamp(220px, 26cqw, 300px);
  }
  .record {
    position: absolute;
    top: 4%;
    left: 4%;
    width: 92%;
    transform: translateX(30%);
  }
  .art {
    position: relative;
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
    padding-left: 8%;
  }
  h1 {
    font-size: var(--fs-display);
    line-height: 1.02;
    font-weight: 800;
    letter-spacing: -0.035em;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .artist {
    font-size: 18px;
    font-weight: 650;
    text-align: left;
    align-self: flex-start;
  }
  .link {
    text-underline-offset: 4px;
  }
  .link:hover {
    text-decoration: underline;
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
  .body {
    padding: 8px var(--page-x) var(--dock-reserve) var(--page-x);
  }
  .skeleton {
    border-radius: var(--r-lg);
    padding: 6px;
  }
  .sk-row {
    display: flex;
    align-items: center;
    gap: 18px;
    height: 52px;
    padding: 0 18px;
  }
  .sk-row span {
    height: 12px;
    border-radius: var(--r-pill);
    background: var(--soft-hover);
  }
  .sk-row span:first-child {
    width: 18px;
  }
  .sk-row span:last-child {
    width: 40%;
  }

  /* Narrow content: Mochy's centred stack. */
  @container (max-width: 760px) {
    .hero {
      padding-top: 76px;
    }
    .hero-inner {
      flex-direction: column;
      align-items: center;
      text-align: center;
      gap: 22px;
    }
    .record {
      display: none;
    }
    .meta {
      align-items: center;
      padding-left: 0;
    }
    .artist {
      align-self: center;
    }
    .facts,
    .actions {
      justify-content: center;
    }
  }
</style>
