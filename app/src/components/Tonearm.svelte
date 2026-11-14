<script lang="ts">
  // The tonearm over the record. Its angle is the song's progress: the stylus starts in the
  // lead-in by the rim and swings in toward the label as the song plays. Drag it (or focus it
  // and use the arrow keys) to seek. The arm and its shadow are drawn once and only rotated
  // about the pivot, so following the song never repaints them; the shadow's blur is an SVG
  // filter rasterised once with its layer.
  import { onMount, untrack } from "svelte";
  import { app } from "../lib/app.svelte";
  import { commands } from "../lib/bindings";
  import { playbackMs } from "../lib/clock.svelte";
  import { duration as formatTime } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { Spring, onFrame, reducedMotion } from "../lib/motion";
  import {
    END_ANGLE,
    PIVOT,
    REACH,
    REST_ANGLE,
    STAGE,
    START_ANGLE,
    angleAroundPivot,
    armAngleFor,
    fractionForAngle,
    stylusArcPath,
    stylusAt,
  } from "../lib/turntable";

  /** `parked`: the arm goes back to its rest (nothing playing, or a record being changed). */
  let { parked = false }: { parked?: boolean } = $props();

  const JUMP_DEGREES = 1.5;
  const KEY_STEP_MS = 5_000;
  const PAGE_STEP_MS = 30_000;
  const SEEK_HOLD_MS = 1500;
  /** How far playback may move before a focused slider announces a new value. */
  const ANNOUNCE_MS = 15_000;

  let stage: HTMLDivElement;
  let armLayer: HTMLDivElement;
  let shadowLayer: HTMLDivElement;
  let grab: SVGGElement;

  const angle = new Spring(REST_ANGLE, { stiffness: 70, damping: 0.82 });
  const lift = new Spring(0, { stiffness: 260, damping: 0.9 });

  let dragging = $state(false);
  let dragAngle = $state(START_ANGLE);
  let grabOffset = 0;
  let dragStartAngle = 0;
  let pointerId: number | null = null;
  let destroyed = false;
  let lowerTimer: ReturnType<typeof setTimeout> | undefined;

  let totalMs = $derived(app.player.durationMs ?? app.current?.durationMs ?? 0);
  let canSeek = $derived(!parked && totalMs > 0 && app.player.index != null);
  let scrubMs = $derived(Math.round(fractionForAngle(dragAngle) * totalMs));

  // The value a screen reader hears: follows seeks, but not every second of playback.
  let ariaMs = $state(0);
  $effect(() => {
    const elapsed = app.player.elapsedMs;
    if (Math.abs(elapsed - untrack(() => ariaMs)) > ANNOUNCE_MS) ariaMs = elapsed;
  });

  function targetAngle(): number {
    if (!canSeek) return REST_ANGLE;
    return armAngleFor(playbackMs() / totalMs);
  }

  let stopLoop: (() => void) | null = null;
  let shown = { angle: NaN, lift: NaN };

  function render() {
    // The arm creeps about a tenth of a degree a second: most frames change nothing.
    if (Math.abs(angle.value - shown.angle) < 0.004 && Math.abs(lift.value - shown.lift) < 0.001) return;
    shown = { angle: angle.value, lift: lift.value };
    const turn = (angle.value - START_ANGLE).toFixed(3);
    armLayer.style.transform = `rotate(${turn}deg) scale(${(1 + 0.025 * lift.value).toFixed(4)})`;
    const shift = 1.2 + 2.4 * lift.value;
    shadowLayer.style.transform = `translate(${(shift * 0.55).toFixed(2)}%, ${(shift * 1.1).toFixed(2)}%) rotate(${turn}deg)`;
    shadowLayer.style.opacity = (0.8 - 0.3 * lift.value).toFixed(3);
  }

  /** A seek just sent: hold the arm where it was dropped until the clock reaches it. */
  let pendingSeek: { ms: number; until: number } | null = null;

  function waitingForSeek(now: number): boolean {
    if (!pendingSeek) return false;
    if (now > pendingSeek.until || Math.abs(playbackMs(now) - pendingSeek.ms) < 1000) {
      pendingSeek = null;
      return false;
    }
    return true;
  }

  function frame(dt: number, now: number) {
    if (!dragging && !waitingForSeek(now)) {
      const target = targetAngle();
      if (reducedMotion()) {
        angle.snap(target);
      } else if (angle.settled && Math.abs(target - angle.value) <= JUMP_DEGREES) {
        angle.snap(target);
      } else {
        angle.target = target;
      }
      lift.target = Math.abs(angle.target - angle.value) > JUMP_DEGREES ? 0.5 : 0;
    }
    const moving = angle.step(dt);
    const lifting = lift.step(dt);
    render();
    const idle = !dragging && !pendingSeek && !moving && !lifting && app.player.state !== "playing";
    if (idle) sleep();
  }

  function wake() {
    if (!stopLoop && !destroyed) stopLoop = onFrame(frame);
  }

  function sleep() {
    stopLoop?.();
    stopLoop = null;
  }

  // Anything that changes where the arm belongs wakes the loop; it sleeps again once still.
  $effect(() => {
    void app.player;
    void parked;
    void totalMs;
    wake();
  });

  onMount(() => {
    angle.snap(targetAngle());
    render();
    wake();
    return () => {
      destroyed = true;
      clearTimeout(lowerTimer);
      sleep();
    };
  });

  function toStage(e: PointerEvent) {
    const rect = stage.getBoundingClientRect();
    return {
      x: ((e.clientX - rect.left) / rect.width) * STAGE,
      y: ((e.clientY - rect.top) / rect.height) * STAGE,
    };
  }

  function onPointerDown(e: PointerEvent) {
    if (!canSeek || e.button !== 0) return;
    e.preventDefault();
    // preventDefault stops the click from focusing the arm; focus it so Escape can cancel.
    grab.focus({ preventScroll: true });
    grab.setPointerCapture(e.pointerId);
    pointerId = e.pointerId;
    dragStartAngle = angle.value;
    grabOffset = angle.value - angleAroundPivot(toStage(e));
    dragAngle = angle.value;
    dragging = true;
    lift.target = 1;
    wake();
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging || e.pointerId !== pointerId) return;
    const next = Math.min(END_ANGLE, Math.max(START_ANGLE, angleAroundPivot(toStage(e)) + grabOffset));
    dragAngle = next;
    angle.snap(next);
  }

  function endDrag(commit: boolean) {
    if (!dragging) return;
    dragging = false;
    if (pointerId != null && grab.hasPointerCapture(pointerId)) grab.releasePointerCapture(pointerId);
    pointerId = null;
    if (commit) {
      pendingSeek = { ms: scrubMs, until: performance.now() + SEEK_HOLD_MS };
      ariaMs = scrubMs;
      app.run(commands.playerSeek(scrubMs));
    } else {
      angle.target = dragStartAngle;
    }
    // Hold it up a moment so the seek lands before it settles back down.
    clearTimeout(lowerTimer);
    lowerTimer = setTimeout(() => {
      if (destroyed) return;
      if (!dragging) lift.target = 0;
      wake();
    }, 300);
    wake();
  }

  // Repeated keys add up before the engine reports the previous seek.
  let keyTarget: { ms: number; until: number } | null = null;
  function seekTo(ms: number) {
    const to = Math.round(Math.min(Math.max(0, totalMs - 500), Math.max(0, ms)));
    keyTarget = { ms: to, until: performance.now() + 700 };
    pendingSeek = { ms: to, until: performance.now() + SEEK_HOLD_MS };
    ariaMs = to;
    angle.target = armAngleFor(to / totalMs);
    wake();
    app.run(commands.playerSeek(to));
  }
  function seekBy(delta: number) {
    const now = performance.now();
    const base = keyTarget && keyTarget.until > now ? keyTarget.ms : playbackMs(now);
    seekTo(base + delta);
  }

  function onKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape" && dragging) {
      e.preventDefault();
      endDrag(false);
      return;
    }
    if (!canSeek) return;
    const keys: Record<string, () => void> = {
      ArrowLeft: () => seekBy(-KEY_STEP_MS),
      ArrowDown: () => seekBy(-KEY_STEP_MS),
      ArrowRight: () => seekBy(KEY_STEP_MS),
      ArrowUp: () => seekBy(KEY_STEP_MS),
      PageDown: () => seekBy(-PAGE_STEP_MS),
      PageUp: () => seekBy(PAGE_STEP_MS),
      Home: () => seekTo(0),
      End: () => seekTo(totalMs),
    };
    const action = keys[e.key];
    if (action) {
      e.preventDefault();
      e.stopPropagation();
      action();
    }
  }

  // The arm is drawn pointing at the song's start angle; the layer rotates from there.
  const base = START_ANGLE;
  const tip = stylusAt(base);
  const rad = (base * Math.PI) / 180;
  const along = (d: number) => ({ x: PIVOT.x + d * Math.cos(rad), y: PIVOT.y + d * Math.sin(rad) });
  const tubeStart = along(40);
  const pivotPct = { x: (PIVOT.x / STAGE) * 100, y: (PIVOT.y / STAGE) * 100 };
  const rest = stylusAt(REST_ANGLE);

  // Headshell geometry along the arm's own x axis (before the base rotation).
  const P = PIVOT;
  const shellFrom = P.x + REACH - 78;
  const shellTo = P.x + REACH + 24;

  let elapsedLabel = $derived(formatTime(scrubMs));
  let arcPath = $derived(stylusArcPath(dragAngle));
  const fullArc = stylusArcPath(END_ANGLE);
  let labelAt = $derived.by(() => {
    const a = (dragAngle * Math.PI) / 180;
    const r = REACH + 64;
    return { x: PIVOT.x + r * Math.cos(a), y: PIVOT.y + r * Math.sin(a) };
  });
</script>

{#snippet armShapes(shadow: boolean)}
  <g transform="rotate({base} {P.x} {P.y})" class:shadow>
    <!-- counterweight: a turned cylinder seen from above, behind a dark decoupling ring -->
    <rect x={P.x - 116} y={P.y - 27} width="62" height="54" rx="9" fill="url(#alu-across)" />
    {#each [-104, -94, -84, -74] as dx (dx)}
      <rect x={P.x + dx} y={P.y - 27} width="1.5" height="54" fill="rgb(0 0 0 / .32)" />
    {/each}
    <rect x={P.x - 54} y={P.y - 15} width="9" height="30" rx="2" fill="#0d0d10" />
    <rect x={P.x - 46} y={P.y - 8} width="30" height="16" rx="3" fill="#17181b" />
    <!-- tube, with a specular line along its top and a darker underside -->
    <rect x={P.x + 24} y={P.y - 9} width={REACH - 100} height="18" rx="9" fill="url(#alu-along)" />
    <rect x={P.x + 30} y={P.y - 5} width={REACH - 112} height="2" rx="1" fill="rgb(255 255 255 / .55)" />
    <rect x={P.x + 30} y={P.y + 6} width={REACH - 112} height="1.2" rx=".6" fill="rgb(0 0 0 / .35)" />
    <rect x={P.x + 26} y={P.y - 11} width="34" height="22" rx="4" fill="#16171a" />
    <rect x={P.x + REACH - 90} y={P.y - 11} width="16" height="22" rx="3" fill="#16171a" />
    <!-- headshell: tapered, angled off the tube, carrying the cartridge -->
    <g transform="rotate(-14 {shellFrom} {P.y})">
      <path
        d="M {shellFrom} {P.y - 17} L {shellTo} {P.y - 13} Q {shellTo + 6} {P.y} {shellTo} {P.y + 13} L {shellFrom} {P.y + 17} Z"
        fill="#2b2c31"
        stroke="rgb(255 255 255 / .16)"
        stroke-width="1.5"
        stroke-linejoin="round"
      />
      <rect x={shellTo - 52} y={P.y - 12} width="42" height="24" rx="3" fill="#0f0f12" />
      <rect x={shellTo - 52} y={P.y - 1.5} width="42" height="3" fill="rgb(255 255 255 / .55)" />
      <path
        d="M {shellTo - 4} {P.y - 13} q 10 -14 22 -30"
        fill="none"
        stroke="#a8abb0"
        stroke-width="6"
        stroke-linecap="round"
      />
    </g>
    <!-- gimbal and bearing cap over the pivot -->
    <circle cx={P.x} cy={P.y} r="34" fill="#1b1c20" stroke="#5a5c62" stroke-width="3" />
    <circle cx={P.x} cy={P.y} r="15" fill="url(#cap)" />
  </g>
{/snippet}

<div class="arm-stage" bind:this={stage}>
  <!-- the arm's base, lever, anti-skate knob and the rest its headshell parks on: still -->
  <svg class="layer" viewBox="0 0 {STAGE} {STAGE}" aria-hidden="true">
    <defs>
      <radialGradient id="plinth" cx="35%" cy="30%" r="80%">
        <stop offset="0" stop-color="#2a2c33" />
        <stop offset="1" stop-color="#0c0d10" />
      </radialGradient>
      <filter id="base-shadow" x="-30%" y="-30%" width="160%" height="160%">
        <feGaussianBlur stdDeviation="8" />
      </filter>
    </defs>
    <circle cx={P.x + 8} cy={P.y + 16} r="66" fill="rgb(0 0 0 / .55)" filter="url(#base-shadow)" />
    <circle cx={P.x} cy={P.y} r="64" fill="url(#plinth)" stroke="rgb(255 255 255 / .1)" stroke-width="1.5" />
    <circle cx={P.x} cy={P.y} r="50" fill="none" stroke="rgb(255 255 255 / .05)" />
    <circle cx={P.x - 52} cy={P.y + 86} r="15" fill="url(#plinth)" stroke="rgb(255 255 255 / .12)" stroke-width="1.5" />
    <line x1={P.x - 52} y1={P.y + 76} x2={P.x - 52} y2={P.y + 82} stroke="rgb(255 255 255 / .45)" stroke-width="2" stroke-linecap="round" />
    <rect x={P.x + 30} y={P.y + 92} width="58" height="10" rx="5" fill="#1d1e23" stroke="rgb(255 255 255 / .1)" />
    <circle cx={P.x + 34} cy={P.y + 97} r="9" fill="url(#plinth)" stroke="rgb(255 255 255 / .12)" />
    <!-- arm rest: a post with a U cradle -->
    <rect x={rest.x - 8} y={rest.y - 4} width="16" height="40" rx="6" fill="#16171b" stroke="rgb(255 255 255 / .08)" />
    <path
      d="M {rest.x - 16} {rest.y - 18} v 10 a 16 16 0 0 0 32 0 v -10"
      fill="none"
      stroke="#3a3c42"
      stroke-width="5"
      stroke-linecap="round"
    />
  </svg>

  <div class="layer shadow-layer" bind:this={shadowLayer} style:transform-origin="{pivotPct.x}% {pivotPct.y}%">
    <svg viewBox="0 0 {STAGE} {STAGE}" aria-hidden="true">
      <defs>
        <filter id="arm-shadow" x="-10%" y="-10%" width="120%" height="120%">
          <feGaussianBlur stdDeviation="7" />
        </filter>
      </defs>
      <g filter="url(#arm-shadow)">{@render armShapes(true)}</g>
    </svg>
  </div>

  <div class="layer arm-layer" bind:this={armLayer} style:transform-origin="{pivotPct.x}% {pivotPct.y}%">
    <svg viewBox="0 0 {STAGE} {STAGE}">
      <defs>
        <linearGradient id="alu-along" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stop-color="#eceef1" />
          <stop offset="0.3" stop-color="#c3c6cb" />
          <stop offset="0.7" stop-color="#7f8288" />
          <stop offset="1" stop-color="#46484d" />
        </linearGradient>
        <linearGradient id="alu-across" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stop-color="#d6d8dc" />
          <stop offset="0.45" stop-color="#9a9da3" />
          <stop offset="1" stop-color="#3f4146" />
        </linearGradient>
        <radialGradient id="cap" cx="38%" cy="32%" r="70%">
          <stop offset="0" stop-color="#f4f5f7" />
          <stop offset="0.35" stop-color="#a7a9ae" />
          <stop offset="0.7" stop-color="#44464b" />
          <stop offset="1" stop-color="#151517" />
        </radialGradient>
      </defs>
      {@render armShapes(false)}
      <!-- the stylus, exactly where the geometry puts it -->
      <circle cx={tip.x} cy={tip.y} r="3" fill="#d0d2d6" />
      <!-- the grab target: a wide invisible stroke along tube and headshell -->
      <g
        bind:this={grab}
        class="grab"
        class:enabled={canSeek}
        role="slider"
        tabindex={canSeek ? 0 : -1}
        aria-label={t("tonearm.seek")}
        aria-valuemin={0}
        aria-valuemax={Math.round(totalMs / 1000)}
        aria-valuenow={Math.round((dragging ? scrubMs : ariaMs) / 1000)}
        aria-valuetext={t("common.seekValue", {
          elapsed: formatTime(dragging ? scrubMs : ariaMs),
          total: formatTime(totalMs),
        })}
        aria-disabled={!canSeek}
        onpointerdown={onPointerDown}
        onpointermove={onPointerMove}
        onpointerup={() => endDrag(true)}
        onpointercancel={() => endDrag(false)}
        onlostpointercapture={() => endDrag(false)}
        onkeydown={onKeyDown}
      >
        <line x1={tubeStart.x} y1={tubeStart.y} x2={tip.x} y2={tip.y} class="hit" />
        <circle cx={tip.x} cy={tip.y} r="42" class="hit-head" />
        <circle cx={tip.x} cy={tip.y} r="48" class="focus-ring" />
      </g>
    </svg>
  </div>

  {#if dragging}
    <svg class="layer arc" viewBox="0 0 {STAGE} {STAGE}" aria-hidden="true">
      <path d={fullArc} class="track" />
      <path d={arcPath} class="done" />
      <text x={labelAt.x} y={labelAt.y} class="time">{elapsedLabel} / {formatTime(totalMs)}</text>
    </svg>
  {/if}
</div>

<style>
  .arm-stage {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .layer {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: visible;
  }
  .layer svg {
    width: 100%;
    height: 100%;
    overflow: visible;
  }
  .arm-layer,
  .shadow-layer {
    will-change: transform;
  }
  .shadow :global(*) {
    fill: #000 !important;
    stroke: #000 !important;
  }

  .grab {
    outline: none;
  }
  .grab.enabled {
    pointer-events: auto;
    cursor: grab;
  }
  .grab.enabled:active {
    cursor: grabbing;
  }
  .hit {
    stroke: transparent;
    stroke-width: 60;
    stroke-linecap: round;
  }
  .hit-head {
    fill: transparent;
  }
  .focus-ring {
    fill: none;
    stroke: var(--ink);
    stroke-width: 4;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--dur-fast) var(--ease-standard);
  }
  :global(:root:not([data-modality="pointer"])) .grab:focus-visible .focus-ring {
    opacity: 1;
  }

  .arc .track {
    fill: none;
    stroke: rgb(255 255 255 / 0.18);
    stroke-width: 6;
    stroke-linecap: round;
  }
  .arc .done {
    fill: none;
    stroke: var(--ink);
    stroke-width: 6;
    stroke-linecap: round;
  }
  .arc .time {
    fill: var(--ink);
    font-family: var(--font-figures);
    font-size: 30px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    paint-order: stroke;
    stroke: rgb(0 0 0 / 0.65);
    stroke-width: 6px;
  }
</style>
