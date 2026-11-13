<script lang="ts">
  // Now Playing: the turntable (or the square cover) beside its own large controls, over the
  // cover's ambient light at full strength. Sized by the pane, not the window, so the sidebar
  // and narrow windows are accounted for; below 760 px of pane the record stacks above the
  // controls and melts into them through a frost band.
  import { app } from "../lib/app.svelte";
  import { commands } from "../lib/bindings";
  import { outputLabel, plural } from "../lib/format";
  import { slideRecord } from "../lib/transitions";
  import Cover from "./Cover.svelte";
  import FormatBadges from "./FormatBadges.svelte";
  import FrostFade from "./FrostFade.svelte";
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
  let vinyl = $derived((app.settings.coverStyle ?? "vinyl") === "vinyl");
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

  function toggleStyle() {
    app.saveSettings({ ...app.settings, coverStyle: vinyl ? "square" : "vinyl" });
  }
</script>

<div class="np">
  <header class="np-head">
    <span class="h">Now Playing</span>
    {#if item}<span class="dim">{item.album}</span>{/if}
  </header>

  <div class="layout">
    <div class="visual">
      {#if vinyl}
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
            <p class="mini-meta"><strong>{item.title}</strong> · {item.artist ?? "Unknown artist"}</p>
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
              <span class="artist">{item.artist ?? "Unknown artist"}</span>
              <button class="album link" onclick={() => app.go({ kind: "album", id: item.albumId })}>{item.album}</button>
            {:else}
              <h1 tabindex="-1" data-view-heading class="dim">Nothing playing</h1>
              <span class="artist dim">Pick an album to start.</span>
            {/if}
          </div>
        {/key}
      </div>

      <FormatBadges />
      {/if}

      <SeekBar />

      <div class="transport">
        <button class="glyph" aria-label="Previous" onclick={() => app.run(commands.playerPrevious())}>
          <Icon name="prev" size={34} />
        </button>
        <button class="glyph main" aria-label={playing ? "Pause" : "Play"} onclick={() => app.run(commands.playerToggle())}>
          <PlayPauseIcon {playing} size={46} />
        </button>
        <button class="glyph" aria-label="Next" onclick={() => app.run(commands.playerNext())}>
          <Icon name="next" size={34} />
        </button>
      </div>

      <div class="tools">
        <button
          class="icon-btn filled"
          class:on={app.queueOpen}
          aria-label="Queue"
          aria-expanded={app.queueOpen}
          data-queue-toggle
          onclick={() => app.toggleQueue()}
        >
          <Icon name="queue" />
        </button>
        <button
          class="icon-btn filled"
          class:on={app.lyricsOpen}
          aria-label="Lyrics"
          aria-pressed={app.lyricsOpen}
          onclick={() => app.toggleLyrics()}
        >
          <Icon name="lyrics" />
        </button>
        <button class="icon-btn filled" aria-label="Go to album" disabled={!item} onclick={() => item && app.go({ kind: "album", id: item.albumId })}>
          <Icon name="albums" />
        </button>
        <button
          class="icon-btn filled"
          class:warn={!bitPerfect}
          aria-label={bitPerfect ? "Bit-perfect output: open output settings" : "Software volume on: open output settings"}
          onclick={() => app.go({ kind: "settings" })}
        >
          <Icon name="volume" />
        </button>
        <button class="icon-btn filled" aria-label={vinyl ? "Show the square cover" : "Show the vinyl record"} onclick={toggleStyle}>
          <Icon name={vinyl ? "albums" : "disc"} />
        </button>
      </div>

      {#if output && !app.lyricsOpen}
        <div class="path-row">
          <p class="path">
            Device opened at {outputLabel(output)}{app.player.underruns ? ` · ${plural(app.player.underruns, "underrun")}` : ""}
          </p>
          {#if path}
            <button
              class="path-toggle"
              class:on={app.signalPathOpen}
              aria-expanded={app.signalPathOpen}
              aria-label="Signal path"
              title="Signal path"
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
</div>

<style>
  .np {
    position: relative;
    height: 100%;
    container-type: size;
    display: flex;
    flex-direction: column;
    padding: 18px calc(var(--inset-right, 0px) + var(--page-x)) 28px calc(var(--inset-left, 0px) + var(--page-x));
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
    font-family: var(--mono);
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
    font-family: var(--mono);
    color: var(--ink-2);
  }
  .path-toggle {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: transparent;
    color: var(--ink-3);
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
