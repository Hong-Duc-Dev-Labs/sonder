<script lang="ts">
  // The transport bar's small record: the same spin as the big one, simplified to read at 56 px.
  import { onMount } from "svelte";
  import { artUrl } from "../lib/art";
  import { attachSpin } from "../lib/spin.svelte";

  let { art, paused = false }: { art: string | null | undefined; paused?: boolean } = $props();
  let spinner: HTMLDivElement;
  let failed = $state(false);
  let src = $derived(artUrl(art, 300));
  $effect(() => {
    void src;
    failed = false;
  });

  onMount(() => attachSpin(spinner));
</script>

<div class="mini" class:paused>
  <div class="spinner" bind:this={spinner}>
    {#if src && !failed}
      <img {src} alt="" draggable="false" onerror={() => (failed = true)} />
    {/if}
  </div>
  <div class="hole"></div>
</div>

<style>
  .mini {
    position: relative;
    width: 100%;
    aspect-ratio: 1;
    border-radius: 50%;
    background: radial-gradient(
      circle closest-side,
      transparent 52%,
      #0c0c0e 53% 68%,
      rgba(255, 255, 255, 0.08) 69%,
      #0c0c0e 70.5% 84%,
      rgba(255, 255, 255, 0.08) 85%,
      #0c0c0e 86.5% 96%,
      #2b2b31 98%,
      #050506 100%
    );
    box-shadow: 0 3px 8px rgba(0, 0, 0, 0.5);
  }
  .spinner {
    position: absolute;
    inset: 24%;
    border-radius: 50%;
    overflow: hidden;
    background: #27303b;
    will-change: transform;
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
    transition: filter var(--dur-slow) var(--ease-standard);
  }
  .paused img {
    filter: saturate(0.55) brightness(0.92);
  }
  .hole {
    position: absolute;
    inset: 46%;
    border-radius: 50%;
    background: #0b0b0d;
  }
</style>
