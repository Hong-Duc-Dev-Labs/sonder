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

</script>

<div class="stage">
  <!-- the platter under the record: only its machined rim shows past the record's edge -->
  <div class="platter" aria-hidden="true"></div>
  <div class="record-slot" data-flight="stage" class:landing={app.flight?.target === "stage" && !app.flight.landed}>
    {#key trackKey}
      <div
        class="record"
        in:slideRecord={{ dir: app.trackDirection, leaving: false, active: changing }}
        out:slideRecord={{ dir: app.trackDirection, leaving: true }}
      >
        <VinylDisc art={item?.art} title={item?.album ?? ""} paused={app.player.state === "paused"} />
      </div>
    {/key}
  </div>
  <!-- the arm waits on its rest while a record flies in, then drops into the groove -->
  <Tonearm parked={changing || !item || app.flight?.target === "stage"} />
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
    /* rubber mat inside, a narrow machined aluminium rim lit from the upper left */
    background:
      radial-gradient(circle closest-side, #141416 0 95.6%, transparent 96%),
      conic-gradient(from -24deg, #8d9096, #3a3c41 70deg, #23252a 150deg, #5d6066 215deg, #b9bcc1 290deg, #8d9096);
    box-shadow:
      inset 0 0 0 1px rgb(0 0 0 / 0.6),
      0 18px 40px rgb(0 0 0 / 0.55);
  }
  /* Hidden while the record flies in from the player. It shows at once when the flight lands
     on it (the flying copy still covers it, then fades away on top). */
  .record-slot.landing {
    opacity: 0;
  }
  .record {
    position: absolute;
    inset: 0;
  }
</style>
