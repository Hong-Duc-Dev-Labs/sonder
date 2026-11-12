<script lang="ts">
  // A band at the bottom of a hero image that melts it into the canvas: three blurred layers of
  // whatever is behind, each stronger and starting lower, then a fade to black. There is no
  // edge where the picture stops. With glass off it is a plain gradient.
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
    background: linear-gradient(
      to bottom,
      transparent 0%,
      rgb(0 0 0 / 0.35) 45%,
      rgb(0 0 0 / 0.8) 80%,
      var(--canvas) 100%
    );
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
