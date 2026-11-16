<script lang="ts">
  // Lyrics beside the record. Synced lyrics follow playback: the line being sung is lit and
  // kept a little above the middle, lines already sung dim, and clicking a line seeks to it.
  // Scrolling by hand pauses the follow for a few seconds. Plain lyrics just scroll.
  import { app } from "../lib/app.svelte";
  import { commands, type Lyrics } from "../lib/bindings";
  import { playbackMs } from "../lib/clock.svelte";
  import { t } from "../lib/i18n.svelte";
  import { activeLine, loadLyrics, prepareTimedLines } from "../lib/lyrics";
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
  let timedLines = $derived(lyrics?.synced ? prepareTimedLines(lyrics.lines) : null);
  let loading = $state(false);
  let active = $state(-1);
  let currentMs = $state(0);
  let scroller: HTMLDivElement | undefined = $state();
  let heldUntil = 0;

  $effect(() => {
    const id = trackId;
    const on = online;
    void app.lyricsVersion;
    lyrics = null;
    active = -1;
    currentMs = 0;
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
      const ms = playbackMs();
      currentMs = ms;
      const i = activeLine(l.lines, ms + LEAD_MS);
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
      {#if lyrics.synced && timedLines}
        {#if timedLines.length > 0 && timedLines[0].startMs > 4000}
          {@const activeBreak = currentMs + LEAD_MS < timedLines[0].startMs}
          <div class="interlude" class:active={activeBreak} aria-hidden="true">
            <span class="dot"></span>
            <span class="dot"></span>
            <span class="dot"></span>
          </div>
        {/if}

        {#each timedLines as line, i (i)}
          <button
            class="line"
            class:on={i === active}
            class:past={i < active}
            class:gap={!line.text}
            data-line={i}
            onclick={() => seek(line.startMs)}
          >
            {#if i === active && line.words.length > 0}
              <span class="karaoke">
                {#each line.words as word, wi (wi)}
                  {@const lit = currentMs + LEAD_MS >= word.startMs}
                  {@const current = lit && currentMs + LEAD_MS < word.endMs}
                  <span class="word" class:lit class:current>{word.text}</span>{#if word.trailingSpace}{' '}{/if}
                {/each}
              </span>
            {:else}
              {line.text || "♪"}
            {/if}
          </button>

          {#if i < timedLines.length - 1 && timedLines[i + 1].startMs - line.endMs > 4000}
            {@const activeBreak = currentMs + LEAD_MS >= line.endMs && currentMs + LEAD_MS < timedLines[i + 1].startMs}
            <div class="interlude" class:active={activeBreak} aria-hidden="true">
              <span class="dot"></span>
              <span class="dot"></span>
              <span class="dot"></span>
            </div>
          {/if}
        {/each}
      {:else}
        {#each lyrics.lines as line, i (i)}
          <p class="line plain" class:gap={!line.text}>{line.text || " "}</p>
        {/each}
      {/if}
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
  .karaoke {
    display: inline;
  }
  .word {
    display: inline-block;
    transition:
      opacity 140ms ease-out,
      text-shadow 180ms ease-out,
      transform 160ms var(--ease-spring, ease-out);
    opacity: var(--lyric-idle, 0.38);
    transform-origin: center bottom;
    will-change: transform, opacity;
  }
  .word.lit {
    opacity: 1;
    text-shadow: 0 0 16px rgba(255, 255, 255, 0.4);
  }
  .word.current {
    transform: scale(1.05);
    text-shadow: 0 0 22px rgba(255, 255, 255, 0.65);
  }
  .interlude {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 14px;
    opacity: 0.25;
    transition: opacity 300ms ease;
  }
  .interlude.active {
    opacity: 0.9;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
    display: inline-block;
  }
  .interlude.active .dot {
    animation: breath 1.4s ease-in-out infinite;
  }
  .interlude.active .dot:nth-child(2) {
    animation-delay: 0.2s;
  }
  .interlude.active .dot:nth-child(3) {
    animation-delay: 0.4s;
  }
  @keyframes breath {
    0%, 100% {
      opacity: 0.3;
      transform: scale(0.85);
    }
    50% {
      opacity: 1;
      transform: scale(1.25);
    }
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
    .word {
      transition: opacity 120ms linear;
      transform: none !important;
    }
    .interlude.active .dot {
      animation: none;
    }
  }
</style>
