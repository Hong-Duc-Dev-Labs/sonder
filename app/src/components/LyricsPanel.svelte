<script lang="ts">
  // Lyrics beside the record. Synced lyrics follow playback: the line being sung is lit and
  // kept a little above the middle, lines already sung dim, and clicking a line seeks to it.
  // Scrolling by hand pauses the follow for a few seconds. Plain lyrics just scroll.
  import { app } from "../lib/app.svelte";
  import { commands, type Lyrics } from "../lib/bindings";
  import { playbackMs } from "../lib/clock.svelte";
  import { t } from "../lib/i18n.svelte";
  import { activeLine, loadLyrics } from "../lib/lyrics";
  import { onFrame, reducedMotion } from "../lib/motion";

  /** Lines light up this much before their stamp, so the eye arrives with the voice. */
  const LEAD_MS = 150;
  /** How long a hand scroll keeps the view where the user put it. */
  const HOLD_MS = 4000;

  let item = $derived(app.current);
  // A number, not the queue entry: the queue is replaced on every edit, and the lyrics must
  // reload only when the track itself changes.
  let trackId = $derived(app.current?.trackId);
  let online = $derived(app.settings.lyricsOnline ?? true);
  let lyrics = $state<Lyrics | null>(null);
  let loading = $state(false);
  let active = $state(-1);
  let scroller: HTMLDivElement | undefined = $state();
  let heldUntil = 0;

  $effect(() => {
    const id = trackId;
    const on = online;
    void app.lyricsVersion;
    lyrics = null;
    active = -1;
    if (id == null) {
      loading = false;
      return;
    }
    let current = true;
    loading = true;
    loadLyrics(id, on).then((found) => {
      if (!current) return;
      lyrics = found;
      loading = false;
    });
    return () => {
      current = false;
    };
  });

  $effect(() => {
    const l = lyrics;
    if (!l?.synced || !l.lines.length) return;
    return onFrame(() => {
      const i = activeLine(l.lines, playbackMs() + LEAD_MS);
      if (i !== active) active = i;
    });
  });

  // Follow the sung line.
  $effect(() => {
    const i = active;
    if (!scroller || i < 0 || performance.now() < heldUntil) return;
    const line = scroller.querySelector<HTMLElement>(`[data-line="${i}"]`);
    if (!line) return;
    const top = line.offsetTop - scroller.clientHeight * 0.38 + line.offsetHeight / 2;
    scroller.scrollTo({ top, behavior: reducedMotion() ? "auto" : "smooth" });
  });

  function hold() {
    heldUntil = performance.now() + HOLD_MS;
  }

  function seek(ms: number) {
    heldUntil = 0;
    app.run(commands.playerSeek(ms));
  }
</script>

<section class="lyrics" aria-label={t("common.lyrics")}>
  {#if loading}
    <p class="state dim">{t("lyrics.loading")}</p>
  {:else if !item}
    <p class="state dim">{t("lyrics.nothingPlaying")}</p>
  {:else if lyrics?.instrumental}
    <p class="state">{t("lyrics.instrumental")}</p>
  {:else if !lyrics}
    <div class="state">
      <p>{t("lyrics.none")}</p>
      {#if !online}
        <button class="link dim" onclick={() => app.go({ kind: "settings" })}>{t("lyrics.onlineOff")}</button>
      {/if}
    </div>
  {:else}
    <div
      class="scroller"
      class:synced={lyrics.synced}
      bind:this={scroller}
      onwheel={hold}
    >
      {#each lyrics.lines as line, i (i)}
        {#if lyrics.synced}
          <button
            class="line"
            class:on={i === active}
            class:past={i < active}
            class:gap={!line.text}
            data-line={i}
            onclick={() => seek(line.startMs)}
          >
            {line.text || "♪"}
          </button>
        {:else}
          <p class="line plain" class:gap={!line.text}>{line.text || " "}</p>
        {/if}
      {/each}
    </div>
    <p class="source dim">{t("lyrics.source", { source: lyrics.source })}</p>
  {/if}
</section>

<style>
  .lyrics {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .state {
    margin: auto 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    font-size: 20px;
    font-weight: 700;
  }
  .state p {
    margin: 0;
  }
  .state .link {
    font-size: 13px;
    font-weight: 500;
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .scroller {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: none;
    outline: none;
    /* Lines melt away at both edges instead of being cut. */
    mask-image: linear-gradient(to bottom, transparent, #000 14%, #000 80%, transparent);
  }
  /* Room above the first line and below the last, as a share of the column's own height, so
     either can sit where the sung line is kept. */
  .scroller.synced::before,
  .scroller.synced::after {
    content: "";
    display: block;
    height: 38%;
  }
  .scroller.synced::after {
    height: 55%;
  }
  .scroller:not(.synced) {
    padding: 24px 0 48px;
  }
  .scroller::-webkit-scrollbar {
    display: none;
  }
  .line {
    display: block;
    width: 100%;
    margin: 0;
    padding: 7px 10px;
    border-radius: var(--r-md, 10px);
    text-align: left;
    font-size: clamp(20px, 1.3cqw + 10px, 28px);
    line-height: 1.22;
    font-weight: 800;
    letter-spacing: -0.02em;
    color: var(--ink);
    /* how lit a line yet to be sung is; a host on glass raises it to stay readable */
    opacity: var(--lyric-idle, 0.32);
    transform-origin: left center;
    transform: scale(0.97);
    transition:
      opacity 360ms var(--ease-standard),
      transform 420ms var(--ease-spring),
      background var(--dur-fast) var(--ease-standard);
  }
  .line.past {
    opacity: var(--lyric-past, 0.5);
  }
  .line.on {
    opacity: 1;
    transform: none;
    text-shadow: 0 0 22px rgb(255 255 255 / 0.25);
  }
  .line.gap {
    font-size: 20px;
  }
  button.line:hover {
    opacity: 0.85;
    background: var(--soft-hover);
  }
  .line.plain {
    opacity: 0.9;
    transform: none;
    font-size: 18px;
    font-weight: 600;
    padding: 3px 0;
  }
  .line.plain.gap {
    height: 14px;
  }
  .source {
    margin: 6px 0 0;
    font-size: 11.5px;
    text-align: left;
  }
  @media (prefers-reduced-motion: reduce) {
    .line {
      transition: opacity 120ms linear;
      transform: none;
    }
  }
</style>
