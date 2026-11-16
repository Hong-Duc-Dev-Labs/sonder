<script lang="ts" module>
  let nextId = 0;
</script>

<script lang="ts">
  // A vinyl record with the album cover as its label. The record's surface never turns: its
  // grooves are circles and its sheen comes from a fixed light up and to the left, so it is
  // painted once. Only the label layer rotates, as a single composited transform per frame.
  //
  // Zones follow the tonearm (see turntable.ts): a smooth lead-in by the rim where the stylus
  // drops, the grooved programme area it travels across, a bright run-out line where it ends,
  // then the glossy dead wax around the label.
  import { onMount } from "svelte";
  import { artUrl } from "../lib/art";
  import { attachSpin } from "../lib/spin.svelte";
  import { drawVinylSurface } from "../lib/vinylSurface";

  let {
    art,
    title = "",
    paused = false,
  }: { art: string | null | undefined; title?: string; paused?: boolean } = $props();

  const pathId = `vinyl-ring-${nextId++}`;
  /**
   * The dead wax runs from the label's edge (radius 220 in the 1000-unit box) to the run-out
   * line (51 % of 500 = 255). The title's centre line sits midway, and the text is set with a
   * central baseline so its glyphs straddle that line instead of standing on it.
   */
  const LABEL_R = 220;
  const RUN_OUT_R = 255;
  const RING_R = (LABEL_R + RUN_OUT_R) / 2;
  const RING_LENGTH = 2 * Math.PI * RING_R;

  let spinner: HTMLDivElement;
  let failed = $state(false);
  let src = $derived(artUrl(art, 1000));
  $effect(() => {
    void src;
    failed = false;
  });

  // Repeat the title round the ring; `textLength` then spaces it to close the circle exactly.
  let ringText = $derived.by(() => {
    const unit = title.trim().toUpperCase();
    if (!unit) return "";
    const piece = `${unit}  ·  `;
    const approxWidth = piece.length * 14;
    const repeats = Math.max(1, Math.floor(RING_LENGTH / approxWidth));
    return piece.repeat(repeats);
  });

  // The still surface is painted into a canvas at device resolution: once per album, and again
  // only when the record is resized.
  let surface: HTMLCanvasElement;
  let seedText = $derived(title);
  $effect(() => {
    void seedText;
    if (surface) drawVinylSurface(surface, seedText);
  });

  onMount(() => {
    const stopSpin = attachSpin(spinner);
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => drawVinylSurface(surface, seedText));
    });
    observer.observe(surface);
    return () => {
      stopSpin();
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  });
</script>

<div class="disc" class:paused>
  <div class="shadow"></div>
  <canvas class="surface" bind:this={surface} aria-hidden="true"></canvas>
  <div class="spinner" bind:this={spinner}>
    <div class="label">
      {#if src && !failed}
        <img {src} alt="" draggable="false" decoding="async" onerror={() => (failed = true)} />
      {:else}
        <div class="placeholder"></div>
      {/if}
    </div>
    {#if ringText}
      <svg class="ring" viewBox="0 0 1000 1000" aria-hidden="true">
        <defs>
          <path id={pathId} d="M 500 {500 - RING_R} a {RING_R} {RING_R} 0 1 1 -0.01 0" />
        </defs>
        <text textLength={RING_LENGTH.toFixed(0)} lengthAdjust="spacing" dominant-baseline="central">
          <textPath href="#{pathId}">{ringText}</textPath>
        </text>
      </svg>
    {/if}
  </div>
  <div class="label-light"></div>
  <!-- The centre hole punched through the label, and the platter's spindle standing in it. -->
  <div class="hub" aria-hidden="true"><span class="pin"></span></div>
</div>

<style>
  .disc {
    position: relative;
    width: 100%;
    aspect-ratio: 1;
    border-radius: 50%;
    /* layout containment only: paint containment would clip the record's shadow */
    contain: layout style;
  }

  /* The record's shadow on the platter, under the canvas. */
  .shadow {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    box-shadow:
      0 4px 10px rgb(0 0 0 / 0.45),
      0 16px 36px rgb(0 0 0 / 0.55),
      0 0 0 1px rgb(255 255 255 / 0.04);
  }
  /* The still surface: painted by drawVinylSurface. */
  .surface {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border-radius: 50%;
    background: #0b0b0d;
  }

  /* Everything that turns: the label and the title printed round it. */
  .spinner {
    position: absolute;
    inset: 0;
    will-change: transform;
  }
  .label {
    position: absolute;
    inset: 28%;
    border-radius: 50%;
    overflow: hidden;
    background: #26262d;
  }
  .label img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
    transition: filter var(--dur-slow) var(--ease-standard);
  }
  .paused .label img {
    filter: saturate(0.55) brightness(0.9);
  }
  .placeholder {
    width: 100%;
    height: 100%;
    background: radial-gradient(circle at 35% 30%, #3a3a44, #1c1c22 70%);
  }
  .ring {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: visible;
  }
  .ring text {
    font-family: var(--font);
    font-size: 15px;
    font-weight: 600;
    letter-spacing: 0.28em;
    fill: rgb(255 255 255 / 0.2);
  }

  /* Still light over the turning label: printed paper with a slightly lifted edge. */
  .label-light {
    position: absolute;
    inset: 28%;
    border-radius: 50%;
    pointer-events: none;
    background: radial-gradient(circle at 30% 26%, rgb(255 255 255 / 0.14), transparent 58%);
    box-shadow:
      inset 0 0 0 3px rgb(0 0 0 / 0.28),
      inset 0 0 0 4px rgb(255 255 255 / 0.06),
      inset 0 0 22px rgb(0 0 0 / 0.35);
    transition: opacity var(--dur-slow) var(--ease-standard);
  }
  .paused .label-light {
    opacity: 0.85;
  }
  /* Real proportions: an LP's centre hole is about 2.4 % of its diameter and the spindle fills
     most of it. The hole shows as a thin dark gap with the paper's cut edge around it. */
  .hub {
    position: absolute;
    left: 50%;
    top: 50%;
    width: 2.9%;
    aspect-ratio: 1;
    transform: translate(-50%, -50%);
    border-radius: 50%;
    background: radial-gradient(circle, #030304 0 62%, #111114 74%, rgb(255 255 255 / 0.18) 84%, transparent 92%);
    box-shadow: inset 0 1px 1.5px rgb(0 0 0 / 0.8);
  }
  /* The spindle seen from above: a flat machined top with circular tooling marks, a bevelled
     rim catching the light up and to the left, and a small shadow cast down-right. */
  .pin {
    position: absolute;
    inset: 16%;
    border-radius: 50%;
    background:
      radial-gradient(circle, rgb(0 0 0 / 0.3) 0 12%, transparent 16%),
      radial-gradient(circle at 34% 28%, rgb(255 255 255 / 0.75) 0 4%, transparent 26%),
      repeating-radial-gradient(circle, rgb(255 255 255 / 0.08) 0 0.6px, transparent 0.6px 1.4px),
      conic-gradient(from 200deg, #9a9da3, #d9dbdf 70deg, #b4b7bc 140deg, #7d8086 220deg, #8c8f95 300deg, #9a9da3);
    box-shadow:
      inset 0.6px 0.6px 0.8px rgb(255 255 255 / 0.7),
      inset -0.8px -0.8px 1px rgb(0 0 0 / 0.55),
      1px 1.5px 2px rgb(0 0 0 / 0.75);
  }
</style>
