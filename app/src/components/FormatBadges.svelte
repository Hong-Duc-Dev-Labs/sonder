<script lang="ts">
  // Source format → device format, and whether the path is bit-perfect.
  import { app } from "../lib/app.svelte";
  import { isBitPerfect, outputLabel, sourceLabel } from "../lib/format";

  let snap = $derived(app.player);
  let perfect = $derived(isBitPerfect(snap));
</script>

{#if snap.source}
  <div class="badges">
    <span class="badge" title="Source file">Source {sourceLabel(snap.source)}</span>
    {#if snap.output}
      <span class="arrow">→</span>
      <span class="badge accent" title="Format the device was opened with ({snap.output.label})">
        Device {outputLabel(snap.output)}
      </span>
    {/if}
    {#if perfect}
      <span class="badge ok" title="The device receives the decoded samples unchanged">Bit-perfect</span>
    {:else if snap.volumeDb != null && snap.output}
      <span class="badge warn" title="Software volume changes the samples">Volume {snap.volumeDb.toFixed(1)} dB</span>
    {/if}
  </div>
{/if}

<style>
  .badges {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .arrow {
    color: var(--text-faint);
    font-size: 12px;
  }
</style>
