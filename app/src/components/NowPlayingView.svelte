<script lang="ts">
  // Now Playing: the turntable (or the square cover) beside its own large controls, over the
  // cover's ambient light at full strength. Sized by the pane, not the window, so the sidebar
  // and narrow windows are accounted for; below 760 px of pane the record stacks above the
  // controls and melts into them through a frost band.
  import { app } from "../lib/app.svelte";
  import { commands, type CoverStyle } from "../lib/bindings";
  import { outputLabel, plural } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { slideRecord } from "../lib/transitions";
  import Cover from "./Cover.svelte";
  import FormatBadges from "./FormatBadges.svelte";
  import FrostFade from "./FrostFade.svelte";
  import LyricStage from "./LyricStage.svelte";
  import LyricsPanel from "./LyricsPanel.svelte";
  import Icon from "./Icon.svelte";
  import PlayPauseIcon from "./PlayPauseIcon.svelte";
  import SeekBar from "./SeekBar.svelte";
  import SignalPathPanel from "./SignalPathPanel.svelte";
  import TurntableStage from "./TurntableStage.svelte";
  import { rise, fadeQuick, pageIn } from "../lib/transitions";

  let item = $derived(app.current);
  let path = $derived(app.player.signalPath);
  let output = $derived(app.player.output);
  let coverStyle = $derived(app.settings.coverStyle ?? "vinyl");
  let playing = $derived(app.player.state === "playing");
  let trackKey = $derived(item ? `entry-${item.entryId}` : "none");
  /** Bit depth and rate of what is playing, as a small chip after the title ("24/96"). */
  let codec = $derived.by(() => {
    const src = app.player.source;
    if (!src || !item) return null;
    const khz = src.rate / 1000;
    return `${src.bits}/${Number.isInteger(khz) ? khz : khz.toFixed(1)}`;
  });
  let bitPerfect = $derived(app.player.volumeDb == null);

  $effect(() => {
    app.pageWash = null;
  });

  function nextStyle(): CoverStyle {
    if (coverStyle === "vinyl") return "square";
    if (coverStyle === "square") return "stage";
    return "vinyl";
  }

  function toggleStyle() {
    app.saveSettings({ ...app.settings, coverStyle: nextStyle() });
  }

  let styleToggleLabel = $derived.by(() => {
    const next = nextStyle();
    if (next === "square") return t("np.showSquare");
    if (next === "stage") return t("np.showStage");
    return t("np.showVinyl");
  });

  let styleToggleIcon = $derived.by(() => {
    const next = nextStyle();
    if (next === "square") return "albums" as const;
    if (next === "stage") return "spectrum" as const;
    return "disc" as const;
  });
</script>

<div class="np" class:stage-mode={coverStyle === "stage"}>
  {#if coverStyle === "stage"}
    <div class="stage-fullscreen">
      <header class="np-head stage-head">
        <span class="h">{t("common.nowPlaying")}</span>
        {#if item}<span class="dim">{item.album}</span>{/if}
      </header>

      <div class="stage-body">
        <LyricStage />

        {#if app.lyricsOpen}
          <div class="stage-lyrics-panel" in:pageIn out:fadeQuick>
            <div class="panel-header">
              <span class="h">{t("common.lyrics")}</span>
              <button class="icon-btn close-btn" aria-label={t("lyrics.close")} onclick={() => app.toggleLyrics()}>
                <Icon name="close" size={16} />
              </button>
            </div>
            <LyricsPanel />
          </div>
        {/if}
      </div>

      <div class="stage-dock">
        <div class="dock-meta">
          <div class="dock-art">
            <Cover hash={item?.art} size={300} alt={item?.album ?? ""} />
          </div>
          <div class="dock-text">
            <div class="dock-title-row">
              <span class="dock-title" title={item?.title}>{item?.title ?? t("common.nothingPlaying")}</span>
              {#if codec}<span class="codec">{codec}</span>{/if}
            </div>
            {#if item}
              <button class="dock-sub link" onclick={() => item && app.go({ kind: "album", id: item.albumId })}>
                {item.artist ?? t("common.unknownArtist")} · {item.album}
              </button>
            {:else}
              <span class="dock-sub dim">{t("np.pickAlbum")}</span>
            {/if}
          </div>
        </div>

        <div class="dock-center">
          <div class="dock-transport">
            <button class="glyph sm" aria-label={t("common.previous")} onclick={() => app.run(commands.playerPrevious())}>
              <Icon name="prev" size={26} />
            </button>
            <button class="glyph main sm" aria-label={playing ? t("common.pause") : t("common.play")} onclick={() => app.run(commands.playerToggle())}>
              <PlayPauseIcon {playing} size={38} />
            </button>
            <button class="glyph sm" aria-label={t("common.next")} onclick={() => app.run(commands.playerNext())}>
              <Icon name="next" size={26} />
            </button>
          </div>
          <div class="dock-seek">
            <SeekBar />
          </div>
        </div>

        <div class="dock-tools">
          <button
            class="icon-btn filled"
            class:on={app.queueOpen}
            aria-label={t("common.queue")}
            aria-expanded={app.queueOpen}
            data-queue-toggle
            onclick={() => app.toggleQueue()}
          >
            <Icon name="queue" />
          </button>
          <button
            class="icon-btn filled"
            class:on={app.lyricsOpen}
            aria-label={t("common.lyrics")}
            aria-pressed={app.lyricsOpen}
            onclick={() => app.toggleLyrics()}
          >
            <Icon name="lyrics" />
          </button>
          <button
            class="icon-btn filled"
            class:warn={!bitPerfect}
            aria-label={bitPerfect ? t("np.outputPerfect") : t("np.outputVolume")}
            onclick={() => app.go({ kind: "settings" })}
          >
            <Icon name="volume" />
          </button>
          <button class="icon-btn filled" aria-label={styleToggleLabel} onclick={toggleStyle}>
            <Icon name={styleToggleIcon} />
          </button>
        </div>
      </div>
    </div>
  {:else}
    <header class="np-head">
      <span class="h">{t("common.nowPlaying")}</span>
      {#if item}<span class="dim">{item.album}</span>{/if}
    </header>

    <div class="layout">
      <div class="visual">
        {#if coverStyle === "vinyl"}
          <TurntableStage />
        {:else}
          <div class="square" data-flight="stage" class:landing={app.flight?.target === "stage" && !app.flight.landed}>
            {#key trackKey}
              <div
                class="square-art"
                in:slideRecord={{ dir: app.trackDirection, leaving: false }}
                out:slideRecord={{ dir: app.trackDirection, leaving: true }}
              >
                <Cover hash={item?.art} size={1000} alt={item?.album ?? ""} />
              </div>
            {/key}
          </div>
        {/if}
        <div class="frost-slot"><FrostFade height="34%" /></div>
      </div>

      <div class="side">
        {#if app.lyricsOpen}
          <div class="lyrics-slot" in:pageIn>
            {#if item}
              <p class="mini-meta"><strong>{item.title}</strong> · {item.artist ?? t("common.unknownArtist")}</p>
            {/if}
            <LyricsPanel />
          </div>
        {:else}
        <div class="meta">
          {#key trackKey}
            <div class="meta-inner" in:rise={{ delay: 120 }} out:fadeQuick>
              {#if item}
                <div class="title-row">
                  <h1 tabindex="-1" data-view-heading title={item.title}>{item.title}</h1>
                  {#if codec}<span class="codec">{codec}</span>{/if}
                </div>
                <span class="artist">{item.artist ?? t("common.unknownArtist")}</span>
                <button class="album link" onclick={() => app.go({ kind: "album", id: item.albumId })}>{item.album}</button>
              {:else}
                <h1 tabindex="-1" data-view-heading class="dim">{t("common.nothingPlaying")}</h1>
                <span class="artist dim">{t("np.pickAlbum")}</span>
              {/if}
            </div>
          {/key}
        </div>

        <FormatBadges />
        {/if}

        <SeekBar />

        <div class="transport">
          <button class="glyph" aria-label={t("common.previous")} onclick={() => app.run(commands.playerPrevious())}>
            <Icon name="prev" size={34} />
          </button>
          <button class="glyph main" aria-label={playing ? t("common.pause") : t("common.play")} onclick={() => app.run(commands.playerToggle())}>
            <PlayPauseIcon {playing} size={46} />
          </button>
          <button class="glyph" aria-label={t("common.next")} onclick={() => app.run(commands.playerNext())}>
            <Icon name="next" size={34} />
          </button>
        </div>

        <div class="tools">
          <button
            class="icon-btn filled"
            class:on={app.queueOpen}
            aria-label={t("common.queue")}
            aria-expanded={app.queueOpen}
            data-queue-toggle
            onclick={() => app.toggleQueue()}
          >
            <Icon name="queue" />
          </button>
          <button
            class="icon-btn filled"
            class:on={app.lyricsOpen}
            aria-label={t("common.lyrics")}
            aria-pressed={app.lyricsOpen}
            onclick={() => app.toggleLyrics()}
          >
            <Icon name="lyrics" />
          </button>
          <button class="icon-btn filled" aria-label={t("common.goToAlbum")} disabled={!item} onclick={() => item && app.go({ kind: "album", id: item.albumId })}>
            <Icon name="albums" />
          </button>
          <button
            class="icon-btn filled"
            class:warn={!bitPerfect}
            aria-label={bitPerfect ? t("np.outputPerfect") : t("np.outputVolume")}
            onclick={() => app.go({ kind: "settings" })}
          >
            <Icon name="volume" />
          </button>
          <button class="icon-btn filled" aria-label={styleToggleLabel} onclick={toggleStyle}>
            <Icon name={styleToggleIcon} />
          </button>
        </div>

        {#if output && !app.lyricsOpen}
          <div class="path-row">
            <p class="path">{t("np.deviceOpened", { format: outputLabel(output) })}</p>
            {#if app.player.underruns}
              <span class="underruns" title={t("np.underrunsTitle")}>
                {plural(app.player.underruns, "underrun")}
              </span>
            {/if}
            {#if path}
              <button
                class="path-toggle"
                class:on={app.signalPathOpen}
                aria-expanded={app.signalPathOpen}
                aria-label={t("common.signalPath")}
                title={t("common.signalPath")}
                onclick={() => (app.signalPathOpen = !app.signalPathOpen)}
              >
                <Icon name="signal" size={16} />
              </button>
            {/if}
          </div>
          {#if app.signalPathOpen && path}
            <div class="path-panel" in:fadeQuick>
              <SignalPathPanel {path} />
            </div>
          {/if}
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .np {
    position: relative;
    height: 100%;
    container-type: size;
    display: flex;
    flex-direction: column;
    padding: 18px var(--page-x) 28px calc(var(--inset-left, 0px) + var(--page-x));
  }
  .np.stage-mode {
    padding: 16px 0 0 0;
  }
  .stage-fullscreen {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
  }
  .stage-head {
    flex: none;
    padding-bottom: 6px;
  }
  .stage-body {
    flex: 1;
    min-height: 0;
    position: relative;
    width: 100%;
    display: flex;
  }
  .stage-lyrics-panel {
    position: absolute;
    top: 12px;
    right: 28px;
    bottom: 24px;
    width: min(380px, 85vw);
    background: rgba(14, 14, 18, 0.82);
    backdrop-filter: blur(32px);
    -webkit-backdrop-filter: blur(32px);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: var(--r-lg, 16px);
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.75);
    display: flex;
    flex-direction: column;
    padding: 16px 20px;
    z-index: 10;
  }
  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-bottom: 8px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    margin-bottom: 8px;
  }
  .panel-header .h {
    font-size: 14px;
    font-weight: 700;
  }
  .close-btn {
    width: 28px;
    height: 28px;
    border-radius: 50%;
  }

  .stage-dock {
    flex: none;
    display: grid;
    grid-template-columns: minmax(220px, 340px) minmax(320px, 640px) minmax(180px, auto);
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 14px var(--page-x) 22px calc(var(--inset-left, 0px) + var(--page-x));
    background: linear-gradient(to top, rgba(0, 0, 0, 0.85) 0%, rgba(0, 0, 0, 0.45) 60%, transparent 100%);
    backdrop-filter: blur(20px);
    -webkit-backdrop-filter: blur(20px);
    z-index: 5;
  }

  .dock-meta {
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
  }
  .dock-art {
    width: 48px;
    height: 48px;
    flex: none;
    border-radius: var(--r-sm, 8px);
    overflow: hidden;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.5);
  }
  .dock-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .dock-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .dock-title {
    font-size: 15px;
    font-weight: 700;
    color: var(--ink);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dock-sub {
    font-size: 12.5px;
    color: var(--ink-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: left;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
  }
  .dock-sub:hover {
    color: var(--ink);
    text-decoration: underline;
  }

  .dock-center {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .dock-transport {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .dock-seek {
    width: 100%;
  }
  .dock-tools {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
  }

  .glyph.sm {
    width: 44px;
    height: 44px;
  }
  .glyph.main.sm {
    width: 54px;
    height: 54px;
  }

  @container (max-width: 820px) {
    .stage-dock {
      grid-template-columns: 1fr;
      grid-template-rows: auto auto auto;
      gap: 10px;
      padding: 12px 20px 20px;
    }
    .dock-meta {
      justify-content: center;
      text-align: center;
    }
    .dock-tools {
      justify-content: center;
    }
  }
  .np-head {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    font-size: 13px;
  }
  .np-head .h {
    font-weight: 700;
  }
  .layout {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: auto minmax(300px, 400px);
    /* One row exactly as tall as the pane, so the side column can never push past it. */
    grid-template-rows: minmax(0, 1fr);
    align-items: center;
    justify-content: center;
    gap: 48px;
  }
  .visual {
    position: relative;
    width: min(calc(100cqh - 110px), calc(100cqw - 540px));
  }
  .square {
    position: relative;
    width: 86%;
    margin: 0 auto;
    aspect-ratio: 1;
  }
  .square.landing {
    opacity: 0;
  }
  .square-art {
    position: absolute;
    inset: 0;
  }
  .square-art :global(.cover) {
    border-radius: var(--r-lg);
    box-shadow: 0 30px 70px rgb(0 0 0 / 0.6);
  }
  .frost-slot {
    display: none;
  }

  .side {
    display: flex;
    flex-direction: column;
    gap: 18px;
    min-width: 0;
    max-height: 100%;
  }
  .meta {
    display: grid;
  }
  /* A few lines around the sung one, not the whole song: it shrinks first when space runs out. */
  .lyrics-slot {
    flex: 0 1 auto;
    height: min(44cqh, 420px);
    min-height: 140px;
    display: flex;
    flex-direction: column;
  }
  .mini-meta {
    margin: 0 0 4px 10px;
    font-size: 13px;
    color: var(--ink-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mini-meta strong {
    color: var(--ink);
  }
  /* Old and new titles overlap while they cross-fade: no layout jump. */
  .meta-inner {
    grid-area: 1 / 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 120px;
    justify-content: flex-end;
    min-width: 0;
  }
  .title-row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  h1 {
    font-size: clamp(26px, 2.2cqw + 12px, 40px);
    line-height: 1.08;
    font-weight: 800;
    letter-spacing: -0.03em;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .codec {
    flex: none;
    padding: 2px 8px;
    border-radius: 6px;
    box-shadow: inset 0 0 0 1.5px rgb(255 255 255 / 0.4);
    font-family: var(--font);
    font-size: 11px;
    font-weight: 700;
    color: var(--ink-2);
  }
  .artist {
    font-size: 18px;
    font-weight: 600;
    color: var(--ink);
  }
  .album {
    align-self: flex-start;
    font-size: 14px;
    color: var(--ink-2);
    text-align: left;
    text-underline-offset: 3px;
  }
  .album:hover {
    color: var(--ink);
    text-decoration: underline;
  }

  .transport {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 18px;
  }
  .glyph {
    width: 72px;
    height: 72px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--ink);
    transition:
      background var(--dur-fast) var(--ease-standard),
      transform var(--dur-med) var(--ease-spring);
  }
  .glyph:hover {
    background: var(--soft-hover);
  }
  .glyph:active {
    transform: scale(0.88);
    background: var(--soft-active);
  }
  .glyph.main {
    width: 92px;
    height: 92px;
  }
  .tools {
    display: flex;
    justify-content: center;
    gap: 10px;
  }
  .tools .icon-btn {
    width: 46px;
    height: 46px;
  }
  .tools .icon-btn.warn {
    color: var(--warn);
  }
  .path-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }
  .path {
    margin: 0;
    text-align: center;
    font-size: 11.5px;
    font-family: var(--font);
    color: var(--ink-2);
  }
  .underruns {
    padding: 2px 8px;
    border-radius: var(--r-pill);
    font-size: 11px;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    color: var(--warn);
    background: color-mix(in srgb, var(--warn) 14%, transparent);
  }
  .path-toggle {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: transparent;
    color: var(--ink-2);
  }
  .path-toggle:hover,
  .path-toggle.on {
    background: var(--soft-hover);
    color: var(--ink);
  }
  .path-panel {
    width: 100%;
    max-width: 420px;
    margin: 4px auto 0;
  }

  /* Narrow pane: record above, controls below, melted together. */
  @container (max-width: 760px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto auto;
      justify-items: center;
      align-content: center;
      gap: 0;
    }
    .visual {
      width: min(calc(100cqh - 420px), 90cqw);
      min-width: 220px;
    }
    .frost-slot {
      display: block;
    }
    .side {
      width: min(440px, 100%);
      margin-top: -24px;
      gap: 12px;
      text-align: center;
    }
    .meta-inner {
      min-height: 0;
      align-items: center;
    }
    .album {
      align-self: center;
    }
    .side :global(.badges) {
      justify-content: center;
    }
    .lyrics-slot {
      height: clamp(160px, calc(100cqh - 560px), 300px);
      text-align: left;
    }
  }
</style>
