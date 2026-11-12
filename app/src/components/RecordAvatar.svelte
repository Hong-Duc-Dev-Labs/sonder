<script lang="ts">
  // A vinyl record whose label is a cover, drawn with gradients only: artist avatars, the record
  // peeking out of an album's sleeve, and the record flying between Now Playing and the player.
  // With `spinning`, the label turns in step with the playing record.
  import { artUrl, type ArtSize } from "../lib/art";
  import { attachSpin } from "../lib/spin.svelte";

  let {
    art,
    size = 300,
    label = 0.42,
    spinning = false,
  }: { art: string | null | undefined; size?: ArtSize; label?: number; spinning?: boolean } = $props();
  let src = $derived(artUrl(art, size));
  let failed = $state(false);
  let labelEl: HTMLImageElement | undefined = $state();
  $effect(() => {
    void src;
    failed = false;
  });
  $effect(() => {
    if (spinning && labelEl) return attachSpin(labelEl);
  });
</script>

<span class="record" style:--label="{label * 100}%" aria-hidden="true">
  {#if src && !failed}
    <img bind:this={labelEl} {src} alt="" draggable="false" loading={spinning ? "eager" : "lazy"} onerror={() => (failed = true)} />
  {/if}
  <span class="hole"></span>
</span>

<style>
  .record {
    position: relative;
    display: block;
    width: 100%;
    aspect-ratio: 1;
    border-radius: 50%;
    background:
      conic-gradient(
        from -70deg,
        transparent 0deg,
        rgb(255 255 255 / 0.12) 24deg,
        transparent 50deg 180deg,
        rgb(255 255 255 / 0.08) 204deg,
        transparent 230deg
      ),
      repeating-radial-gradient(circle, rgb(255 255 255 / 0.035) 0 1px, transparent 1px 2.6px),
      radial-gradient(circle closest-side, #121216 0 60%, #08080a 61% 96%, #2b2b31 98%, #050506 100%);
    box-shadow: 0 8px 22px rgb(0 0 0 / 0.55);
  }
  img {
    position: absolute;
    inset: calc((100% - var(--label)) / 2);
    width: var(--label);
    height: var(--label);
    border-radius: 50%;
    object-fit: cover;
  }
  .hole {
    position: absolute;
    inset: 47%;
    border-radius: 50%;
    background: radial-gradient(circle at 40% 35%, #3a3a40, #0b0b0d 70%);
  }
</style>
