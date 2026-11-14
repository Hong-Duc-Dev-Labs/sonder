<script lang="ts">
  // Listening stats from the local play history: totals, a listening-time chart, the most
  // played tracks, artists and albums for a period, and the latest plays. Nothing here leaves
  // this computer.
  import { app } from "../lib/app.svelte";
  import { commands, type ListeningStatsInfo, type StatsPeriodSetting } from "../lib/bindings";
  import { durationShort } from "../lib/format";
  import { i18n, t } from "../lib/i18n.svelte";
  import { barHeights, barLabel, dateLocale, labelledBars } from "../lib/stats";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";

  const PERIODS: StatsPeriodSetting[] = ["week", "month", "year", "all"];

  let period = $state<StatsPeriodSetting>("month");
  let stats = $state<ListeningStatsInfo | null>(null);

  $effect(() => {
    app.pageWash = null;
    const asked = period;
    void app.historyVersion;
    void app.libraryVersion;
    app.run(commands.historyStats(asked)).then((r) => {
      if (r && asked === period) stats = r;
    });
  });

  let heights = $derived(barHeights((stats?.bars ?? []).map((b) => b.listenedMs)));
  let labelled = $derived(labelledBars(stats?.bars.length ?? 0));

  const listened = (ms: number) => (ms > 0 ? durationShort(ms) : "—");
  const playsLabel = (n: number) => t("stats.plays", { count: n });

  function playedAt(seconds: number): string {
    return new Intl.DateTimeFormat(dateLocale(i18n.locale), {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(seconds * 1000));
  }

  async function clearHistory() {
    const confirmed = await app.ask({
      title: t("stats.clearTitle"),
      message: t("stats.clearMessage"),
      confirm: t("stats.clear"),
      danger: true,
    });
    if (confirmed == null) return;
    const removed = await app.run(commands.historyClear());
    if (removed != null) app.notify("info", t("stats.cleared", { count: removed }));
  }
</script>

<div class="page scroll">
  <header class="head">
    <div>
      <h1 tabindex="-1" data-view-heading>{t("stats.title")}</h1>
      <p class="dim small">{t("stats.local")}</p>
    </div>
    <div class="periods" role="group" aria-label={t("stats.period")}>
      {#each PERIODS as p (p)}
        <button class="chip" aria-pressed={period === p} onclick={() => (period = p)}>{t(`stats.period.${p}`)}</button>
      {/each}
    </div>
  </header>

  {#if stats}
    <div class="totals">
      <div class="total soft">
        <span class="kicker">{t("stats.listened")}</span>
        <span class="figure">{listened(stats.listenedMs)}</span>
      </div>
      <div class="total soft">
        <span class="kicker">{t("stats.playsTitle")}</span>
        <span class="figure">{stats.plays}</span>
      </div>
      <div class="total soft">
        <span class="kicker">{t("stats.tracks")}</span>
        <span class="figure">{stats.tracks}</span>
      </div>
    </div>

    {#if stats.bars.length}
      <section class="chart soft" aria-labelledby="stats-chart">
        <h2 id="stats-chart" class="kicker">{t("stats.chart")}</h2>
        <div class="bars" style:--count={stats.bars.length}>
          {#each stats.bars as bar, i (bar.label)}
            <div class="bar-col" title="{barLabel(bar.label, i18n.locale)}: {listened(bar.listenedMs)}">
              <span class="bar" style:height="{Math.max(heights[i] * 100, bar.listenedMs ? 2 : 0)}%"></span>
            </div>
          {/each}
        </div>
        <div class="bar-labels" aria-hidden="true" style:--count={stats.bars.length}>
          {#each stats.bars as bar, i (bar.label)}
            <span>{labelled.has(i) ? barLabel(bar.label, i18n.locale) : ""}</span>
          {/each}
        </div>
      </section>
    {/if}

    {#if !stats.plays}
      <p class="dim empty">{t("stats.empty")}</p>
    {:else}
      <div class="tops">
        <section aria-labelledby="stats-tracks">
          <h2 id="stats-tracks">{t("stats.topTracks")}</h2>
          <ol class="rank soft">
            {#each stats.topTracks as item, i (item.track.id)}
              <li>
                <span class="n">{i + 1}</span>
                <span class="thumb"><Cover hash={item.track.art} alt="" /></span>
                <button
                  class="what"
                  title={t("common.playItem", { title: item.track.title })}
                  onclick={() => app.run(commands.playerPlayTracks([item.track.id], 0))}
                >
                  <span class="t">{item.track.title}</span>
                  <span class="sub">{item.track.artist ?? item.track.albumArtist ?? ""}</span>
                </button>
                <span class="count">{playsLabel(item.plays)}</span>
              </li>
            {/each}
          </ol>
        </section>

        <section aria-labelledby="stats-artists">
          <h2 id="stats-artists">{t("stats.topArtists")}</h2>
          <ol class="rank soft">
            {#each stats.topArtists as item, i (item.id)}
              <li>
                <span class="n">{i + 1}</span>
                <span class="thumb round"><Cover hash={item.art} alt="" /></span>
                <button class="what" onclick={() => app.go({ kind: "artist", id: item.id })}>
                  <span class="t">{item.name}</span>
                  <span class="sub">{listened(item.listenedMs)}</span>
                </button>
                <span class="count">{playsLabel(item.plays)}</span>
              </li>
            {/each}
          </ol>
        </section>

        <section aria-labelledby="stats-albums">
          <h2 id="stats-albums">{t("stats.topAlbums")}</h2>
          <ol class="rank soft">
            {#each stats.topAlbums as item, i (item.id)}
              <li>
                <span class="n">{i + 1}</span>
                <span class="thumb"><Cover hash={item.art} alt="" /></span>
                <button class="what" onclick={() => app.go({ kind: "album", id: item.id })}>
                  <span class="t">{item.title}</span>
                  <span class="sub">{item.artist ?? ""}</span>
                </button>
                <span class="count">{playsLabel(item.plays)}</span>
              </li>
            {/each}
          </ol>
        </section>
      </div>
    {/if}

    {#if stats.recent.length}
      <section aria-labelledby="stats-recent">
        <h2 id="stats-recent">{t("stats.recent")}</h2>
        <ul class="rank soft">
          {#each stats.recent as item, i (`${item.track.id}-${item.playedAt}-${i}`)}
            <li>
              <span class="thumb"><Cover hash={item.track.art} alt="" /></span>
              <button
                class="what"
                title={t("common.playItem", { title: item.track.title })}
                onclick={() => app.run(commands.playerPlayTracks([item.track.id], 0))}
              >
                <span class="t">{item.track.title}</span>
                <span class="sub">{item.track.artist ?? item.track.albumArtist ?? ""}</span>
              </button>
              <span class="count">{playedAt(item.playedAt)}</span>
            </li>
          {/each}
        </ul>
        <button class="btn small clear" onclick={clearHistory}><Icon name="trash" size={16} /> {t("stats.clear")}</button>
      </section>
    {/if}
  {/if}
</div>

<style>
  .head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 16px;
    margin-bottom: 18px;
  }
  h1 {
    font-size: 40px;
    font-weight: 800;
    letter-spacing: -0.035em;
  }
  h2 {
    font-size: var(--fs-title);
    margin: 0 0 10px;
  }
  .small {
    font-size: var(--fs-small);
    margin: 4px 0 0;
  }
  .periods {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .totals {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: 12px;
    margin-bottom: 16px;
  }
  .total {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 16px 18px;
    border-radius: var(--r-md);
  }
  .figure {
    font-family: var(--font-figures);
    font-size: 28px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .chart {
    padding: 16px 18px 12px;
    border-radius: var(--r-md);
    margin-bottom: 24px;
  }
  .chart h2 {
    margin-bottom: 12px;
  }
  .bars,
  .bar-labels {
    display: grid;
    grid-template-columns: repeat(var(--count), minmax(0, 1fr));
    gap: 3px;
  }
  .bars {
    height: 140px;
    align-items: end;
  }
  .bar-col {
    height: 100%;
    display: flex;
    align-items: flex-end;
  }
  .bar {
    width: 100%;
    border-radius: 3px 3px 0 0;
    background: var(--ink);
    opacity: 0.85;
  }
  .bar-col:hover .bar {
    opacity: 1;
  }
  .bar-labels {
    margin-top: 6px;
    font-size: var(--fs-label);
    color: var(--ink-3);
    white-space: nowrap;
  }
  .bar-labels span {
    overflow: visible;
  }
  .empty {
    margin: 24px 0;
  }
  .tops {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 20px;
    margin-bottom: 24px;
  }
  .rank {
    list-style: none;
    margin: 0;
    padding: 6px;
    border-radius: var(--r-md);
  }
  .rank li {
    display: grid;
    grid-template-columns: auto 40px minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border-radius: var(--r-sm);
  }
  ul.rank li {
    grid-template-columns: 40px minmax(0, 1fr) auto;
  }
  .rank li:hover {
    background: var(--soft-hover);
  }
  .n {
    width: 1.6em;
    text-align: right;
    color: var(--ink-3);
    font-variant-numeric: tabular-nums;
  }
  .thumb {
    width: 40px;
    height: 40px;
    border-radius: var(--r-xs);
    overflow: hidden;
    background: var(--soft);
  }
  .thumb.round {
    border-radius: 50%;
  }
  .what {
    display: flex;
    flex-direction: column;
    min-width: 0;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .t,
  .sub {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .t {
    font-weight: 600;
  }
  .sub,
  .count {
    font-size: var(--fs-small);
    color: var(--ink-2);
  }
  .count {
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .clear {
    margin-top: 12px;
  }
</style>
