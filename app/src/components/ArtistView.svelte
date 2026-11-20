<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { commands, type ArtistInfo } from "../lib/bindings";
  import { plural, shuffled } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import AlbumCard from "./AlbumCard.svelte";
  import BlurredArt from "./BlurredArt.svelte";
  import FrostFade from "./FrostFade.svelte";
  import Icon from "./Icon.svelte";
  import RecordAvatar from "./RecordAvatar.svelte";

  let { id }: { id: number } = $props();
  let info = $state<ArtistInfo | null | undefined>(undefined);

  $effect(() => {
    void app.libraryVersion;
    const want = id;
    app.run(commands.libraryArtist(want)).then((r) => {
      if (want === id) info = r ?? null;
    });
  });

  let covers = $derived(info ? [...info.albums, ...info.appearsOn].map((a) => a.art).filter(Boolean) : []);
  let sections = $derived(
    info
      ? [
          { key: "albums", title: t("common.albums"), list: [...info.albums].reverse() },
          { key: "appears", title: t("artist.appearsOn"), list: info.appearsOn },
        ].filter((s) => s.list.length > 0)
      : [],
  );

  $effect(() => {
    const first = info ? [...info.albums, ...info.appearsOn].find((a) => a.artColors.length) : undefined;
    app.pageWash = first?.artColors[0] ?? null;
    return () => {
      app.pageWash = null;
    };
  });

  async function play(shuffle: boolean) {
    const tracks = await app.run(commands.libraryArtistTracks(id));
    if (!tracks?.length) return;
    const ids = tracks.map((t) => t.id);
    app.run(commands.playerPlayTracks(shuffle ? shuffled(ids) : ids, 0));
  }
</script>

<div class="page-full scroll">
  {#if info === null}
    <p class="missing dim">{t("artist.missing")}</p>
  {:else if info}
    <section class="hero">
      <div class="backdrop" aria-hidden="true">
        <BlurredArt hashes={covers} tilt={-6} />
        <FrostFade height="70%" />
      </div>
      <div class="hero-inner">
        <span class="avatar"><RecordAvatar art={info.artist.art} label={0.5} /></span>
        <div class="meta">
          <span class="kicker">{t("artist.kicker")}</span>
          <h1 tabindex="-1" data-view-heading>{info.artist.name}</h1>
          <span class="facts">
            {[
              info.albums.length ? plural(info.albums.length, "album") : null,
              info.appearsOn.length ? plural(info.appearsOn.length, "appearance") : null,
              plural(info.artist.trackCount, "track"),
            ]
              .filter(Boolean)
              .join(" • ")}
          </span>
          <div class="actions">
            <button class="btn primary" onclick={() => play(false)}><Icon name="play" size={18} /> {t("common.playAll")}</button>
            <button class="btn" onclick={() => play(true)}><Icon name="shuffle" size={18} /> {t("common.shuffle")}</button>
          </div>
        </div>
      </div>
    </section>

    <div class="body">
      {#each sections as section (section.key)}
        <h2>{section.title}</h2>
        <div class="grid">
          {#each section.list as album (album.id)}
            <AlbumCard {album} />
          {/each}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .page-full {
    height: 100%;
  }
  .missing {
    padding: 48px var(--page-x) 48px var(--page-x);
  }
  .hero {
    position: relative;
    min-height: clamp(300px, 42vh, 420px);
    display: flex;
    align-items: flex-end;
    padding: 44px var(--page-x) 32px var(--page-x);
    overflow: hidden;
  }
  .backdrop {
    position: absolute;
    inset: 0;
    /* Fades out, frost and all, so the hero melts into the lit canvas below with no edge. */
    -webkit-mask-image: linear-gradient(to bottom, #000 55%, transparent 100%);
    mask-image: linear-gradient(to bottom, #000 55%, transparent 100%);
  }
  .hero-inner {
    position: relative;
    display: flex;
    align-items: flex-end;
    gap: 28px;
  }
  .avatar {
    width: 136px;
    flex: none;
  }
  .meta {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  h1 {
    font-size: clamp(42px, 5.5vw, 76px);
    line-height: 0.98;
    font-weight: 800;
    letter-spacing: -0.04em;
    overflow-wrap: anywhere;
  }
  .facts {
    color: var(--ink-2);
  }
  .actions {
    display: flex;
    gap: 10px;
    margin-top: 12px;
  }
  .body {
    padding: 8px var(--page-x) var(--dock-reserve) var(--page-x);
  }
  h2 {
    margin: 22px 0 18px;
    font-size: 22px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(176px, 1fr));
    gap: 28px;
  }
</style>
