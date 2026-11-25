<script lang="ts">
  // Source format → device format, and whether the path is bit-perfect.
  import { app } from "../lib/app.svelte";
  import { isBitPerfect, outputLabel, sourceLabel } from "../lib/format";
  import { t } from "../lib/i18n.svelte";

  let snap = $derived(app.player);
  let perfect = $derived(isBitPerfect(snap));
  let isShared = $derived(Boolean(snap.signalPath?.shared));
</script>

{#if snap.source}
  <div class="badges">
    <span class="badge" title={t("badges.sourceTitle")}>{t("badges.source", { format: sourceLabel(snap.source) })}</span>
    {#if snap.output}
      <span class="arrow">→</span>
      <span class="badge accent" title={t("badges.deviceTitle", { label: snap.output.label })}>
        {t("badges.device", { format: outputLabel(snap.output) })}
      </span>
    {/if}
    {#if perfect}
      <span class="badge ok" title={t("badges.bitPerfectTitle")}>{t("common.bitPerfect")}</span>
    {:else if snap.volumeDb != null && snap.output}
      <span class="badge warn" title={t("badges.volumeTitle")}>{t("badges.volume", { db: snap.volumeDb.toFixed(1) })}</span>
    {/if}
    {#if isShared}
      <span class="badge warn" title={t("badges.sharedTitle")}>{t("badges.shared")}</span>
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
