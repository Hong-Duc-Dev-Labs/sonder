<script lang="ts">
  // Soft pools of the playing cover's colours drifting slowly behind the whole app. The pools
  // are painted into a canvas an eighth of the window's size and scaled up by CSS: they are
  // blurry by nature, so the small canvas looks the same at a sixty-fourth of the pixels. The
  // drift is slow, so it is redrawn about 30 times a second through the shared frame loop and
  // stops entirely when nothing moves (reduced motion, `still`, hidden window). With nothing
  // playing the pools take a quiet house palette, so the canvas is never a flat black.
  import { onMount } from "svelte";
  import { onFrame, onReducedMotionChange, reducedMotion } from "../lib/motion";

  let {
    colors = [],
    strength = 0.4,
    still = false,
  }: { colors?: string[]; strength?: number; still?: boolean } = $props();

  const SCALE = 8;
  const STEP = 1 / 30;
  const FADE_SECONDS = 1.4;
  /**
   * Seconds per full drift loop for each pool: quick enough to see the light move, distinct so
   * the pattern never repeats visibly.
   */
  const PERIODS = [19, 27, 23, 31];
  /** Deep, desaturated tones for when no cover lends its colours. */
  const IDLE = ["#3a2d5c", "#1d4652", "#55283a", "#26314f"];

  type Pool = { color: string; phase: number };

  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;
  let previous: Pool[] = [];
  let current: Pool[] = [];
  let mix = 1;
  let clock = Math.random() * 1000;
  let carry = 0;
  let stop: (() => void) | null = null;

  /** Up to four pools; fewer colours are repeated at other phases so the light stays full. */
  function pools(given: string[]): Pool[] {
    const list = given.length ? given : IDLE;
    return PERIODS.map((_, i) => ({ color: list[i % list.length], phase: i * 1.9 + list.length }));
  }

  function draw() {
    if (!ctx) return;
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
        // each pool also swells and shrinks a little, so the light breathes as it drifts
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
    carry += dt;
    if (carry < STEP) return;
    const step = carry;
    carry = 0;
    if (!frozen()) clock += step;
    if (mix < 1) mix = Math.min(1, mix + step / FADE_SECONDS);
    draw();
    // Sleep when nothing moves: a finished fade with the drift frozen.
    if (mix >= 1 && frozen()) sleep();
  }

  function frozen() {
    return still || reducedMotion();
  }

  // Unfreezing wakes the loop; freezing lets the running frame put it to sleep.
  $effect(() => {
    void still;
    wake();
  });

  function wake() {
    if (!stop && ctx) stop = onFrame(frame);
  }

  function sleep() {
    stop?.();
    stop = null;
  }

  function resize() {
    if (!canvas) return;
    canvas.width = Math.max(16, Math.ceil(window.innerWidth / SCALE));
    canvas.height = Math.max(16, Math.ceil(window.innerHeight / SCALE));
    draw();
  }

  // A new set of colours fades in over the old one.
  $effect(() => {
    const next = pools(colors);
    if (next.map((p) => p.color).join() === current.map((p) => p.color).join()) return;
    previous = mix < 0.5 ? previous : current;
    current = next;
    mix = reducedMotion() ? 1 : 0;
    draw();
    wake();
  });

  onMount(() => {
    ctx = canvas.getContext("2d", { alpha: false });
    resize();
    window.addEventListener("resize", resize);
    const offReduced = onReducedMotionChange(() => wake());
    wake();
    return () => {
      sleep();
      offReduced();
      window.removeEventListener("resize", resize);
    };
  });
</script>

<div class="ambient" aria-hidden="true" style:opacity={strength}>
  <canvas bind:this={canvas}></canvas>
  <div class="shade"></div>
</div>

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
  /* Darker toward the bottom, where the player and controls sit. */
  .shade {
    position: absolute;
    inset: 0;
    background:
      radial-gradient(ellipse at 50% 30%, transparent 35%, rgb(0 0 0 / 0.45) 100%),
      linear-gradient(to bottom, transparent 40%, rgb(0 0 0 / 0.6));
  }
</style>
