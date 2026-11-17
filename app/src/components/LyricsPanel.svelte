<script lang="ts">
  // Lyrics beside the record. Synced lyrics follow playback: the line being sung is lit and
  // kept at the golden ratio (38% from top), lines already sung dim, and clicking a line seeks to it.
  // Scrolling by hand pauses the follow for a few seconds with a floating "Resume autoscroll" pill.
  import { app } from "../lib/app.svelte";
  import { commands, type Lyrics } from "../lib/bindings";
  import { playbackMs } from "../lib/clock.svelte";
  import { t } from "../lib/i18n.svelte";
  import { activeLine, loadLyrics, prepareTimedLines } from "../lib/lyrics";
  import { onFrame, reducedMotion } from "../lib/motion";
  import Icon from "./Icon.svelte";

  /** Lines light up this much before their stamp, so the eye arrives with the voice. */
  const LEAD_MS = 140;
  /** How long a hand scroll keeps the view where the user put it. */
  const HOLD_MS = 5000;

  let item = $derived(app.current);
  let trackId = $derived(app.current?.trackId);
  let online = $derived(app.settings.lyricsOnline ?? true);
  let lyrics = $state<Lyrics | null>(null);
  let timedLines = $derived(lyrics?.synced ? prepareTimedLines(lyrics.lines) : null);
  let loading = $state(false);
  let active = $state(-1);
  let currentMs = $state(0);
  let scroller: HTMLDivElement | undefined = $state();
  let isUserHeld = $state(false);
  let heldUntil = 0;
  let holdTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    const id = trackId;
    const on = online;
    void app.lyricsVersion;
    lyrics = null;
    active = -1;
    currentMs = 0;
    isUserHeld = false;
    heldUntil = 0;
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

  // Follow the sung line unless user is exploring manually
  $effect(() => {
    const i = active;
    if (!scroller || i < 0 || isUserHeld || performance.now() < heldUntil) return;
    const line = scroller.querySelector<HTMLElement>(`[data-line="${i}"]`);
    if (!line) return;
    const top = line.offsetTop - scroller.clientHeight * 0.38 + line.offsetHeight / 2;
    scroller.scrollTo({ top, behavior: reducedMotion() ? "auto" : "smooth" });
  });

  function hold() {
    isUserHeld = true;
    heldUntil = performance.now() + HOLD_MS;
    clearTimeout(holdTimer);
    holdTimer = setTimeout(() => {
      isUserHeld = false;
      resumeFollow();
    }, HOLD_MS);
  }

  function resumeFollow() {
    isUserHeld = false;
    heldUntil = 0;
    clearTimeout(holdTimer);
    if (!scroller || active < 0) return;
    const line = scroller.querySelector<HTMLElement>(`[data-line="${active}"]`);
    if (line) {
      const top = line.offsetTop - scroller.clientHeight * 0.38 + line.offsetHeight / 2;
      scroller.scrollTo({ top, behavior: reducedMotion() ? "auto" : "smooth" });
    }
  }

  function seek(ms: number) {
    heldUntil = 0;
    isUserHeld = false;
    clearTimeout(holdTimer);
    app.run(commands.playerSeek(ms));
  }

  function onScrollerKeyDown(e: KeyboardEvent) {
    if (["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End"].includes(e.key)) {
      hold();
    }
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
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="scroller"
      class:synced={lyrics.synced}
      bind:this={scroller}
      onwheel={hold}
      ontouchstart={hold}
      onkeydown={onScrollerKeyDown}
      tabindex="-1"
    >
      {#if lyrics.synced && timedLines}
        {#if timedLines.length > 0 && timedLines[0].startMs > 4000}
          {@const activeBreak = currentMs + LEAD_MS < timedLines[0].startMs}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="interlude"
            class:active={activeBreak}
            onclick={() => seek(timedLines[0].startMs)}
            title={t("lyrics.instrumental")}
            aria-label={t("lyrics.instrumental")}
          >
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
            aria-current={i === active ? "true" : undefined}
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
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="interlude"
              class:active={activeBreak}
              onclick={() => seek(timedLines[i + 1].startMs)}
              title={t("lyrics.instrumental")}
              aria-label={t("lyrics.instrumental")}
            >
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

    {#if isUserHeld}
      <button
        class="resume-pill"
        onclick={resumeFollow}
        aria-label={t("lyrics.resume")}
      >
        <Icon name="down" size={14} />
        <span>{t("lyrics.resume")}</span>
      </button>
    {/if}

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
    overscroll-behavior-y: contain;
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
    /* Lines melt away at both edges instead of being abruptly cut */
    mask-image: linear-gradient(to bottom, transparent, #000 12%, #000 82%, transparent);
  }
  /* Room above the first line and below the last, as a share of the column's height */
  .scroller.synced::before {
    content: "";
    display: block;
    height: 38%;
  }
  .scroller.synced::after {
    content: "";
    display: block;
    height: 65%;
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
    padding: 5px 10px;
    border-radius: var(--r-md, 10px);
    text-align: left;
    font-size: clamp(16px, 1.05cqw + 6px, 22px);
    line-height: 1.36;
    font-weight: 700;
    letter-spacing: -0.015em;
    color: var(--ink, #fff);
    /* Inactive lines kept legible above WCAG AA 3:1 contrast */
    opacity: var(--lyric-idle, 0.44);
    transform-origin: left center;
    transform: scale(0.97);
    transition:
      opacity 320ms var(--ease-standard, ease),
      transform 360ms var(--ease-spring-smooth, cubic-bezier(0.34, 1.25, 0.64, 1)),
      background var(--dur-fast, 150ms) var(--ease-standard, ease);
    border: none;
    background: transparent;
    cursor: pointer;
  }
  .line:focus-visible {
    outline: 2px solid #fff;
    outline-offset: 2px;
    background: rgba(255, 255, 255, 0.1);
  }
  .line.past {
    opacity: var(--lyric-past, 0.58);
  }
  .line.on {
    opacity: 1;
    transform: none;
    text-shadow: 0 0 20px rgb(255 255 255 / 0.35);
  }
  .line.gap {
    font-size: 16px;
  }
  button.line:hover {
    opacity: 0.9;
    background: var(--soft-hover, rgba(255, 255, 255, 0.1));
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
      opacity 240ms cubic-bezier(0.16, 1, 0.3, 1),
      text-shadow 240ms cubic-bezier(0.16, 1, 0.3, 1),
      transform 240ms cubic-bezier(0.16, 1, 0.3, 1);
    opacity: 0.44;
    transform-origin: 50% 82%;
    will-change: transform, opacity;
  }
  .word.lit {
    opacity: 1;
    text-shadow: 0 0 16px rgba(255, 255, 255, 0.5);
  }
  .word.current {
    transform: translateY(-1px) scale(1.06);
    text-shadow: 0 0 8px #fff, 0 0 20px #fff, 0 0 36px var(--wash, rgba(255, 255, 255, 0.5));
    transition:
      opacity 110ms cubic-bezier(0.34, 1.4, 0.64, 1),
      text-shadow 110ms cubic-bezier(0.34, 1.4, 0.64, 1),
      transform 110ms cubic-bezier(0.34, 1.4, 0.64, 1);
  }
  .interlude {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    opacity: 0.32;
    cursor: pointer;
    border-radius: 999px;
    transition: all 0.2s ease;
  }
  .interlude:hover {
    opacity: 0.75;
    background: rgba(255, 255, 255, 0.08);
  }
  .interlude.active {
    opacity: 0.95;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
    display: inline-block;
  }
  .interlude.active .dot {
    animation: breath 1.2s ease-in-out infinite;
  }
  .interlude.active .dot:nth-child(2) {
    animation-delay: 0.18s;
  }
  .interlude.active .dot:nth-child(3) {
    animation-delay: 0.36s;
  }
  @keyframes breath {
    0%, 100% {
      opacity: 0.35;
      transform: scale(0.85);
    }
    50% {
      opacity: 1;
      transform: scale(1.3);
      box-shadow: 0 0 10px #fff;
    }
  }

  .resume-pill {
    position: absolute;
    bottom: 24px;
    left: 50%;
    transform: translateX(-50%);
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: rgba(18, 18, 24, 0.88);
    backdrop-filter: blur(14px);
    border: 1px solid rgba(255, 255, 255, 0.16);
    color: #fff;
    font-size: 12px;
    font-weight: 600;
    padding: 6px 14px;
    border-radius: 999px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.45);
    cursor: pointer;
    z-index: 15;
    transition: all 0.2s ease;
  }
  .resume-pill:hover {
    background: rgba(255, 255, 255, 0.2);
    border-color: rgba(255, 255, 255, 0.28);
    transform: translateX(-50%) translateY(-1px);
  }
  .resume-pill:focus-visible {
    outline: 2px solid #fff;
    outline-offset: 2px;
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