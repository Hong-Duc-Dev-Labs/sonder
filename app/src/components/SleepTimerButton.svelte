<script lang="ts">
  // The sleep timer in the transport bar: a moon button with the minutes left beside it, and a
  // menu of durations. The timer itself runs in the backend, so it fires with the window closed.
  import { app } from "../lib/app.svelte";
  import { t } from "../lib/i18n.svelte";
  import { minutesLeft } from "../lib/stats";
  import Icon from "./Icon.svelte";
  import Popover from "./Popover.svelte";

  const CHOICES = [15, 30, 45, 60, 90];

  let open = $state(false);
  let button: HTMLButtonElement | undefined = $state();
  /** Wall clock, ticking while a timed timer runs, so the countdown moves. */
  let now = $state(Date.now());

  $effect(() => {
    if (app.sleep.endsAtMs == null) return;
    now = Date.now();
    const tick = window.setInterval(() => (now = Date.now()), 15_000);
    return () => window.clearInterval(tick);
  });

  let active = $derived(app.sleep.endsAtMs != null || app.sleep.endOfTrack);
  let left = $derived(app.sleep.endsAtMs != null ? minutesLeft(app.sleep.endsAtMs, now) : null);
  let label = $derived(
    left != null
      ? t("sleep.left", { m: left })
      : app.sleep.endOfTrack
        ? t("sleep.atEndOfTrack")
        : t("sleep.title"),
  );

  function choose(minutes: number | null, endOfTrack = false) {
    open = false;
    void app.setSleep(minutes, endOfTrack);
  }
</script>

<button
  bind:this={button}
  class="icon-btn sleep"
  class:on={active}
  aria-label={label}
  title={label}
  aria-haspopup="menu"
  aria-expanded={open}
  onclick={() => (open = !open)}
>
  <Icon name="moon" size={20} />
  {#if left != null}<span class="left">{left}′</span>{/if}
</button>

<Popover anchor={button} bind:open placement="top" menu label={t("sleep.title")}>
  <span class="kicker">{t("sleep.title")}</span>
  {#each CHOICES as minutes (minutes)}
    <button role="menuitem" onclick={() => choose(minutes)}>{t("sleep.minutes", { m: minutes })}</button>
  {/each}
  <button role="menuitem" disabled={app.current == null} onclick={() => choose(null, true)}>
    {t("sleep.endOfTrack")}
  </button>
  {#if active}
    <span class="sep" role="separator"></span>
    <button role="menuitem" onclick={() => choose(null)}>{t("sleep.off")}</button>
  {/if}
</Popover>

<style>
  .sleep {
    position: relative;
    width: auto;
    min-width: 40px;
    gap: 4px;
    padding: 0 8px;
  }
  .left {
    font-size: var(--fs-label);
    font-variant-numeric: tabular-nums;
  }
  .kicker {
    display: block;
    padding: 6px 12px 4px;
  }
  .sep {
    display: block;
    height: 1px;
    margin: 4px 8px;
    background: var(--hairline);
  }
</style>
