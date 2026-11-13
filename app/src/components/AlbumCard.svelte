<script lang="ts">
  // An album in a grid: a real button (opens the album), with Play as a sibling over the cover
  // and a record that slides half out of the sleeve on hover. Only the card last opened carries
  // the shared view-transition name, so navigating morphs one cover instead of every card.
  import type { Album } from "../lib/bindings";
  import { app } from "../lib/app.svelte";
  import { commands } from "../lib/bindings";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";
  import RecordAvatar from "./RecordAvatar.svelte";

  let { album }: { album: Album } = $props();

  function open() {
    app.albumPreview.set(album.id, album);
    app.morphAlbumId = album.id;
    app.go({ kind: "album", id: album.id });
  }
</script>

<div class="card">
  <div class="sleeve">
    <span class="record"><RecordAvatar art={album.art} /></span>
    <div class="art" style:view-transition-name={app.morphAlbumId === album.id ? "album-art" : undefined}>
      <Cover hash={album.art} alt="" />
    </div>
    <button class="play" aria-label="Play {album.title}" onclick={() => app.run(commands.playerPlayAlbum(album.id, 0))}>
      <Icon name="play" size={22} />
    </button>
  </div>
  <button class="open" onclick={open} aria-label="{album.title}{album.artist ? ` by ${album.artist}` : ''}">
    <span class="title">{album.title}</span>
    <span class="artist">{album.artist ?? "Unknown artist"}{album.year ? ` · ${album.year}` : ""}</span>
  </button>
</div>

<style>
  .card {
    position: relative;
    min-width: 0;
  }
  .sleeve {
    position: relative;
    aspect-ratio: 1;
    transition: transform var(--dur-med) var(--ease-spring);
  }
  .record {
    position: absolute;
    top: 6%;
    left: 6%;
    width: 88%;
    transition: transform var(--dur-slow) var(--ease-spring);
  }
  .art {
    position: relative;
  }
  .art :global(.cover) {
    border-radius: var(--r-md);
    box-shadow: 0 10px 26px rgb(0 0 0 / 0.45);
  }
  .card:hover .sleeve,
  .card:focus-within .sleeve {
    transform: translateY(-4px);
  }
  .card:hover .record,
  .card:focus-within .record {
    transform: translateX(26%) rotate(40deg);
  }
  /* The open button's hit area covers the whole card; Play sits above it. */
  .open {
    display: flex;
    flex-direction: column;
    width: 100%;
    margin-top: 12px;
    text-align: left;
  }
  .open::after {
    content: "";
    position: absolute;
    inset: 0;
    z-index: 1;
    border-radius: var(--r-md);
  }
  .open:focus-visible {
    outline: none;
    box-shadow: none;
  }
  :global(:root:not([data-modality="pointer"])) .open:focus-visible::after {
    outline: 2px solid var(--ink);
    outline-offset: 4px;
  }
  .title {
    font-weight: 650;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .artist {
    margin-top: 2px;
    color: var(--ink-2);
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .play {
    position: absolute;
    z-index: 2;
    right: 10px;
    bottom: 10px;
    width: 46px;
    height: 46px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--ink);
    color: #050506;
    box-shadow: 0 6px 18px rgb(0 0 0 / 0.5);
    opacity: 0;
    transform: translateY(8px) scale(0.9);
    transition:
      opacity var(--dur-fast) var(--ease-standard),
      transform var(--dur-med) var(--ease-spring);
  }
  .card:hover .play,
  .card:focus-within .play {
    opacity: 1;
    transform: none;
  }
  .play:active {
    transform: scale(0.9);
  }
</style>
