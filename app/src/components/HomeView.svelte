<script lang="ts">
  // Home: a greeting, the record on the platter (or the queue waiting to play), shortcuts to
  // the collections, and shelves of recently added albums, playlists and artists. Everything
  // comes from the library as it is; there is no listening history.
  import { app } from "../lib/app.svelte";
  import { commands, type Album, type Artist } from "../lib/bindings";
  import { duration, plural } from "../lib/format";
  import { greeting } from "../lib/greeting";
  import AlbumCard from "./AlbumCard.svelte";
  import Cover from "./Cover.svelte";
  import FormatBadges from "./FormatBadges.svelte";
  import Icon from "./Icon.svelte";
  import PlayPauseIcon from "./PlayPauseIcon.svelte";
  import RecordAvatar from "./RecordAvatar.svelte";
  import Shelf from "./Shelf.svelte";

  const SHELF = 12;
  const UP_NEXT = 3;
  /** Placeholder cards shown while the first shelf loads, so the page does not jump. */
  const SKELETONS = 7;

  const heading = greeting(new Date().getHours());

  let recent = $state<Album[]>([]);
  let albumTotal = $state<number | null>(null);
  let artists = $state<Artist[] | null>(null);

  $effect(() => {
    app.pageWash = null;
    void app.libraryVersion;
    app.run(commands.libraryAlbums("added", 0, SHELF)).then((r) => {
      if (!r) return;
      recent = r.albums;
      albumTotal = r.total;
    });
    app.run(commands.libraryArtists()).then((r) => {
      if (r) artists = r;
    });
  });

  // The hero shows what is playing or paused; stopped with a queue, the queue's first track.
  let playingItem = $derived(app.current);
  let waiting = $derived(playingItem ? null : (app.queue[0] ?? null));
  let item = $derived(playingItem ?? waiting);
  let playing = $derived(app.player.state === "playing");
  let kicker = $derived(playingItem ? (playing ? "Now playing" : "Paused") : "Up next in the queue");
  let empty = $derived(albumTotal === 0);
  /** The few queue entries after the hero's track, with their queue positions. */
  let upNext = $derived.by(() => {
    const start = playingItem && app.player.index != null ? app.player.index + 1 : waiting ? 1 : 0;
    return app.queue.slice(start, start + UP_NEXT).map((entry, i) => ({ entry, index: start + i }));
  });

  function playOrPause() {
    app.run(waiting ? commands.playerPlayIndex(0) : commands.playerToggle());
  }

  function seeRecent() {
    if (app.settings.albumSort !== "added") app.saveSettings({ ...app.settings, albumSort: "added" });
    app.go({ kind: "albums" });
  }
</script>

<div class="page scroll">
  <header class="head">
    <h1 tabindex="-1" data-view-heading>{heading}</h1>
    {#if albumTotal && artists}
      <span class="dim small">{plural(albumTotal, "album")} · {plural(artists.length, "artist")} · {plural(app.playlists.length, "playlist")}</span>
    {/if}
  </header>

  {#if item}
    <section class="hero soft" aria-label={kicker} style:--hero-wash={item.artColors[0] ?? "transparent"}>
      <button class="hero-art" aria-label="Open Now Playing" onclick={() => app.go({ kind: "now" })}>
        <Cover hash={item.art} alt="" />
      </button>
      <div class="hero-text">
        <span class="kicker">{kicker}</span>
        <h2 title={item.title}>{item.title}</h2>
        <span class="artist">{item.artist ?? "Unknown artist"}</span>
        <button class="album link" onclick={() => app.go({ kind: "album", id: item.albumId })}>{item.album}</button>
        <div class="hero-actions">
          <button class="btn primary" onclick={playOrPause}>
            <PlayPauseIcon playing={!!playingItem && playing} size={18} />
            {playingItem && playing ? "Pause" : "Play"}
          </button>
          <button class="btn" onclick={() => app.go({ kind: "now" })}><Icon name="now" size={18} /> Now Playing</button>
        </div>
        {#if playingItem}<div class="hero-badges"><FormatBadges /></div>{/if}
      </div>
      {#if upNext.length}
        <div class="hero-next">
          <div class="next-head">
            <h3 class="kicker">Up next</h3>
            <button class="chip" aria-expanded={app.queueOpen} onclick={() => app.toggleQueue()}>Queue</button>
          </div>
          {#each upNext as { entry, index } (entry.entryId)}
            <button
              class="next-row"
              aria-label="Play {entry.title}{entry.artist ? ` by ${entry.artist}` : ''}"
              onclick={() => app.run(commands.playerPlayIndex(index))}
            >
              <span class="next-art"><Cover hash={entry.art} alt="" /></span>
              <span class="next-main">
                <span class="next-title">{entry.title}</span>
                <span class="dim small">{entry.artist ?? entry.album}</span>
              </span>
              <span class="next-time">{duration(entry.durationMs)}</span>
            </button>
          {/each}
        </div>
      {/if}
    </section>
  {:else if empty}
    <section class="hero soft empty-lib">
      <Icon name="folder" size={36} />
      <div>
        <h2>Your library is empty</h2>
        <p class="dim">Add a music folder and Sonder will find your albums.</p>
      </div>
      <button class="btn primary" onclick={() => app.go({ kind: "settings" })}>Add a music folder</button>
    </section>
  {:else}
    <section class="hero soft idle">
      <Icon name="disc" size={36} />
      <div>
        <h2>Nothing playing</h2>
        <p class="dim">Pick an album and it will spin here.</p>
      </div>
      <button class="btn" onclick={() => app.go({ kind: "albums" })}>Browse albums</button>
    </section>
  {/if}

  {#if !empty}
    <div class="shortcuts">
      <button class="shortcut soft" onclick={() => app.go({ kind: "playlists" })}>
        <span class="badge-icon"><Icon name="playlist" size={22} /></span>
        <span class="info">
          <span class="name">Playlists</span>
          <span class="dim small">{app.playlists.length ? plural(app.playlists.length, "playlist") : "Make your first"}</span>
        </span>
        <Icon name="chevron" size={18} />
      </button>
      <button class="shortcut soft" onclick={() => app.go({ kind: "artists" })}>
        <span class="badge-icon"><Icon name="artists" size={22} /></span>
        <span class="info">
          <span class="name">Artists</span>
          <span class="dim small">{artists ? plural(artists.length, "artist") : "…"}</span>
        </span>
        <Icon name="chevron" size={18} />
      </button>
    </div>

    {#if albumTotal == null}
      <div class="skeleton" aria-hidden="true">
        <div class="sk-head"></div>
        <div class="sk-row">
          {#each { length: SKELETONS } as _, i (i)}
            <div class="sk-card"><div class="sk-cover"></div><div class="sk-line"></div><div class="sk-line short"></div></div>
          {/each}
        </div>
      </div>
    {:else if recent.length}
      <Shelf title="Recently added" onSeeAll={seeRecent}>
        {#each recent as album (album.id)}
          <AlbumCard {album} />
        {/each}
      </Shelf>
    {/if}

    {#if app.playlists.length}
      <Shelf title="Playlists" onSeeAll={() => app.go({ kind: "playlists" })}>
        {#each app.playlists.slice(0, SHELF) as playlist (playlist.id)}
          <div class="tile">
            <button class="tile-btn" onclick={() => app.go({ kind: "playlist", id: playlist.id })}>
              <span class="tile-art"><Cover hash={playlist.art} alt="" /></span>
              <span class="tile-name">{playlist.name}</span>
              <span class="dim small">{plural(playlist.itemCount, "track")}</span>
            </button>
          </div>
        {/each}
      </Shelf>
    {/if}

    {#if artists?.length}
      <Shelf title="Artists" onSeeAll={() => app.go({ kind: "artists" })}>
        {#each artists.slice(0, SHELF) as artist (artist.id)}
          <div class="tile">
            <button class="tile-btn artist" onclick={() => app.go({ kind: "artist", id: artist.id })}>
              <span class="tile-avatar"><RecordAvatar art={artist.art} label={0.5} /></span>
              <span class="tile-name">{artist.name}</span>
              <span class="dim small">{plural(artist.albumCount || artist.trackCount, artist.albumCount ? "album" : "track")}</span>
            </button>
          </div>
        {/each}
      </Shelf>
    {/if}
  {/if}
</div>

<style>
  .head {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 24px;
  }
  h1 {
    font-size: var(--fs-display);
    font-weight: 800;
    letter-spacing: -0.035em;
    line-height: 1.05;
  }

  /* The record on the platter, its cover's colour washed into the card. */
  .hero {
    position: relative;
    container-type: inline-size;
    display: flex;
    align-items: center;
    gap: 28px;
    padding: 24px;
    border-radius: var(--r-xl);
    background:
      radial-gradient(120% 140% at 0% 0%, color-mix(in srgb, var(--hero-wash, transparent) 34%, transparent), transparent 70%),
      var(--soft);
    box-shadow: inset 0 0 0 1px var(--hairline);
    transition: background var(--dur-slow) var(--ease-standard);
  }
  .hero-art {
    width: clamp(120px, 14vw, 184px);
    flex: none;
    border-radius: var(--r-md);
    overflow: hidden;
    box-shadow: 0 14px 34px rgb(0 0 0 / 0.45);
    transition: transform var(--dur-med) var(--ease-spring);
  }
  .hero-art:hover {
    transform: scale(1.03);
  }
  .hero-text {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    min-width: 0;
  }
  .hero-badges {
    margin-top: 14px;
  }

  /* What plays after the hero's track; only when the card is wide enough to spare the room. */
  .hero-next {
    display: none;
    flex: none;
    width: min(38%, 380px);
    flex-direction: column;
    gap: 2px;
    padding-left: 24px;
    border-left: 1px solid var(--hairline);
  }
  @container (min-width: 900px) {
    .hero-next {
      display: flex;
    }
  }
  .next-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 0 0 6px 8px;
  }
  .next-head .kicker {
    margin: 0;
    color: var(--ink-2);
  }
  .next-row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    padding: 6px 10px 6px 8px;
    border-radius: var(--r-sm);
    text-align: left;
    transition: background var(--dur-fast) var(--ease-standard);
  }
  .next-row:hover {
    background: var(--soft-hover);
  }
  .next-art {
    width: 40px;
    flex: none;
    border-radius: 6px;
    overflow: hidden;
  }
  .next-main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .next-title,
  .next-main .dim {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .next-title {
    font-weight: 650;
  }
  .next-time {
    font-size: var(--fs-small);
    font-variant-numeric: tabular-nums;
    color: var(--ink-2);
  }

  /* Stand-in for the first shelf while it loads: same height, so nothing below it jumps. */
  .skeleton {
    margin-top: 36px;
  }
  .sk-head {
    width: 160px;
    height: 20px;
    margin: 10px 0 24px;
    border-radius: var(--r-pill);
    background: var(--soft);
  }
  .sk-row {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 172px;
    gap: 22px;
    overflow: hidden;
  }
  .sk-cover {
    aspect-ratio: 1;
    border-radius: var(--r-md);
    background: var(--soft);
  }
  .sk-line {
    height: 12px;
    width: 80%;
    margin-top: 14px;
    border-radius: var(--r-pill);
    background: var(--soft);
  }
  .sk-line.short {
    width: 50%;
    margin-top: 8px;
  }
  .hero h2 {
    max-width: 100%;
    font-size: var(--fs-headline);
    font-weight: 800;
    letter-spacing: -0.025em;
    line-height: 1.15;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .artist {
    font-size: var(--fs-title);
    color: var(--ink-2);
  }
  .album {
    font-size: var(--fs-small);
    color: var(--ink-2);
  }
  .album:hover {
    color: var(--ink);
    text-decoration: underline;
  }
  .hero-actions {
    display: flex;
    gap: 10px;
    margin-top: 14px;
  }
  .hero.idle,
  .hero.empty-lib {
    gap: 20px;
    color: var(--ink-2);
  }
  .hero.idle > div,
  .hero.empty-lib > div {
    flex: 1;
  }
  .hero.idle h2,
  .hero.empty-lib h2 {
    font-size: var(--fs-title);
    color: var(--ink);
  }
  .hero.idle p,
  .hero.empty-lib p {
    margin: 2px 0 0;
  }

  .shortcuts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 14px;
    margin-top: 16px;
  }
  .shortcut {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 14px 16px 14px 14px;
    border-radius: var(--r-lg);
    text-align: left;
    color: var(--ink-2);
    transition:
      background var(--dur-fast) var(--ease-standard),
      transform var(--dur-med) var(--ease-spring);
  }
  .shortcut:hover {
    background: var(--soft-hover);
    color: var(--ink);
  }
  .shortcut:active {
    transform: scale(0.98);
  }
  .badge-icon {
    width: 44px;
    height: 44px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: var(--r-sm);
    background: var(--soft-active);
    color: var(--ink);
  }
  .info {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    font-weight: 700;
    color: var(--ink);
  }

  .tile-btn {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    width: 100%;
    text-align: left;
    border-radius: var(--r-md);
  }
  .tile-art {
    width: 100%;
    margin-bottom: 10px;
    border-radius: var(--r-md);
    overflow: hidden;
    transition: transform var(--dur-med) var(--ease-spring);
  }
  .tile-avatar {
    width: 100%;
    margin-bottom: 10px;
    transition: transform var(--dur-slow) var(--ease-spring);
  }
  .tile-btn:hover .tile-art {
    transform: translateY(-3px);
  }
  .tile-btn:hover .tile-avatar {
    transform: rotate(35deg);
  }
  .tile-btn.artist {
    align-items: center;
    text-align: center;
  }
  .tile-name {
    max-width: 100%;
    font-weight: 650;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @media (max-width: 760px) {
    .hero {
      flex-direction: column;
      align-items: flex-start;
    }
  }
</style>
