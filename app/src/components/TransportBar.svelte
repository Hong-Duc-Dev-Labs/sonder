<script lang="ts">
  // The floating glass player: disc, title, a compact signal-path chip (full path in its
  // popover), controls, seek bar, then lyrics, queue and (only while software volume is on)
  // volume. Narrow (under ~860 px of its own width) the seek bar becomes a thin line along the
  // pill's bottom edge and the volume slider moves into a popover. It slides away on Now Playing, which
  // has its own controls.
  import { app } from "../lib/app.svelte";
  import { commands, type RepeatMode } from "../lib/bindings";
  import { playbackMs } from "../lib/clock.svelte";
  import { duration, isBitPerfect, outputLabel, sourceLabel } from "../lib/format";
  import { onFrame } from "../lib/motion";
  import FormatBadges from "./FormatBadges.svelte";
  import Icon from "./Icon.svelte";
  import MiniDisc from "./MiniDisc.svelte";
  import PlayPauseIcon from "./PlayPauseIcon.svelte";
  import Popover from "./Popover.svelte";
  import { keySeeker } from "../lib/seek";
  import { rangeFill } from "../lib/rangeFill";

  let { hidden = false }: { hidden?: boolean } = $props();

  const RING_R = 27;
  const RING_C = 2 * Math.PI * RING_R;

  let item = $derived(app.current);
  let snap = $derived(app.player);
  let playing = $derived(snap.state === "playing");
  let total = $derived(snap.durationMs ?? item?.durationMs ?? 0);
  let dragging = $state<number | null>(null);
  let shownMs = $derived(dragging ?? snap.elapsedMs);
  let perfect = $derived(isBitPerfect(snap));

  let statusOpen = $state(false);
  let statusChip: HTMLButtonElement | undefined = $state();
  let track: HTMLDivElement | undefined = $state();
  let fill: HTMLDivElement | undefined = $state();
  let knob: HTMLDivElement | undefined = $state();
  let ring: SVGCircleElement | undefined = $state();

  // Progress painted through transforms, written only when it moves by a whole pixel, so the
  // glass is not re-rendered every frame for a bar that creeps a few pixels a second.
  let lastPx = -1;
  let lastRing = -1;
  function paint(ms: number) {
    const f = total ? Math.min(1, Math.max(0, ms / total)) : 0;
    const width = track?.clientWidth ?? 0;
    const px = Math.round(f * width);
    if (px !== lastPx && fill && knob) {
      lastPx = px;
      fill.style.transform = `scaleX(${f.toFixed(4)})`;
      knob.style.transform = `translateX(${px}px)`;
    }
    const dash = Math.round((1 - f) * RING_C * 2) / 2;
    if (dash !== lastRing && ring) {
      lastRing = dash;
      ring.style.strokeDashoffset = String(dash);
    }
  }

  $effect(() => {
    const ms = shownMs;
    void total;
    lastPx = -1;
    lastRing = -1;
    if (dragging != null || !playing) {
      paint(ms);
      return;
    }
    return onFrame(() => paint(playbackMs()));
  });

  const seeker = keySeeker(() => total);

  // Repeat cycles off → all → one; the engine owns the mode, the snapshot reports it.
  let repeat = $derived<RepeatMode>(snap.repeat ?? "off");
  const repeatOrder: Record<RepeatMode, RepeatMode> = { off: "all", all: "one", one: "off" };
  const repeatLabels: Record<RepeatMode, string> = {
    off: "Repeat off",
    all: "Repeat all",
    one: "Repeat one",
  };
  function cycleRepeat() {
    app.run(commands.playerSetRepeat(repeatOrder[repeat]));
  }

  // Software volume, only while it is on: with it off the output is bit-perfect and the status
  // chip says so. The pill never turns it on; that is a choice made in Settings.
  const VOL_MIN = -60;
  let volumeDb = $derived(snap.volumeDb);
  let volumeDraft = $state(0);
  let volumeOpen = $state(false);
  let volumeButton: HTMLButtonElement | undefined = $state();
  $effect(() => {
    if (volumeDb != null) volumeDraft = volumeDb;
    else volumeOpen = false;
  });
  function commitVolume() {
    app.run(commands.playerSetVolume(volumeDraft));
  }

  function commitSeek() {
    if (dragging != null) app.run(commands.playerSeek(Math.round(dragging)));
    dragging = null;
  }

  // One compact chip for the signal path: the source format, a dot for its state (green when
  // bit-perfect, amber with software volume) and the full path in a popover.
  let chipText = $derived.by(() => {
    const src = snap.source;
    if (!src) return null;
    const khz = src.rate / 1000;
    return `${src.bits}/${Number.isInteger(khz) ? khz : khz.toFixed(1)}`;
  });
  let chipState = $derived(perfect ? "ok" : snap.volumeDb != null && snap.output ? "warn" : "plain");
  let chipLabel = $derived.by(() => {
    if (!snap.source) return "Signal path";
    const state = perfect ? "bit-perfect" : snap.volumeDb != null ? `software volume ${snap.volumeDb.toFixed(1)} dB` : "";
    return `Signal path: ${sourceLabel(snap.source)}${state ? `, ${state}` : ""}`;
  });
</script>

<footer class="dock" class:hidden inert={hidden} aria-label="Player">
  <div class="pill glass">
    <div class="now">
      {#if item}
        <button class="disc" aria-label="Open Now Playing" onclick={() => app.go({ kind: "now" })}>
          <svg class="ring" viewBox="0 0 60 60" aria-hidden="true">
            <circle class="ring-track" cx="30" cy="30" r={RING_R} />
            <circle
              bind:this={ring}
              class="ring-fill"
              cx="30"
              cy="30"
              r={RING_R}
              stroke-dasharray={RING_C}
              stroke-dashoffset={RING_C}
            />
          </svg>
          <span class="disc-inner" data-flight="pill" class:landing={app.flight?.target === "pill" && !app.flight.landed}><MiniDisc art={item.art} paused={snap.state === "paused"} /></span>
        </button>
        <div class="text">
          <span class="t" title={item.title}>{item.title}</span>
          <span class="a" title={item.artist ?? ""}>{item.artist ?? ""}</span>
        </div>
        {#if chipText}
          <button
            class="status {chipState}"
            bind:this={statusChip}
            aria-label={chipLabel}
            title={chipLabel}
            aria-haspopup="dialog"
            aria-expanded={statusOpen}
            onclick={() => (statusOpen = !statusOpen)}
          >
            <span class="dot" aria-hidden="true"></span>
            {chipText}
          </button>
        {/if}
      {:else}
        <span class="idle">Nothing playing</span>
      {/if}
    </div>

    <div class="controls">
      <button class="ctl" aria-label="Previous" onclick={() => app.run(commands.playerPrevious())}>
        <Icon name="prev" size={20} />
      </button>
      <button class="ctl main" aria-label={playing ? "Pause" : "Play"} onclick={() => app.run(commands.playerToggle())}>
        <PlayPauseIcon {playing} size={22} />
      </button>
      <button class="ctl" aria-label="Next" onclick={() => app.run(commands.playerNext())}>
        <Icon name="next" size={20} />
      </button>
      <button
        class="ctl"
        class:on={repeat !== "off"}
        aria-label={repeatLabels[repeat]}
        title={repeatLabels[repeat]}
        aria-pressed={repeat !== "off"}
        onclick={cycleRepeat}
      >
        <Icon name={repeat === "one" ? "repeat-one" : "repeat"} size={20} />
      </button>
    </div>

    <div class="seek" class:disabled={!item || !total}>
      <span class="time">{duration(shownMs)}</span>
      <div class="bar">
        <div class="track" bind:this={track}>
          <div class="fill" bind:this={fill}></div>
        </div>
        <div class="knob" bind:this={knob}></div>
        <input
          type="range"
          min="0"
          max={Math.max(total, 1)}
          step="1000"
          value={shownMs}
          disabled={!item || !total}
          aria-label="Seek"
          aria-valuetext="{duration(shownMs)} of {duration(total)}"
          oninput={(e) => (dragging = Number(e.currentTarget.value))}
          onchange={commitSeek}
          onkeydown={(e) => seeker.onKey(e)}
        />
      </div>
      <span class="time end">{duration(total || null)}</span>
      <span class="time remain">-{duration(Math.max(0, total - shownMs))}</span>
    </div>

    <div class="right">
      <button
        data-lyrics-toggle
        class="icon-btn"
        class:on={app.lyricsOpen}
        aria-label="Lyrics"
        title="Lyrics"
        aria-pressed={app.lyricsOpen}
        onclick={() => app.toggleLyrics()}
      >
        <Icon name="lyrics" />
      </button>
      <button
        data-queue-toggle
        class="icon-btn"
        class:on={app.queueOpen}
        aria-label="Queue"
        title="Queue"
        aria-expanded={app.queueOpen}
        onclick={() => app.toggleQueue()}
      >
        <Icon name="queue" />
      </button>
      {#if volumeDb != null}
        <input
          class="range vol-inline"
          type="range"
          min={VOL_MIN}
          max="0"
          step="0.5"
          aria-label="Software volume"
          aria-valuetext="{volumeDraft.toFixed(1)} dB"
          title="{volumeDraft.toFixed(1)} dB"
          bind:value={volumeDraft}
          use:rangeFill={volumeDraft}
          onchange={commitVolume}
        />
        <button
          class="icon-btn vol-btn"
          bind:this={volumeButton}
          aria-label="Software volume {volumeDraft.toFixed(1)} dB"
          aria-haspopup="dialog"
          aria-expanded={volumeOpen}
          onclick={() => (volumeOpen = !volumeOpen)}
        >
          <Icon name="volume" />
        </button>
      {/if}
    </div>
  </div>
</footer>

<Popover anchor={volumeButton} bind:open={volumeOpen} placement="top" label="Software volume">
  <div class="vol-pop">
    <span class="kicker">Software volume</span>
    <input
      class="range"
      type="range"
      min={VOL_MIN}
      max="0"
      step="0.5"
      aria-label="Software volume"
      aria-valuetext="{volumeDraft.toFixed(1)} dB"
      bind:value={volumeDraft}
      use:rangeFill={volumeDraft}
      onchange={commitVolume}
    />
    <span class="device">{volumeDraft.toFixed(1)} dB</span>
  </div>
</Popover>

<Popover anchor={statusChip} bind:open={statusOpen} placement="top" label="Signal path">
  <div class="path">
    <span class="kicker">Signal path</span>
    <FormatBadges />
    {#if snap.output}
      <span class="device">Device: {outputLabel(snap.output)}</span>
    {/if}
  </div>
</Popover>

<style>
  /* The dock only places the pill. It must not carry a view-transition name, opacity under 1,
     a filter or a mask: any of them makes it a backdrop root, and the pill's backdrop blur
     would then see nothing but the empty dock, leaving the page sharp behind the glass. Those
     belong on the pill itself. */
  .dock {
    position: absolute;
    /* inside the page sheet's bottom edge (App.svelte), one gutter in from each side, so it
       narrows with the sheet when the inspector docks */
    left: calc(var(--sidebar) + var(--gutter) * 3);
    right: calc(max(var(--gutter), var(--inset-right)) + var(--gutter));
    bottom: calc(var(--gutter) * 2);
    display: flex;
    justify-content: center;
    z-index: 20;
    pointer-events: none;
    transition: transform var(--dur-slow) var(--ease-out);
  }
  .dock.hidden {
    transform: translateY(calc(100% + var(--gutter) * 2));
  }
  .dock.hidden .pill {
    opacity: 0;
  }
  .pill {
    /* its own layer during a cover morph: above the moving page, not cross-faded */
    view-transition-name: player;
    transition: opacity var(--dur-med) var(--ease-standard);
    pointer-events: auto;
    container-type: inline-size;
    width: min(100%, 1080px);
    height: var(--dock-h);
    border-radius: var(--r-pill);
    display: grid;
    grid-template-columns: minmax(0, 1.15fr) auto minmax(0, 1.2fr) auto;
    align-items: center;
    gap: 18px;
    padding: 0 14px 0 8px;
  }

  .now {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }
  .disc {
    position: relative;
    flex: none;
    width: 60px;
    height: 60px;
    display: grid;
    place-items: center;
    border-radius: 50%;
  }
  /* Hidden while the record flies in from Now Playing; shown at once when it lands. */
  .disc-inner.landing {
    opacity: 0;
  }
  .ring {
    position: absolute;
    inset: 0;
    transform: rotate(-90deg);
  }
  .ring-track {
    fill: none;
    stroke: rgb(255 255 255 / 0.14);
    stroke-width: 2.5;
  }
  .ring-fill {
    fill: none;
    stroke: var(--ink);
    stroke-width: 2.5;
    stroke-linecap: round;
  }
  .disc-inner {
    width: 50px;
  }
  .text {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: 2px;
  }
  .t,
  .a {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .t {
    font-weight: 650;
  }
  .a {
    font-size: var(--fs-small);
    color: var(--ink-2);
  }
  .idle {
    padding-left: 12px;
    color: var(--ink-2);
  }
  /* The signal-path chip: format plus a state dot, never wider than it needs to be. */
  .status {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 10px 0 8px;
    border-radius: var(--r-pill);
    background: var(--soft);
    box-shadow: inset 0 0 0 1px var(--hairline);
    font-family: var(--mono);
    font-size: 11px;
    font-weight: 650;
    color: var(--ink-2);
    white-space: nowrap;
    transition:
      background var(--dur-fast) var(--ease-standard),
      color var(--dur-fast) var(--ease-standard);
  }
  .status:hover,
  .status[aria-expanded="true"] {
    background: var(--soft-hover);
    color: var(--ink);
  }
  .status .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--ink-3);
  }
  .status.ok .dot {
    background: var(--ok);
    box-shadow: 0 0 8px var(--ok);
  }
  .status.warn .dot {
    background: var(--warn);
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .ctl {
    width: 40px;
    height: 40px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--ink);
    transition:
      background var(--dur-fast) var(--ease-standard),
      transform var(--dur-med) var(--ease-spring);
  }
  .ctl:hover {
    background: var(--soft-hover);
  }
  .ctl.on {
    color: var(--ok);
  }
  .ctl:active {
    transform: scale(0.88);
  }
  .ctl.main {
    width: 48px;
    height: 48px;
    background: var(--ink);
    color: #050506;
  }
  .ctl.main:hover {
    background: #fff;
    transform: scale(1.05);
  }
  .ctl.main:active {
    transform: scale(0.92);
  }

  .seek {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .seek.disabled {
    opacity: 0.4;
  }
  .time {
    flex: none;
    width: 40px;
    font-size: var(--fs-label);
    color: var(--ink-2);
    font-variant-numeric: tabular-nums;
    text-align: center;
  }
  .remain {
    display: none;
  }
  .bar {
    position: relative;
    flex: 1;
    height: 20px;
    display: flex;
    align-items: center;
  }
  .track {
    position: relative;
    width: 100%;
    height: 4px;
    border-radius: var(--r-pill);
    background: rgb(255 255 255 / 0.3);
    overflow: hidden;
  }
  .fill {
    position: absolute;
    inset: 0;
    background: var(--ink);
    transform-origin: left center;
    transform: scaleX(0);
  }
  .knob {
    position: absolute;
    left: -6px;
    top: 4px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--ink);
    box-shadow: 0 1px 4px rgb(0 0 0 / 0.5);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-standard);
    pointer-events: none;
  }
  .bar:hover .knob,
  .bar:focus-within .knob {
    opacity: 1;
  }
  :global(:root:not([data-modality="pointer"])) .bar:focus-within {
    border-radius: var(--r-pill);
    outline: 2px solid var(--ink);
    outline-offset: 2px;
  }
  .bar input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    padding: 0;
    opacity: 0;
    cursor: pointer;
    background: none;
    box-shadow: none;
  }
  .bar input:disabled {
    cursor: default;
  }

  .right {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .vol-inline {
    width: 96px;
    margin-left: 6px;
  }
  .vol-btn {
    display: none;
  }
  .vol-pop {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px;
    min-width: 200px;
  }

  .path {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px;
  }
  .device {
    font-family: var(--mono);
    font-size: var(--fs-label);
    color: var(--ink-2);
  }

  /* Narrow pill: the seek bar becomes a line on the bottom edge. */
  @container (max-width: 860px) {
    .pill {
      grid-template-columns: minmax(0, 1fr) auto auto auto;
      gap: 10px;
    }
    .seek {
      display: contents;
    }
    .seek .time:not(.remain) {
      display: none;
    }
    .remain {
      display: block;
    }
    .bar {
      position: absolute;
      left: 36px;
      right: 36px;
      bottom: 0;
      height: 14px;
    }
    .track {
      height: 3px;
    }
    .knob {
      display: none;
    }
    .vol-inline {
      display: none;
    }
    .vol-btn {
      display: grid;
    }
  }
</style>
