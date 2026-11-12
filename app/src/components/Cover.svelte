<script lang="ts">
  import { artUrl, type ArtSize } from "../lib/art";

  let { hash, size = 300, alt = "" }: { hash: string | null | undefined; size?: ArtSize; alt?: string } =
    $props();
  let failed = $state(false);
  let src = $derived(artUrl(hash, size));
  $effect(() => {
    void src;
    failed = false;
  });
</script>

<div class="cover">
  {#if src && !failed}
    <img {src} {alt} loading="lazy" decoding="async" draggable="false" onerror={() => (failed = true)} />
  {:else}
    <!-- No art: a blank record label. -->
    <div class="placeholder" aria-hidden="true"><span class="label"></span></div>
  {/if}
</div>

<style>
  .cover {
    width: 100%;
    aspect-ratio: 1;
    border-radius: var(--r-sm);
    overflow: hidden;
    background: var(--soft);
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .placeholder {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    background:
      radial-gradient(circle at 30% 25%, rgb(255 255 255 / 0.08), transparent 60%),
      linear-gradient(135deg, #1c1c22, #0c0c0f);
  }
  .label {
    width: 46%;
    aspect-ratio: 1;
    border-radius: 50%;
    background:
      radial-gradient(circle, #0b0b0d 0 7%, transparent 7.5%),
      radial-gradient(circle at 35% 30%, #3a3a44, #22222a 70%);
    box-shadow:
      0 0 0 1px rgb(255 255 255 / 0.06),
      0 4px 14px rgb(0 0 0 / 0.5);
  }
</style>
