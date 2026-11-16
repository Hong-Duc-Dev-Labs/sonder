<script lang="ts">
  // Ambient backdrop behind the whole app:
  // When fogBackdrop is enabled (default), renders the GPU-accelerated FogBackdrop
  // with 5-octave FBM noise, domain warping, and audio-reactive bass pulse.
  // Otherwise falls back to soft 2D canvas pools of the cover's colours.
  import { onMount } from "svelte";
  import { app } from "../lib/app.svelte";
  import { onFrame, onReducedMotionChange, reducedMotion } from "../lib/motion";
  import FogBackdrop from "./FogBackdrop.svelte";

  let {
    colors = [],
    strength = 0.4,
    still = false,
  }: { colors?: string[]; strength?: number; still?: boolean } = $props();

  const SCALE = 8;
  const STEP = 1 / 30;
  const FADE_SECONDS = 1.4;
  const PERIODS = [19, 27, 23, 31];
  const IDLE = ["#3a2d5c", "#1d4652", "#55283a", "#26314f"];

  type Pool = { color: string; phase: number };

  let canvas: HTMLCanvasElement | undefined = $state();
  let ctx: CanvasRenderingContext2D | null = null;
  let previous: Pool[] = [];
  let current: Pool[] = [];
  let mix = 1;
  let clock = Math.random() * 1000;
  let carry = 0;
  let stop: (() => void) | null = null;

  let useFog = $derived(app.settings.fogBackdrop ?? true);
  let isPlaying = $derived(app.player.state === "playing");
  let primaryColor = $derived(colors[0] ?? undefined);

  function pools(given: string[]): Pool[] {
    const list = given.length ? given : IDLE;
    return PERIODS.map((_, i) => ({ color: list[i % list.length], phase: i * 1.9 + list.length }));
  }

  function draw() {
    if (!ctx || !canvas || useFog) return;
    const w = canvas.width;
    const h = canvas.height;
    ctx.globalCompositeOperation = "source-over";
    ctx.globalAlpha = 1;
    ctx.fillStyle = "#000";
    ctx.fillRect(0, 0, w, h);
    ctx.globalCompositeOperation = "screen";
    const radius = Math.max(w, h) * 0.62;
    const layer = (set: Pool[], alpha: number) => {
      if (alpha <= 0.001) return;
      set.forEach((pool, i) => {
        const t = (clock / PERIODS[i]) * Math.PI * 2 + pool.phase;
        const x = w * (0.5 + 0.4 * Math.sin(t) * (i % 2 ? 1 : -1));
        const y = h * (0.45 + 0.36 * Math.cos(t * 0.8 + i));
        const breath = 1 + 0.14 * Math.sin(t * 1.7 + i * 2.1);
        const g = ctx!.createRadialGradient(x, y, 0, x, y, radius * (0.8 + 0.12 * i) * breath);
        g.addColorStop(0, pool.color);
        g.addColorStop(1, "transparent");
        ctx!.globalAlpha = alpha * (i === 0 ? 0.85 : 0.6);
        ctx!.fillStyle = g;
        ctx!.fillRect(0, 0, w, h);
      });
    };
    layer(previous, 1 - mix);
    layer(current, mix);
  }

  function frame(dt: number) {
    if (useFog) return;
    carry += dt;
    if (carry < STEP) return;
    const step = carry;
    carry = 0;
    if (!frozen()) clock += step;
    if (mix < 1) mix = Math.min(1, mix + step / FADE_SECONDS);
    draw();
    if (mix >= 1 && frozen()) sleep();
  }

  function frozen() {
    return still || reducedMotion();
  }

  $effect(() => {
    void still;
    if (!useFog) wake();
  });

  function wake() {
    if (!stop && ctx && !useFog) stop = onFrame(frame);
  }

  function sleep() {
    stop?.();
    stop = null;
  }

  function resize() {
    if (!canvas || useFog) return;
    canvas.width = Math.max(16, Math.ceil(window.innerWidth / SCALE));
    canvas.height = Math.max(16, Math.ceil(window.innerHeight / SCALE));
    draw();
  }

  $effect(() => {
    if (useFog) return;
    const next = pools(colors);
    if (next.map((p) => p.color).join() === current.map((p) => p.color).join()) return;
    previous = mix < 0.5 ? previous : current;
    current = next;
    mix = reducedMotion() ? 1 : 0;
    draw();
    wake();
  });

  $effect(() => {
    if (canvas && !useFog) {
      ctx = canvas.getContext("2d", { alpha: false });
      resize();
      window.addEventListener("resize", resize);
      wake();
      return () => {
        sleep();
        window.removeEventListener("resize", resize);
        ctx = null;
      };
    }
  });

  onMount(() => {
    const offReduced = onReducedMotionChange(() => wake());
    return () => {
      offReduced();
    };
  });
</script>

{#if useFog}
  <FogBackdrop color={primaryColor} {still} playing={isPlaying} {strength} />
{:else}
  <div class="ambient" aria-hidden="true" style:opacity={strength}>
    <canvas bind:this={canvas}></canvas>
    <div class="shade"></div>
  </div>
{/if}

<style>
  .ambient {
    position: fixed;
    inset: 0;
    z-index: 0;
    pointer-events: none;
    transition: opacity 900ms var(--ease-standard);
  }
  canvas {
    width: 100%;
    height: 100%;
    display: block;
  }
  .shade {
    position: absolute;
    inset: 0;
    background:
      radial-gradient(ellipse at 50% 30%, transparent 35%, rgb(0 0 0 / 0.45) 100%),
      linear-gradient(to bottom, transparent 40%, rgb(0 0 0 / 0.6));
  }
</style>
