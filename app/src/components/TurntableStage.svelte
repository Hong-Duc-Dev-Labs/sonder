<script lang="ts">
  // The platter, the record and the arm. On a track change the arm lifts to its rest first,
  // then the old record slides out (still turning), the new one lands from the side the queue
  // moved to, and the arm drops into its lead-in. Skipping again mid-change restarts the
  // sequence from there. Changes are keyed by queue entry, so editing the queue around the
  // playing entry does not swap the record.
  import { app } from "../lib/app.svelte";
  import { reducedMotion } from "../lib/motion";
  import { RECORD_LAND_DELAY, RECORD_LAND_MS, slideRecord } from "../lib/transitions";
  import Tonearm from "./Tonearm.svelte";
  import VinylDisc from "./VinylDisc.svelte";

  /** The new record has landed, plus a beat, before the arm drops. */
  const SETTLE_MS = RECORD_LAND_DELAY + RECORD_LAND_MS + 80;
  const STROBE_DOTS = 60;

  let item = $derived(app.current);
  let trackKey = $derived(item ? `entry-${item.entryId}` : "none");
  let changing = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let seenKey: string | null = null;

  $effect(() => {
    const key = trackKey;
    if (seenKey !== null && key !== seenKey && !reducedMotion()) {
      changing = true;
      clearTimeout(timer);
      timer = setTimeout(() => (changing = false), SETTLE_MS);
    }
    seenKey = key;
    return () => clearTimeout(timer);
  });

  const dots = Array.from({ length: STROBE_DOTS }, (_, i) => {
    const a = (i / STROBE_DOTS) * Math.PI * 2;
    return { x: 50 + 48.6 * Math.cos(a), y: 50 + 48.6 * Math.sin(a) };
  });
</script>

<div class="stage">
  <!-- the platter: brushed metal with strobe dots on its rim, still -->
  <div class="platter" aria-hidden="true">
    <svg viewBox="0 0 100 100">
      {#each dots as d, i (i)}
        <circle cx={d.x} cy={d.y} r="0.42" />
      {/each}
    </svg>
  </div>
  <div class="record-slot">
    {#key trackKey}
      <div
        class="record"
        in:slideRecord={{ dir: app.trackDirection, leaving: false }}
        out:slideRecord={{ dir: app.trackDirection, leaving: true }}
      >
        <VinylDisc art={item?.art} title={item?.album ?? ""} paused={app.player.state === "paused"} />
      </div>
    {/key}
  </div>
  <Tonearm parked={changing || !item} />
</div>

<style>
  .stage {
    position: relative;
    width: 100%;
    aspect-ratio: 1;
  }
  /* Record placement matches the tonearm geometry: centre (450, 500), radius 430 of 1000. */
  .record-slot,
  .platter {
    position: absolute;
    left: 2%;
    top: 7%;
    width: 86%;
    aspect-ratio: 1;
  }
  .platter {
    inset: auto;
    left: 0.45%;
    top: 5.45%;
    width: 89.1%;
    border-radius: 50%;
    background:
      repeating-conic-gradient(rgb(255 255 255 / 0.025) 0 0.4deg, transparent 0.4deg 1.1deg),
      radial-gradient(circle, #2f3137 0 70%, #24262b 100%);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.12),
      0 18px 40px rgb(0 0 0 / 0.55);
  }
  .platter svg {
    width: 100%;
    height: 100%;
    fill: rgb(255 255 255 / 0.18);
  }
  .record {
    position: absolute;
    inset: 0;
  }
</style>
