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

  let {
    art,
    title = "",
    paused = false,
  }: { art: string | null | undefined; title?: string; paused?: boolean } = $props();

  const pathId = `vinyl-ring-${nextId++}`;
  /** Ring text radius in the 1000-unit box: in the dead wax just outside the label (220). */
  const RING_R = 240;
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

  onMount(() => attachSpin(spinner));
</script>

<div class="disc" class:paused>
  <div class="surface"></div>
  <div class="grooves"></div>
  <div class="sheen"></div>
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
        <text textLength={RING_LENGTH.toFixed(0)} lengthAdjust="spacing">
          <textPath href="#{pathId}">{ringText}</textPath>
        </text>
      </svg>
    {/if}
  </div>
  <div class="label-light"></div>
  <div class="spindle"></div>
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

  /* Body and zones, still. Percentages are of the radius (closest-side). */
  .surface {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    box-shadow:
      0 28px 56px rgb(0 0 0 / 0.6),
      0 6px 14px rgb(0 0 0 / 0.5);
    background:
      /* run-out: one bright spiral line where the stylus stops */
      radial-gradient(
        circle closest-side,
        transparent 50.6%,
        rgb(255 255 255 / 0.12) 51%,
        rgb(0 0 0 / 0.6) 51.5%,
        transparent 52%
      ),
      /* lead-in: a smooth glossy band just inside the rim */
      radial-gradient(
        circle closest-side,
        transparent 91.6%,
        rgb(255 255 255 / 0.05) 92%,
        rgb(255 255 255 / 0.02) 94.6%,
        transparent 95%
      ),
      radial-gradient(
        circle closest-side,
        #141418 0 44%,
        #101013 44.5% 50.5%,
        #08080a 51% 96.2%,
        #2c2c32 97.4%,
        #121215 98.8%,
        #050506 100%
      );
  }

  /* The programme area's grooves, visible only where the light catches them. */
  .grooves {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background:
      repeating-radial-gradient(circle at center, rgb(255 255 255 / 0.04) 0 0.6px, transparent 0.6px 1.8px),
      repeating-radial-gradient(circle at center, rgb(0 0 0 / 0.25) 0 1px, transparent 1px 4.2px);
    -webkit-mask-image:
      radial-gradient(circle closest-side, transparent 51.5%, #000 52% 91.6%, transparent 92%),
      conic-gradient(
        from -70deg,
        #000 0deg 8deg,
        rgb(0 0 0 / 0.35) 30deg,
        rgb(0 0 0 / 0.15) 90deg,
        #000 180deg 188deg,
        rgb(0 0 0 / 0.35) 210deg,
        rgb(0 0 0 / 0.15) 270deg,
        #000 360deg
      );
    -webkit-mask-composite: source-in;
    mask-image:
      radial-gradient(circle closest-side, transparent 51.5%, #000 52% 91.6%, transparent 92%),
      conic-gradient(
        from -70deg,
        #000 0deg 8deg,
        rgb(0 0 0 / 0.35) 30deg,
        rgb(0 0 0 / 0.15) 90deg,
        #000 180deg 188deg,
        rgb(0 0 0 / 0.35) 210deg,
        rgb(0 0 0 / 0.15) 270deg,
        #000 360deg
      );
    mask-composite: intersect;
  }

  /* Two light lobes from a fixed light up and to the left, with a narrow hot core and a faint
     warm/cool fringe at their edges. */
  .sheen {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: conic-gradient(
      from -70deg,
      transparent 0deg,
      rgb(255 210 170 / 0.04) 14deg,
      rgb(255 255 255 / 0.12) 21deg,
      rgb(255 255 255 / 0.22) 24deg,
      rgb(255 255 255 / 0.12) 27deg,
      rgb(170 200 255 / 0.04) 34deg,
      transparent 50deg 180deg,
      rgb(255 210 170 / 0.03) 194deg,
      rgb(255 255 255 / 0.08) 201deg,
      rgb(255 255 255 / 0.16) 204deg,
      rgb(255 255 255 / 0.08) 207deg,
      rgb(170 200 255 / 0.03) 214deg,
      transparent 230deg
    );
    -webkit-mask-image: radial-gradient(circle closest-side, transparent 44%, #000 44.5%);
    mask-image: radial-gradient(circle closest-side, transparent 44%, #000 44.5%);
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
  .spindle {
    position: absolute;
    inset: 46.2%;
    border-radius: 50%;
    background: radial-gradient(circle at 36% 30%, #f4f5f7 0 10%, #a7a9ae 32%, #44464b 62%, #151517 100%);
    box-shadow:
      0 0 0 1px rgb(0 0 0 / 0.6),
      0 2px 3px rgb(0 0 0 / 0.55);
  }
</style>
