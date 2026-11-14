<script lang="ts">
  // A track's quality verdict in a list. Problems show as a word; a genuine track as a small
  // check, so a checked album does not turn into a wall of labels. Clicking opens the report.
  import { app } from "../lib/app.svelte";
  import type { TrackItem } from "../lib/bindings";
  import { t } from "../lib/i18n.svelte";
  import { verdictKey, verdictShortKey, verdictTone } from "../lib/quality";
  import Icon from "./Icon.svelte";

  let { track }: { track: TrackItem } = $props();
  let badge = $derived(app.qualityOf(track));
</script>

{#if badge}
  {@const tone = verdictTone(badge.verdict)}
  {@const label = `${t(verdictKey(badge.verdict))} · ${t("quality.confidence", { level: t(`quality.confidence.${badge.confidence}`) })}`}
  <button
    type="button"
    class="quality tone-{tone}"
    class:icon={badge.verdict === "genuine"}
    title={label}
    aria-label={label}
    onclick={(e) => {
      e.stopPropagation();
      app.checkQuality(track);
    }}
    ondblclick={(e) => e.stopPropagation()}
    onpointerdown={(e) => e.stopPropagation()}
  >
    {#if badge.verdict === "genuine"}
      <Icon name="check" size={14} />
    {:else}
      {t(verdictShortKey(badge.verdict))}
    {/if}
  </button>
{/if}

<style>
  .quality {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 1px 7px;
    border: none;
    border-radius: var(--r-pill);
    font: inherit;
    font-size: var(--fs-label);
    font-weight: 600;
    color: var(--tone);
    background: color-mix(in srgb, var(--tone) 16%, transparent);
    cursor: pointer;
    white-space: nowrap;
  }
  .quality.icon {
    padding: 2px;
    background: transparent;
  }
  .quality:hover {
    background: color-mix(in srgb, var(--tone) 26%, transparent);
  }
  .tone-ok {
    --tone: var(--ok);
  }
  .tone-warn {
    --tone: var(--warn);
  }
  .tone-bad {
    --tone: var(--error);
  }
  .tone-faint {
    --tone: var(--ink-3);
  }
</style>
