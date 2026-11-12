<script lang="ts">
  // A heavily blurred backdrop made from one cover or a mosaic of several. It is painted once
  // into a tiny canvas and scaled up by CSS, so scrolling a hero costs no blur work at all.
  // The covers come from another origin, which only taints the canvas for reading; drawing and
  // showing it is fine. Without canvas filters (older WebKit) the tiles are blurred by CSS.
  import { artUrl } from "../lib/art";

  let { hashes, tilt = 0 }: { hashes: (string | null | undefined)[]; tilt?: number } = $props();

  const SIDE = 96;
  let canvas: HTMLCanvasElement;
  let cssBlur = $state(false);
  let generation = 0;

  function load(src: string): Promise<HTMLImageElement | null> {
    return new Promise((resolve) => {
      const img = new Image();
      img.decoding = "async";
      img.onload = () => resolve(img);
      img.onerror = () => resolve(null);
      img.src = src;
    });
  }

  $effect(() => {
    const list = hashes.filter((h): h is string => !!h).slice(0, 6);
    const gen = ++generation;
    const ctx = canvas?.getContext("2d");
    if (!ctx) return;
    Promise.all(list.map((h) => load(artUrl(h, 300)!))).then((images) => {
      if (gen !== generation) return;
      const imgs = images.filter((i): i is HTMLImageElement => !!i);
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      if ("filter" in ctx) ctx.filter = "none";
      ctx.fillStyle = "#000";
      ctx.fillRect(0, 0, SIDE, SIDE);
      if (!imgs.length) return;
      const supportsFilter = "filter" in ctx;
      cssBlur = !supportsFilter;
      if (supportsFilter) ctx.filter = "blur(7px) saturate(1.4)";
      ctx.translate(SIDE / 2, SIDE / 2);
      ctx.rotate((tilt * Math.PI) / 180);
      ctx.scale(1.35, 1.35);
      ctx.translate(-SIDE / 2, -SIDE / 2);
      const cols = imgs.length === 1 ? 1 : imgs.length <= 4 ? 2 : 3;
      const rows = Math.ceil(imgs.length / cols);
      const w = SIDE / cols;
      const h = SIDE / rows;
      imgs.forEach((img, i) => ctx.drawImage(img, (i % cols) * w, Math.floor(i / cols) * h, w, h));
    });
  });
</script>

<canvas class="blurred" class:css-blur={cssBlur} width={SIDE} height={SIDE} bind:this={canvas} aria-hidden="true"
></canvas>

<style>
  .blurred {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .css-blur {
    filter: blur(24px) saturate(1.4);
  }
</style>
