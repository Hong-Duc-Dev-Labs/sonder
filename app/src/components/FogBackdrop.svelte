<script lang="ts">
  // Volumetric Fog Backdrop ported from egoist/mygo-music:
  // GPU-accelerated WebGL fragment shader with 5-octave FBM noise, domain warping,
  // floating dust specks that glint in their own time, and bass-reactive expansion.
  // Half-resolution rendering with 700ms eased crossfade between song covers.
  // Uses cached ResizeObserver to eliminate layout thrashing inside requestAnimationFrame.
  import { onMount, untrack } from "svelte";
  import { playbackMs } from "../lib/clock.svelte";
  import {
    createBassPulseState,
    createFogRenderer,
    fogColors,
    type FogColors,
  } from "../lib/fog";
  import { onFrame, reducedMotion } from "../lib/motion";

  let {
    color = "#2b2a3a",
    still = false,
    playing = true,
    strength = 1,
  }: {
    color?: string;
    still?: boolean;
    playing?: boolean;
    strength?: number;
  } = $props();

  const FADE_SECONDS = 0.7; // 700ms matching MyGo's backdropFade
  const SCALE = 2; // Render at 1/2 size on GPU for silky 60fps & low power

  let canvas: HTMLCanvasElement;
  let renderer: ReturnType<typeof createFogRenderer> | null = null;
  let stop: (() => void) | null = null;

  let currentTargetColors = $state<FogColors>(untrack(() => fogColors(color)));
  let previousColors = $state<FogColors>(untrack(() => fogColors(color)));
  let mix = 1;

  let drift = Math.random() * 50;
  let speed = 0;
  const meter = createBassPulseState();

  let cachedW = 480;
  let cachedH = 270;

  $effect(() => {
    const next = fogColors(color || "#2b2a3a");
    if (mix >= 1) {
      previousColors = currentTargetColors;
      mix = 0;
    }
    currentTargetColors = next;
  });

  function lerpVec4(
    a: [number, number, number, number],
    b: [number, number, number, number],
    t: number,
  ): [number, number, number, number] {
    return [
      a[0] + (b[0] - a[0]) * t,
      a[1] + (b[1] - a[1]) * t,
      a[2] + (b[2] - a[2]) * t,
      a[3] + (b[3] - a[3]) * t,
    ];
  }

  function frame(dt: number) {
    if (!renderer || !canvas) return;

    const frozen = still || reducedMotion();
    const targetSpeed = playing && !frozen ? 1 : 0;
    speed += (targetSpeed - speed) * Math.min(1, 2.5 * dt);
    if (Math.abs(targetSpeed - speed) < 0.005) {
      speed = targetSpeed;
    }

    drift += speed * dt;
    const bass = meter.step(dt, playing && !frozen, playbackMs());

    if (mix < 1) {
      mix = Math.min(1, mix + dt / FADE_SECONDS);
    }
    // Cubic ease out
    const k = 1 - Math.pow(1 - mix, 3);
    const activeDeep = lerpVec4(previousColors.deep, currentTargetColors.deep, k);
    const activeFog = lerpVec4(previousColors.fog, currentTargetColors.fog, k);

    renderer.render(cachedW, cachedH, drift, bass, activeDeep, activeFog);

    const shouldSleep = (frozen || !playing) && mix >= 1 && speed === 0;
    if (shouldSleep) {
      // Idle sleep
      if (stop) {
        stop();
        stop = null;
      }
    }
  }

  function wake() {
    if (!stop && renderer) {
      stop = onFrame(frame);
    }
  }

  $effect(() => {
    void color;
    void playing;
    void still;
    wake();
  });

  onMount(() => {
    renderer = createFogRenderer(canvas);
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const cr = entry.contentRect;
        cachedW = Math.min(960, Math.max(32, Math.floor(cr.width / SCALE)));
        cachedH = Math.min(540, Math.max(32, Math.floor(cr.height / SCALE)));
      }
    });
    ro.observe(canvas);
    wake();
    return () => {
      ro.disconnect();
      if (stop) stop();
      if (renderer) renderer.destroy();
    };
  });
</script>

<div class="fog-wrap" style:opacity={strength}>
  <canvas bind:this={canvas} class="fog-canvas"></canvas>
  <div class="fog-vignette" aria-hidden="true"></div>
</div>

<style>
  .fog-wrap {
    position: absolute;
    inset: 0;
    overflow: hidden;
    pointer-events: none;
    z-index: 0;
    contain: strict;
    transition: opacity 0.4s ease;
  }

  .fog-canvas {
    width: 100%;
    height: 100%;
    display: block;
    object-fit: cover;
    image-rendering: auto;
  }

  .fog-vignette {
    position: absolute;
    inset: 0;
    background: radial-gradient(circle at 50% 45%, transparent 40%, rgba(11, 11, 13, 0.45) 90%),
      linear-gradient(to bottom, transparent 60%, rgba(11, 11, 13, 0.5) 100%);
    pointer-events: none;
  }
</style>