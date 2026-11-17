<script lang="ts">
  // Folia Flow Lyric Stage for Now Playing.
  // Ported directly from MyGo Music (egoist/mygo-music internal/app/ui_stage.go):
  // - Critical spring step physics (springStep, stiffness 200, damping 20)
  // - Line seeding (lineRandom), organic layout switching (start, center, end) and lift (-14%, 0, +12%)
  // - Waiting word position (sin/cos offset, blur 10px, 0.5x scale)
  // - Word swelling to 1.4x as sung with glowing text overlay, settling with smootherstep and gentle drift
  // - Simultaneous line crossfade (previous line fades over 0.4s while new line arrives)
  // - Interlude 3 breathing dots with progress illumination for breaks > 4s
  import { app } from "../lib/app.svelte";
  import { commands, type Lyrics } from "../lib/bindings";
  import { playbackMs } from "../lib/clock.svelte";
  import { t } from "../lib/i18n.svelte";
  import { activeLine, loadLyrics, prepareTimedLines } from "../lib/lyrics";
  import { onFrame, reducedMotion } from "../lib/motion";
  import {
    clamp01,
    computeLineLayout,
    easeOutCubic,
    evaluateWord,
    type LineLayout,
  } from "../lib/stageMotion";

  let item = $derived(app.current);
  let trackId = $derived(app.current?.trackId);
  let online = $derived(app.settings.lyricsOnline ?? true);
  let lyrics = $state<Lyrics | null>(null);
  let loading = $state(false);
  let active = $state(-1);
  let currentMs = $state(0);
  let accentColor = $derived(item?.artColors[0] ?? "var(--accent, #38bdf8)");

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
    const isInst = lyrics?.instrumental;
    const l = timedLines;
    if (!isInst && !l?.length) return;
    return onFrame(() => {
      const ms = playbackMs();
      currentMs = ms;
      if (l?.length) {
        const i = activeLine(l, ms);
        if (i !== active) active = i;
      }
    });
  });

  function seek(ms: number) {
    app.run(commands.playerSeek(ms));
  }

  let curLine = $derived(timedLines && active >= 0 && active < timedLines.length ? timedLines[active] : null);
  let prevLine = $derived(timedLines && active > 0 ? timedLines[active - 1] : null);
  let nextLine = $derived(timedLines && active + 1 < timedLines.length ? timedLines[active + 1] : null);

  // Dynamic window width for responsive font sizing matching mygo-music (f = min(64, max(36, w * 0.045)))
  let winW = $state(1280);

  // Adaptive font scaling for dense verses to prevent wrapping collisions
  let wordCount = $derived(curLine?.words.length ?? 0);
  let fontMultiplier = $derived(wordCount > 10 ? 0.86 : wordCount > 7 ? 0.93 : 1.0);
  let stageFontSize = $derived(
    Math.min(64, Math.max(36, (winW || 1280) * 0.045)) * fontMultiplier,
  );

  // Line layout calculation with spaceWords collision avoidance
  let curLayout = $derived<LineLayout | null>(
    curLine ? computeLineLayout(curLine.startMs, curLine.words, stageFontSize) : null,
  );
  let prevLayout = $derived<LineLayout | null>(
    prevLine ? computeLineLayout(prevLine.startMs, prevLine.words, stageFontSize) : null,
  );

  // Crossfade calculation: previous line fades out over 0.4s (400ms) after curLine starts
  let prevFade = $derived.by(() => {
    if (!curLine || !prevLine) return 0;
    const sinceCur = (currentMs - curLine.startMs) / 400;
    if (sinceCur < 0) return 1;
    if (sinceCur >= 1) return 0;
    return 1 - easeOutCubic(sinceCur);
  });

  // Interlude break detection (> 4s gaps)
  let interludeInfo = $derived.by(() => {
    if (!timedLines?.length) return null;
    // Intro break
    if (timedLines[0].startMs > 4000 && currentMs < timedLines[0].startMs) {
      return { from: 0, to: timedLines[0].startMs };
    }
    // Mid-song break between lines
    if (curLine && nextLine) {
      const gap = nextLine.startMs - curLine.endMs;
      if (gap > 4000 && currentMs >= curLine.endMs && currentMs < nextLine.startMs) {
        return { from: curLine.endMs, to: nextLine.startMs };
      }
    }
    return null;
  });

  let inInterlude = $derived(interludeInfo != null);

  // Interlude breathing calculation with reduced motion safety
  let interludeBreath = $derived.by(() => {
    if (reducedMotion()) return 1;
    const time = currentMs;
    return 1 + 0.12 * Math.sin(time / 420);
  });

  let interludeProgress = $derived.by(() => {
    if (!interludeInfo) return 0;
    const span = Math.max(100, interludeInfo.to - interludeInfo.from);
    return clamp01((currentMs - interludeInfo.from) / span);
  });

  function onStageKeyDown(e: KeyboardEvent) {
    if (!timedLines?.length) return;
    if (e.key === "ArrowDown" || e.key === "ArrowRight") {
      if (nextLine) {
        e.preventDefault();
        seek(nextLine.startMs);
      }
    } else if (e.key === "ArrowUp" || e.key === "ArrowLeft") {
      if (prevLine) {
        e.preventDefault();
        seek(prevLine.startMs);
      } else if (curLine && currentMs > curLine.startMs + 1000) {
        e.preventDefault();
        seek(curLine.startMs);
      }
    }
  }
</script>

<svelte:window bind:innerWidth={winW} />

<!-- Screen Reader Polite Live Region for Active Lyrics -->
<div class="sr-only" aria-live="polite" aria-atomic="true">
  {#if inInterlude}
    {t("lyrics.instrumental")}
  {:else if curLine}
    {curLine.text}
  {/if}
</div>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="stage-container"
  tabindex="0"
  role="region"
  aria-label={t("np.tabStage")}
  onkeydown={onStageKeyDown}
  style:--stage-accent={accentColor}
  style:--stage-mult={fontMultiplier}
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
      <div class="interlude-row" aria-hidden="true">
        {#each [0, 1, 2] as _i}
          <span class="interlude-dot lit" style:transform="scale({interludeBreath})"></span>
        {/each}
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
    <div class="stage-main-area">
      {#if inInterlude}
        <div class="interlude-container" aria-label="Instrumental interlude">
          <div class="interlude-row">
            {#each [0, 1, 2] as i}
              {@const lit = clamp01(interludeProgress * 3 - i)}
              <span
                class="interlude-dot"
                style:background="rgba(255, 255, 255, {(0.2 + 0.7 * lit).toFixed(2)})"
                style:transform="scale({interludeBreath.toFixed(3)})"
              ></span>
            {/each}
          </div>
        </div>
      {:else}
        <!-- Previous line dissolving out during line switch -->
        {#if prevLine && prevLayout && prevFade > 0.01}
          <div
            class="stage-line-row prev-fading"
            style:justify-content={prevLayout.justify}
            style:transform={reducedMotion() ? undefined : `translate3d(0, calc(${prevLayout.liftRatio} * 30vh), 0)`}
            style:opacity={prevFade.toFixed(2)}
            style:--stage-f="{stageFontSize}px"
          >
            {#each prevLine.words as word, i (i)}
              {@const shape = prevLayout.shapes[i] ?? { x: 0, y: 0, rotate: 0, scale: 1, drift: 0, margin: 16 }}
              {@const frame = evaluateWord(shape, word.startMs, word.endMs, currentMs, reducedMotion())}
              {#if frame.opacity > 0.005}
                <span
                  class="stage-word-box"
                  style:margin-right="{frame.margin.toFixed(1)}px"
                  style:transform="translate3d({frame.x.toFixed(1)}px, {frame.y.toFixed(1)}px, 0) rotate({frame.rotate.toFixed(1)}deg) scale({frame.scale.toFixed(3)})"
                  style:opacity={(frame.opacity * prevFade).toFixed(2)}
                >
                  <span
                    class="word-body"
                    style:filter={frame.blur > 0.3 ? `blur(${frame.blur.toFixed(1)}px)` : undefined}
                    style:color="rgba(255, 255, 255, {frame.textColorAlpha.toFixed(2)})"
                  >
                    {word.text}
                  </span>
                  {#if frame.glow > 0.01}
                    <span
                      class="word-glow"
                      aria-hidden="true"
                      style:color={accentColor}
                      style:opacity={(0.85 * frame.glow * prevFade).toFixed(2)}
                    >
                      {word.text}
                    </span>
                  {/if}
                </span>
              {/if}
            {/each}
          </div>
        {/if}

        <!-- Current line in Folia Flow kinetic typography -->
        {#if curLine && curLayout}
          <div
            class="stage-line-row cur-active"
            style:justify-content={curLayout.justify}
            style:transform={reducedMotion() ? undefined : `translate3d(0, calc(${curLayout.liftRatio} * 30vh), 0)`}
            style:--stage-f="{stageFontSize}px"
          >
            {#each curLine.words as word, i (i)}
              {@const shape = curLayout.shapes[i] ?? { x: 0, y: 0, rotate: 0, scale: 1, drift: 0, margin: 16 }}
              {@const frame = evaluateWord(shape, word.startMs, word.endMs, currentMs, reducedMotion())}
              {#if frame.opacity > 0.005}
                <span
                  class="stage-word-box"
                  style:margin-right="{frame.margin.toFixed(1)}px"
                  style:transform="translate3d({frame.x.toFixed(1)}px, {frame.y.toFixed(1)}px, 0) rotate({frame.rotate.toFixed(1)}deg) scale({frame.scale.toFixed(3)})"
                  style:opacity={frame.opacity.toFixed(2)}
                >
                  <span
                    class="word-body"
                    style:filter={frame.blur > 0.3 ? `blur(${frame.blur.toFixed(1)}px)` : undefined}
                    style:color="rgba(255, 255, 255, {frame.textColorAlpha.toFixed(2)})"
                  >
                    {word.text}
                  </span>
                  {#if frame.glow > 0.01}
                    <span
                      class="word-glow"
                      aria-hidden="true"
                      style:color={accentColor}
                      style:opacity={(0.85 * frame.glow).toFixed(2)}
                    >
                      {word.text}
                    </span>
                  {/if}
                </span>
              {/if}
            {/each}
          </div>
        {/if}
      {/if}
    </div>

    <!-- Foot strip: Next Line preview (clean single line like mygo-music) -->
    <footer class="stage-foot">
      {#if nextLine}
        <button
          class="stage-next-line"
          onclick={() => seek(nextLine.startMs)}
          title={nextLine.text}
          aria-label={`${t("common.upNext")}: ${nextLine.text}`}
        >
          <span class="next-text">{nextLine.text}</span>
        </button>
      {/if}
    </footer>
  {/if}
</div>

<style>
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  .stage-container {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    align-items: center;
    padding: 0 48px 12px 48px;
    box-sizing: border-box;
    user-select: none;
    overflow: hidden;
    background: transparent;
    outline: none;
  }

  .stage-main-area {
    position: relative;
    flex: 1;
    width: 100%;
    max-width: 1440px;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
  }

  /* Each line row covers the stage center with organic alignment and lift */
  .stage-line-row {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    align-content: center;
    flex-wrap: wrap;
    padding: 0 10vw;
    box-sizing: border-box;
    pointer-events: none;
    font-family: var(--font);
    font-size: var(--stage-f, 48px);
    line-height: 1.35;
    font-weight: 800;
    letter-spacing: -0.015em;
    text-shadow: none;
    transition: opacity 0.15s linear;
  }

  .stage-line-row.prev-fading {
    z-index: 1;
  }

  .stage-line-row.cur-active {
    z-index: 2;
  }

  /* Individual word box */
  .stage-word-box {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    will-change: transform, opacity;
    transform-origin: center center;
    white-space: nowrap;
  }

  .word-body {
    position: relative;
    z-index: 2;
  }

  /* Overlaid glowing blur text */
  .word-glow {
    position: absolute;
    inset: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    filter: blur(calc(var(--stage-f, 48px) * 0.2));
    pointer-events: none;
    user-select: none;
  }

  /* Interlude 3 breathing dots */
  .interlude-container {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .interlude-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 18px;
  }

  .interlude-dot {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.25);
    box-shadow: none;
    will-change: transform, background;
    transition: background 0.1s linear;
  }

  .interlude-dot.lit {
    background: #fff;
    box-shadow: none;
  }

  /* Foot row along the bottom */
  .stage-foot {
    flex: none;
    width: 100%;
    max-width: 800px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    padding-bottom: 4px;
    z-index: 10;
  }

  .stage-next-line {
    display: inline-flex;
    align-items: center;
    background: transparent;
    border: none;
    color: rgba(255, 255, 255, 0.35);
    font-family: var(--font);
    font-size: 14.5px;
    font-weight: 500;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 85%;
    padding: 4px 14px;
    border-radius: 999px;
    box-shadow: none;
    transition: color 0.2s ease, background 0.2s ease;
  }

  .stage-next-line:hover {
    color: rgba(255, 255, 255, 0.88);
    background: rgba(255, 255, 255, 0.08);
    box-shadow: none;
    transform: none;
  }

  .stage-next-line:focus-visible {
    outline: 2px solid rgba(255, 255, 255, 0.7);
    outline-offset: 2px;
  }

  .next-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* States */
  .stage-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    font-size: 18px;
    font-weight: 600;
  }

  .stage-state .dim {
    color: rgba(255, 255, 255, 0.45);
  }

  .inst-label {
    font-size: 15px;
    color: rgba(255, 255, 255, 0.6);
  }

  .stage-state .link {
    background: none;
    border: none;
    color: inherit;
    font-size: 13px;
    text-decoration: underline;
    cursor: pointer;
  }

  @media (prefers-reduced-motion: reduce) {
    .stage-word-box {
      transform: none !important;
    }
    .interlude-dot {
      transform: none !important;
    }
  }
</style>