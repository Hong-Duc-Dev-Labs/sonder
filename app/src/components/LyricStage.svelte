<script lang="ts">
  // Cinematic Flow Lyric Stage for Now Playing.
  // Damped spring physics (springStep), organic typographic rhythm with lineRandom,
  // ambient glow powered by album cover dominant color, and breathing interlude dots.
  import { app } from "../lib/app.svelte";
  import { commands, type Lyrics } from "../lib/bindings";
  import { playbackMs } from "../lib/clock.svelte";
  import { t } from "../lib/i18n.svelte";
  import { activeLine, loadLyrics, prepareTimedLines, type TimedWord } from "../lib/lyrics";
  import { onFrame, reducedMotion } from "../lib/motion";
  import { clamp01, easeOutCubic, lerp, springStep } from "../lib/stageMotion";

  const LEAD_MS = 150;

  let item = $derived(app.current);
  let trackId = $derived(app.current?.trackId);
  let online = $derived(app.settings.lyricsOnline ?? true);
  let lyrics = $state<Lyrics | null>(null);
  let loading = $state(false);
  let active = $state(-1);
  let currentMs = $state(0);
  let accentColor = $derived(item?.artColors[0] ?? "var(--accent, #3b82f6)");

  let timedLines = $derived(lyrics?.synced ? prepareTimedLines(lyrics.lines) : null);

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
    const l = timedLines;
    if (!l?.length) return;
    return onFrame(() => {
      const ms = playbackMs();
      currentMs = ms;
      const i = activeLine(l, ms + LEAD_MS);
      if (i !== active) active = i;
    });
  });

  function seek(ms: number) {
    app.run(commands.playerSeek(ms));
  }

  let curLine = $derived(timedLines && active >= 0 && active < timedLines.length ? timedLines[active] : null);
  let prevLine = $derived(timedLines && active > 0 ? timedLines[active - 1] : null);
  let nextLine = $derived(timedLines && active + 1 < timedLines.length ? timedLines[active + 1] : null);

  let isIntroInterlude = $derived.by(() => {
    if (!timedLines?.length) return false;
    return timedLines[0].startMs > 4000 && currentMs + LEAD_MS < timedLines[0].startMs;
  });

  let isMidInterlude = $derived.by(() => {
    if (!curLine || !nextLine) return false;
    const gap = nextLine.startMs - curLine.endMs;
    const now = currentMs + LEAD_MS;
    return gap > 4000 && now >= curLine.endMs && now < nextLine.startMs;
  });

  let inInterlude = $derived(isIntroInterlude || isMidInterlude);

  function stageIn(_node: Element) {
    if (reducedMotion()) return { duration: 150, css: (t: number) => `opacity: ${t};` };
    return {
      duration: 360,
      css: (t: number) => {
        const eased = easeOutCubic(t);
        const y = (1 - eased) * 20;
        return `opacity: ${eased}; transform: translate3d(0, ${y}px, 0);`;
      },
    };
  }

  function stageOut(_node: Element) {
    if (reducedMotion()) return { duration: 120, css: (t: number) => `opacity: ${t};` };
    return {
      duration: 240,
      css: (t: number) => {
        const eased = easeOutCubic(t);
        const y = (1 - eased) * -16;
        return `opacity: ${eased}; transform: translate3d(0, ${y}px, 0);`;
      },
    };
  }

  function getWordStyle(word: TimedWord, nowMs: number, reduced: boolean): string {
    if (reduced) {
      const isLit = nowMs >= word.startMs;
      return `opacity: ${isLit ? 1 : 0.35};`;
    }

    const dx = word.dx ?? 0;
    const dy = word.dy ?? 0;
    const rot = word.rot ?? 0;
    const duration = Math.max(0.12, (word.endMs - word.startMs) / 1000);

    if (nowMs < word.startMs) {
      return `opacity: 0.28; transform: translate3d(${dx.toFixed(1)}px, ${(dy + 4).toFixed(1)}px, 0) rotate(${rot.toFixed(1)}deg) scale(0.94);`;
    }

    const since = (nowMs - word.startMs) / 1000;

    if (nowMs < word.endMs) {
      const spring = springStep(since);
      const progress = clamp01(since / duration);
      const scale = lerp(0.94, 1.16, spring);
      const glow = Math.sin(progress * Math.PI);
      const opacity = clamp01(since / 0.06);

      return `opacity: ${opacity.toFixed(2)}; transform: translate3d(${dx.toFixed(1)}px, ${dy.toFixed(1)}px, 0) rotate(${rot.toFixed(1)}deg) scale(${scale.toFixed(3)}); text-shadow: 0 0 ${Math.round(24 * glow)}px ${accentColor}, 0 0 16px rgba(255, 255, 255, 0.7);`;
    }

    const afterSince = (nowMs - word.endMs) / 1000;
    const settle = easeOutCubic(afterSince / 0.35);
    const scale = lerp(1.16, 1.0, settle);
    const opacity = lerp(1.0, 0.75, settle);
    const glow = lerp(0.25, 0.05, settle);

    return `opacity: ${opacity.toFixed(2)}; transform: translate3d(${dx.toFixed(1)}px, ${dy.toFixed(1)}px, 0) rotate(${rot.toFixed(1)}deg) scale(${scale.toFixed(3)}); text-shadow: 0 0 ${Math.round(10 * glow)}px ${accentColor};`;
  }
</script>

<div
  class="stage-container"
  class:landing={app.flight?.target === "stage" && !app.flight.landed}
  data-flight="stage"
  style:--stage-accent={accentColor}
>
  {#if loading}
    <div class="stage-state">
      <p class="dim">{t("lyrics.loading")}</p>
    </div>
  {:else if !item}
    <div class="stage-state">
      <p class="dim">{t("lyrics.nothingPlaying")}</p>
    </div>
  {:else if lyrics?.instrumental}
    <div class="stage-state">
      <div class="interlude-stage">
        <span class="interlude-dot d1"></span>
        <span class="interlude-dot d2"></span>
        <span class="interlude-dot d3"></span>
      </div>
      <p class="inst-label">{t("lyrics.instrumental")}</p>
    </div>
  {:else if !lyrics}
    <div class="stage-state">
      <p>{t("lyrics.none")}</p>
      {#if !online}
        <button class="link dim" onclick={() => app.go({ kind: "settings" })}>{t("lyrics.onlineOff")}</button>
      {/if}
    </div>
  {:else if !lyrics.synced}
    <div class="stage-state">
      <p class="dim">{t("lyrics.unsynced")}</p>
    </div>
  {:else if !timedLines?.length}
    <div class="stage-state">
      <p>{t("lyrics.none")}</p>
    </div>
  {:else}
    <div class="stage-content">
      {#if prevLine && !inInterlude}
        <button class="peek-line prev" onclick={() => seek(prevLine.startMs)}>
          {prevLine.text || "♪"}
        </button>
      {:else}
        <div class="peek-spacer"></div>
      {/if}

      <div class="center-stage">
        {#if inInterlude}
          <div class="interlude-stage" aria-label="Instrumental break">
            <span class="interlude-dot d1"></span>
            <span class="interlude-dot d2"></span>
            <span class="interlude-dot d3"></span>
          </div>
        {:else if curLine}
          {#key curLine.startMs}
            <div class="active-line-wrapper" in:stageIn out:stageOut>
              <div class="words-flow">
                {#each curLine.words as word, wi (wi)}
                  <span
                    class="stage-word"
                    style={getWordStyle(word, currentMs + LEAD_MS, reducedMotion())}
                  >
                    {word.text}
                  </span>{#if word.trailingSpace}{' '}{/if}
                {/each}
              </div>
            </div>
          {/key}
        {/if}
      </div>

      {#if nextLine}
        <button class="peek-line next" onclick={() => seek(nextLine.startMs)}>
          {nextLine.text || "♪"}
        </button>
      {:else}
        <div class="peek-spacer"></div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .stage-container {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    padding: 24px 36px;
    box-sizing: border-box;
    user-select: none;
    background: transparent;
  }
  .stage-container.landing {
    opacity: 0;
  }

  .stage-content {
    position: relative;
    z-index: 1;
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    align-items: center;
    text-align: center;
  }

  .center-stage {
    flex: 1;
    width: 100%;
    display: grid;
    place-items: center;
    min-height: 0;
    position: relative;
  }

  .active-line-wrapper {
    grid-area: 1 / 1;
    width: 100%;
    display: flex;
    justify-content: center;
    align-items: center;
  }

  .words-flow {
    display: inline-block;
    max-width: min(960px, 92vw);
    font-size: clamp(28px, 3.6cqw + 14px, 52px);
    line-height: 1.25;
    font-weight: 800;
    letter-spacing: -0.025em;
    color: var(--ink, #fff);
    text-wrap: balance;
  }

  .stage-word {
    display: inline-block;
    will-change: transform, opacity;
    transform-origin: center bottom;
  }

  .peek-spacer {
    height: 28px;
  }

  .peek-line {
    background: none;
    border: none;
    color: var(--ink-2, rgba(255, 255, 255, 0.45));
    font-family: inherit;
    font-weight: 600;
    max-width: 82%;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
    transition: color 180ms ease, opacity 180ms ease;
    padding: 4px 10px;
    border-radius: var(--r-md, 10px);
  }
  .peek-line:hover {
    color: var(--ink, #fff);
    background: var(--soft-hover, rgba(255, 255, 255, 0.08));
  }
  .peek-line.prev {
    font-size: 15px;
    opacity: 0.35;
    margin-bottom: auto;
  }
  .peek-line.next {
    font-size: 17px;
    opacity: 0.44;
    margin-top: auto;
  }

  .interlude-stage {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
  }

  .interlude-dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--stage-accent, #fff);
    box-shadow: 0 0 16px var(--stage-accent, #fff);
    animation: stageBreath 1.6s ease-in-out infinite;
  }
  .interlude-dot.d2 {
    animation-delay: 0.25s;
  }
  .interlude-dot.d3 {
    animation-delay: 0.5s;
  }

  @keyframes stageBreath {
    0%, 100% {
      opacity: 0.25;
      transform: scale(0.8);
    }
    50% {
      opacity: 1;
      transform: scale(1.35);
    }
  }

  .stage-state {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    font-size: 18px;
    font-weight: 700;
  }
  .inst-label {
    margin-top: 10px;
    font-size: 16px;
    color: var(--ink-2);
  }
  .stage-state .link {
    font-size: 13px;
    font-weight: 500;
    text-decoration: underline;
    text-underline-offset: 3px;
  }

  @media (prefers-reduced-motion: reduce) {
    .stage-word {
      transform: none !important;
    }
    .interlude-dot {
      animation: none;
    }
  }
</style>
