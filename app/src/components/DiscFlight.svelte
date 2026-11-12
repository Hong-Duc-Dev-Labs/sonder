<script lang="ts">
  // The record flying between Now Playing and the player pill. It starts exactly over the
  // record that just left and lands exactly on the one that is arriving (hidden meanwhile), its
  // label still turning in step with playback, then cross-fades into it. Only transform,
  // border-radius and opacity animate.
  import { onMount, untrack } from "svelte";
  import { app, type DiscFlight } from "../lib/app.svelte";
  import { artUrl } from "../lib/art";
  import RecordAvatar from "./RecordAvatar.svelte";

  let { flight }: { flight: DiscFlight } = $props();

  /** Landing on the small pill disc is quicker than growing into the stage. */
  const DURATION = { pill: 560, stage: 640 };
  /** Matches VinylDisc's label (its inset of 28 %). */
  const LABEL = 0.44;
  /** Share of the flight spent travelling; the rest is the hand-over fade. */
  const LANDING = 0.8;

  let el: HTMLDivElement;
  // Captured once: the flight runs from where it was launched, whatever changes meanwhile.
  // Each flight is its own instance (keyed in App), so reading the prop once is intended.
  const { from, to, fromRound, toRound, target, art } = untrack(() => flight);

  onMount(() => {
    const scale = to.width / from.width;
    const dx = to.left - from.left;
    const dy = to.top - from.top;
    // A gentle arc: the record lifts a little off the straight line at mid-flight.
    const lift = Math.min(60, Math.hypot(dx, dy) * 0.12) * (dy > 0 ? -1 : 1);
    const radius = (round: boolean) => (round ? "50%" : "8%");
    const mine = () => app.flight === untrack(() => flight);
    const travel = el.animate(
      [
        { transform: "translate(0, 0) scale(1)", borderRadius: radius(fromRound) },
        {
          offset: 0.58,
          transform: `translate(${dx * 0.55}px, ${dy * 0.5 + lift}px) scale(${(1 + scale) / 2})`,
        },
        { transform: `translate(${dx}px, ${dy}px) scale(${scale})`, borderRadius: radius(toRound) },
      ],
      { duration: DURATION[target] * LANDING, easing: "cubic-bezier(0.32, 0.72, 0, 1)", fill: "forwards" },
    );
    // Landed exactly over the target: show the target underneath first, then fade the copy
    // away on top, so the record never dims or blinks as it hands over.
    let fade: Animation | undefined;
    travel.finished.then(
      () => {
        if (!mine()) return;
        app.flight!.landed = true;
        fade = el.animate([{ opacity: 1 }, { opacity: 0 }], {
          duration: DURATION[target] * (1 - LANDING),
          delay: 32,
          easing: "ease-out",
          fill: "forwards",
        });
        fade.finished.then(
          () => {
            if (mine()) app.flight = null;
          },
          () => {},
        );
      },
      () => {},
    );
    return () => {
      travel.cancel();
      fade?.cancel();
    };
  });
</script>

<div
  class="flight"
  bind:this={el}
  style:left="{from.left}px"
  style:top="{from.top}px"
  style:width="{from.width}px"
  style:height="{from.height}px"
  aria-hidden="true"
>
  {#if fromRound || toRound}
    <div class="record" class:fade={!fromRound}>
      <RecordAvatar {art} size={1000} label={LABEL} spinning />
    </div>
  {/if}
  {#if !fromRound || !toRound}
    <img class="cover" class:fade={fromRound} src={artUrl(art, 1000) ?? ""} alt="" />
  {/if}
</div>

<style>
  .flight {
    position: fixed;
    z-index: 25;
    transform-origin: 0 0;
    pointer-events: none;
    overflow: hidden;
    border-radius: 50%;
    will-change: transform;
    box-shadow: 0 24px 48px rgb(0 0 0 / 0.55);
  }
  .record,
  .cover {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .record :global(.record) {
    box-shadow: none;
  }
  .cover {
    object-fit: cover;
    display: block;
  }
  /* A square cover becoming the round record, or the record becoming the square cover: the
     look it ends as fades in over the other while the shape rounds or squares off. */
  .fade {
    animation: crossfade 0.45s ease-in-out both;
  }
  @keyframes crossfade {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
</style>
