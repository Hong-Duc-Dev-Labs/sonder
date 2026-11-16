<script lang="ts">
  // Now Playing's seek bar with elapsed and total time. The fill follows the clock every
  // frame through a transform, written only when it moves by a whole pixel; a transparent
  // range input on top handles pointer, keyboard and screen readers.
  // Supports an `inline` mode for compact docks (horizontal elapsed - bar - remaining).
  import { app } from "../lib/app.svelte";
  import { commands } from "../lib/bindings";
  import { playbackMs } from "../lib/clock.svelte";
  import { duration } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { onFrame } from "../lib/motion";
  import { keySeeker } from "../lib/seek";

  let { inline = false }: { inline?: boolean } = $props();

  let total = $derived(app.player.durationMs ?? app.current?.durationMs ?? 0);
  let playing = $derived(app.player.state === "playing");
  let dragging = $state<number | null>(null);
  let shownMs = $state(0);
  let enabled = $derived(!!app.current && total > 0);

  let track: HTMLDivElement | undefined = $state();
  let fill: HTMLDivElement | undefined = $state();
  let knob: HTMLDivElement | undefined = $state();
  const seeker = keySeeker(() => total);

  let hoverMs = $state<number | null>(null);
  let hoverPx = $state(0);

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

  function onPointerMove(e: MouseEvent) {
    if (!track || !enabled || !total) return;
    const rect = track.getBoundingClientRect();
    const ratio = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    hoverPx = Math.round(ratio * rect.width);
    hoverMs = Math.round(ratio * total);
  }

  function onPointerLeave() {
    hoverMs = null;
  }

  function commit() {
    if (dragging != null) app.run(commands.playerSeek(Math.round(dragging)));
    dragging = null;
  }
</script>

<div
  class="seek"
  class:inline
  class:disabled={!enabled}
  class:is-dragging={dragging != null}
>
  {#if inline}
    <span class="time elapsed">{duration(shownMs)}</span>
  {/if}

  <div
    class="bar"
    onmousemove={onPointerMove}
    onmouseleave={onPointerLeave}
    role="presentation"
  >
    <div class="track" bind:this={track}><div class="fill" bind:this={fill}></div></div>
    <div class="knob" class:visible={dragging != null} bind:this={knob}></div>
    {#if hoverMs != null && !dragging}
      <div class="hover-tooltip" style:left="{hoverPx}px">
        {duration(hoverMs)}
      </div>
    {/if}
    <input
      type="range"
      min="0"
      max={Math.max(total, 1)}
      step="any"
      value={shownMs}
      disabled={!enabled}
      aria-label={t("common.seek")}
      aria-valuetext={t("common.seekValue", { elapsed: duration(shownMs), total: duration(total) })}
      oninput={(e) => (dragging = Number(e.currentTarget.value))}
      onchange={commit}
      onkeydown={(e) => seeker.onKey(e)}
    />
  </div>

  {#if inline}
    <span class="time remaining">-{duration(Math.max(0, total - shownMs))}</span>
  {:else}
    <div class="times">
      <span>{duration(shownMs)}</span>
      <span>{duration(total || null)}</span>
    </div>
  {/if}
</div>

<style>
  .seek {
    position: relative;
    width: 100%;
  }

  .seek.disabled {
    opacity: 0.4;
  }

  .seek.inline {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .seek.inline .bar {
    flex: 1;
    height: 18px;
  }

  .seek.inline .track {
    height: 6px;
  }

  .seek.inline .time {
    font-size: 11.5px;
    font-weight: 500;
    color: rgba(255, 255, 255, 0.72);
    font-family: var(--font-figures);
    font-variant-numeric: tabular-nums;
    min-width: 38px;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.8);
  }

  .seek.inline .time.elapsed {
    text-align: right;
  }

  .seek.inline .time.remaining {
    text-align: left;
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
    border-radius: var(--r-pill, 999px);
    background: rgb(255 255 255 / 0.22);
    overflow: hidden;
    transition: height var(--dur-fast, 150ms) var(--ease-standard, ease);
  }

  .fill {
    position: absolute;
    inset: 0;
    background: var(--ink, #fff);
    transform-origin: left center;
    transform: scaleX(0);
  }

  .knob {
    position: absolute;
    left: -8px;
    top: 50%;
    margin-top: -8px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--ink, #fff);
    box-shadow: 0 2px 8px rgb(0 0 0 / 0.6);
    opacity: 0;
    transition: opacity var(--dur-fast, 150ms) var(--ease-standard, ease), transform 0.05s linear;
    pointer-events: none;
  }

  .knob.visible,
  .bar:hover .knob,
  .bar:focus-within .knob {
    opacity: 1;
  }

  .bar:hover .track,
  .bar:focus-within .track,
  .is-dragging .track {
    height: 10px;
  }

  .hover-tooltip {
    position: absolute;
    bottom: 24px;
    transform: translateX(-50%);
    background: rgba(18, 18, 24, 0.92);
    backdrop-filter: blur(8px);
    border: 1px solid rgba(255, 255, 255, 0.16);
    color: #fff;
    font-size: 11px;
    font-weight: 600;
    padding: 2px 7px;
    border-radius: 5px;
    pointer-events: none;
    white-space: nowrap;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.5);
    font-family: var(--font-figures);
    font-variant-numeric: tabular-nums;
    z-index: 20;
  }

  :global(:root:not([data-modality="pointer"])) .bar:focus-within {
    border-radius: var(--r-pill, 999px);
    outline: 2px solid var(--ink, #fff);
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
    color: var(--ink-2, #b8b8c0);
    font-family: var(--font-figures);
    font-variant-numeric: tabular-nums;
  }
</style>