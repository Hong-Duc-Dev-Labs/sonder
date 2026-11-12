<script lang="ts">
  // Play and pause as the same two shapes: each pause bar folds into one half of the play
  // triangle, so switching morphs instead of swapping. Webviews that cannot animate the `d`
  // property simply switch shapes.
  let { playing, size = 22 }: { playing: boolean; size?: number } = $props();

  const PAUSE = ["M7 4 L10.5 4 L10.5 20 L7 20 Z", "M13.5 4 L17 4 L17 20 L13.5 20 Z"];
  const PLAY = ["M8 4.5 L14 8.25 L14 15.75 L8 19.5 Z", "M14 8.25 L20 12 L20 12 L14 15.75 Z"];

  let shapes = $derived(playing ? PAUSE : PLAY);
</script>

<svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
  {#each shapes as d, i (i)}
    <path {d} style:d="path('{d}')" />
  {/each}
</svg>

<style>
  path {
    transition: d var(--dur-med) var(--ease-out);
  }
</style>
