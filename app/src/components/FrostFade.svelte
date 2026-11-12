<script lang="ts">
  // A band at the bottom of a hero image that melts it into the canvas: three blurred layers of
  // whatever is behind, each stronger and starting lower, under a soft darkening. There is no
  // edge where the picture stops. With glass off it is a plain gradient. Pair it with a mask
  // on its container when the content below is not black.
  let { height = "45%" }: { height?: string } = $props();
</script>

<div class="frost" style:height aria-hidden="true">
  <div class="band b1"></div>
  <div class="band b2"></div>
  <div class="band b3"></div>
  <div class="scrim"></div>
</div>

<style>
  .frost {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    pointer-events: none;
  }
  .band {
    position: absolute;
    inset: 0;
  }
  .b1 {
    -webkit-backdrop-filter: blur(4px);
    backdrop-filter: blur(4px);
    mask-image: linear-gradient(to bottom, transparent 0%, #000 40%);
  }
  .b2 {
    -webkit-backdrop-filter: blur(14px);
    backdrop-filter: blur(14px);
    mask-image: linear-gradient(to bottom, transparent 25%, #000 70%);
  }
  .b3 {
    -webkit-backdrop-filter: blur(32px);
    backdrop-filter: blur(32px);
    mask-image: linear-gradient(to bottom, transparent 50%, #000 100%);
  }
  .scrim {
    position: absolute;
    inset: 0;
    /* Darkens for legible text but never to solid black: what lies below is the cover-lit
       canvas, and an opaque end would draw a line where the two meet. */
    background: linear-gradient(to bottom, transparent 0%, rgb(0 0 0 / 0.3) 50%, rgb(0 0 0 / 0.45) 100%);
  }
  :global([data-glass="off"]) .band,
  :global([data-glass="reduced"]) .b1 {
    display: none;
  }
  @media (prefers-reduced-transparency: reduce) {
    .band {
      display: none;
    }
  }
</style>
