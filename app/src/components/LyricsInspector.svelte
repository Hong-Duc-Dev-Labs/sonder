<script lang="ts">
  // Lyrics in the inspector column, for every page but Now Playing (which shows them in place
  // of its title). Escape closes it and returns focus to the lyrics button that opened it.
  import { app } from "../lib/app.svelte";
  import { t } from "../lib/i18n.svelte";
  import Icon from "./Icon.svelte";
  import LyricsPanel from "./LyricsPanel.svelte";

  let item = $derived(app.current);

  function close() {
    app.closeInspector();
  }
</script>

<!-- svelte:window in App.svelte closes it on Escape, so focus outside the panel is covered. -->
<aside class="panel glass" aria-label={t("common.lyrics")}>
  <div class="head">
    <div class="titles">
      <h2>{t("common.lyrics")}</h2>
      {#if item}
        {@const line = `${item.title} · ${item.artist ?? t("common.unknownArtist")}`}
        <span class="sub" title={line}>{line}</span>
      {/if}
    </div>
    <button class="icon-btn" aria-label={t("lyrics.close")} onclick={close}><Icon name="close" size={18} /></button>
  </div>
  <div class="body">
    <LyricsPanel />
  </div>
</aside>

<style>
  .panel {
    width: 100%;
    height: 100%;
    border-radius: var(--r-lg);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding: 20px 16px 4px 20px;
  }
  .titles {
    min-width: 0;
  }
  h2 {
    font-size: 22px;
    font-weight: 800;
  }
  .sub {
    display: block;
    font-size: var(--fs-small);
    color: var(--ink-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .body {
    container-type: inline-size;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 0 10px 14px;
    /* On glass over a bright cover the default dim lines fall under 3:1; these keep unsung
       lines near 4.5:1 while the sung line still stands out at full white. */
    --lyric-idle: 0.5;
    --lyric-past: 0.62;
  }
</style>
