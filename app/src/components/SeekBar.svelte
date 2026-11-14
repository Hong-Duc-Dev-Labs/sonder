<script lang="ts">
  // Now Playing's thick seek bar with elapsed and total time. The fill follows the clock every
  // frame through a transform, written only when it moves by a whole pixel; a transparent
  // range input on top handles pointer, keyboard and screen readers.
  import { app } from "../lib/app.svelte";
  import { commands } from "../lib/bindings";
  import { playbackMs } from "../lib/clock.svelte";
  import { duration } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { onFrame } from "../lib/motion";
  import { keySeeker } from "../lib/seek";

  let total = $derived(app.player.durationMs ?? app.current?.durationMs ?? 0);
  let playing = $derived(app.player.state === "playing");
  let dragging = $state<number | null>(null);
  let shownMs = $state(0);
  let enabled = $derived(!!app.current && total > 0);

  let track: HTMLDivElement | undefined = $state();
  let fill: HTMLDivElement | undefined = $state();
  let knob: HTMLDivElement | undefined = $state();
  const seeker = keySeeker(() => total);

  let lastPx = -1;
  let lastSecond = -1;
  function paint(ms: number) {
    const f = total ? Math.min(1, Math.max(0, ms / total)) : 0;
    const px = Math.round(f * (track?.clientWidth ?? 0));
    if (px !== lastPx && fill && knob) {
      lastPx = px;
      fill.style.transform = `scaleX(${f.toFixed(4)})`;
      knob.style.transform = `translateX(${px}px)`;
    }
    const second = Math.floor(ms / 1000);
    if (second !== lastSecond) {
      lastSecond = second;
      shownMs = ms;
    }
  }

  $effect(() => {
    const ms = dragging ?? app.player.elapsedMs;
    void total;
    lastPx = -1;
    lastSecond = -1;
    if (dragging != null || !playing) {
      paint(ms);
      return;
    }
    return onFrame(() => paint(playbackMs()));
  });

  function commit() {
    if (dragging != null) app.run(commands.playerSeek(Math.round(dragging)));
    dragging = null;
  }
</script>

<div class="seek" class:disabled={!enabled}>
  <div class="bar">
    <div class="track" bind:this={track}><div class="fill" bind:this={fill}></div></div>
    <div class="knob" bind:this={knob}></div>
    <input
      type="range"
      min="0"
      max={Math.max(total, 1)}
      step="1000"
      value={shownMs}
      disabled={!enabled}
      aria-label={t("common.seek")}
      aria-valuetext={t("common.seekValue", { elapsed: duration(shownMs), total: duration(total) })}
      oninput={(e) => (dragging = Number(e.currentTarget.value))}
      onchange={commit}
      onkeydown={(e) => seeker.onKey(e)}
    />
  </div>
  <div class="times">
    <span>{duration(shownMs)}</span>
    <span>{duration(total || null)}</span>
  </div>
</div>

<style>
  .seek.disabled {
    opacity: 0.4;
  }
  .bar {
    position: relative;
    height: 24px;
    display: flex;
    align-items: center;
  }
  .track {
    position: relative;
    width: 100%;
    height: 8px;
    border-radius: var(--r-pill);
    background: rgb(255 255 255 / 0.22);
    overflow: hidden;
    transition: height var(--dur-fast) var(--ease-standard);
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
    left: -8px;
    top: 4px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--ink);
    box-shadow: 0 2px 6px rgb(0 0 0 / 0.5);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-standard);
    pointer-events: none;
  }
  .bar:hover .track,
  .bar:focus-within .track {
    height: 10px;
  }
  .bar:hover .knob,
  .bar:focus-within .knob {
    opacity: 1;
  }
  :global(:root:not([data-modality="pointer"])) .bar:focus-within {
    border-radius: var(--r-pill);
    outline: 2px solid var(--ink);
    outline-offset: 3px;
  }
  input {
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
  input:disabled {
    cursor: default;
  }
  .times {
    display: flex;
    justify-content: space-between;
    margin-top: 6px;
    font-size: 13px;
    color: var(--ink-2);
    font-family: var(--font-figures);
    font-variant-numeric: tabular-nums;
  }
</style>
