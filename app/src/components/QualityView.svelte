<script lang="ts">
  // The quality check page: pick any song (from the library, the one playing, or a file on
  // disk) and read its report. A library track shows its cached verdict at once and then runs
  // a fresh check, which also draws the charts the cache leaves out; a picked file outside the
  // library is checked without saving. Leaving the page stops a check that is still running.
  import { onMount } from "svelte";
  import { app, type QualityTarget } from "../lib/app.svelte";
  import { commands, events, type QualityReport, type TrackItem } from "../lib/bindings";
  import { duration, fold, rate } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import {
    criterionKey,
    evidenceValue,
    heat,
    linearTicks,
    reasonKey,
    statusKey,
    statusTone,
    tally,
    tickLabel,
    verdictKey,
    verdictTone,
  } from "../lib/quality";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";
  import QualityBadge from "./QualityBadge.svelte";

  /** Rows the picker draws; typing narrows the rest. */
  const PICKER_ROWS = 150;
  /** Progress events of a picked file carry this id (library ids start at 1). */
  const PICKED_FILE_ID = 0;
  const FLOOR_DB = -120;
  const W = 600;
  const H = 200;

  let tracks = $state<TrackItem[] | null>(null);
  let filter = $state("");
  let canvas: HTMLCanvasElement | undefined = $state();

  let report = $state<QualityReport | null>(null);
  let running = $state(false);
  let progress = $state(0);
  let error = $state<string | null>(null);
  /** The target on screen, as a key; a different one starts over. */
  let shown: string | null = null;
  /** Bumped on every run, so the answer of an older run is dropped. */
  let runs = 0;
  /** The check in flight; a new one waits for it to stop so they never overlap. */
  let pending: Promise<unknown> = Promise.resolve();

  $effect(() => {
    app.pageWash = null;
    void app.libraryVersion;
    app.run(commands.libraryTracks()).then((r) => {
      if (r) tracks = r;
    });
  });

  onMount(() => {
    const listening = events.qualityProgressEvent.listen((e) => {
      if (running && e.payload.trackId === progressId(app.qualityTarget)) progress = e.payload.fraction ?? 0;
    });
    return () => {
      void listening.then((stop) => stop());
      runs++;
      if (running) void commands.qualityCancelCheck();
    };
  });

  const keyOf = (target: QualityTarget) => (target.kind === "track" ? `t${target.track.id}` : `f${target.file.path}`);
  const progressId = (target: QualityTarget | null) =>
    target?.kind === "track" ? target.track.id : target ? PICKED_FILE_ID : null;

  $effect(() => {
    const target = app.qualityTarget;
    if (!target) return;
    const key = keyOf(target);
    if (shown === key) return;
    shown = key;
    report = null;
    error = null;
    void open(target, key);
  });

  async function open(target: QualityTarget, key: string) {
    if (target.kind === "track") {
      const cached = await commands.qualityCached(target.track.id).catch(() => null);
      if (shown === key && cached && !report) report = cached;
    }
    await check(target, key);
  }

  async function check(target: QualityTarget, key: string) {
    const run = ++runs;
    if (running) void commands.qualityCancelCheck();
    // The backend resets the cancel flag when a check starts: let the old one stop first.
    await pending;
    if (run !== runs) return;
    running = true;
    progress = 0;
    error = null;
    const work = target.kind === "track" ? commands.qualityCheckTrack(target.track.id) : commands.qualityCheckFile();
    pending = work.catch(() => {});
    try {
      const fresh = await work;
      if (run === runs && shown === key) report = fresh;
    } catch (err) {
      const message = typeof err === "string" ? err : String(err);
      if (run === runs && shown === key && message !== "cancelled") error = message;
    } finally {
      if (run === runs) running = false;
    }
  }

  function recheck() {
    const target = app.qualityTarget;
    if (target) void check(target, keyOf(target));
  }

  function pick(track: TrackItem) {
    app.qualityTarget = { kind: "track", track };
  }

  let playing = $derived.by(() => {
    const id = app.current?.trackId;
    return id != null ? (tracks?.find((tr) => tr.id === id) ?? null) : null;
  });

  /** Each track's searchable text, folded once per load rather than on every keystroke. */
  let haystacks = $derived(
    (tracks ?? []).map((tr) => fold([tr.title, tr.artist, tr.albumArtist, tr.album].filter(Boolean).join(" "))),
  );
  let matching = $derived.by(() => {
    const terms = fold(filter).split(/\s+/).filter(Boolean);
    const all = tracks ?? [];
    if (!terms.length) return all;
    return all.filter((_, i) => terms.every((term) => haystacks[i].includes(term)));
  });
  let listed = $derived(matching.slice(0, PICKER_ROWS));
  let selectedId = $derived(app.qualityTarget?.kind === "track" ? app.qualityTarget.track.id : null);

  // ---- report presentation ----

  let topHz = $derived(report?.spectrumTopHz ?? (report ? report.source.rate / 2 : 22_050));
  const xAt = (hz: number) => Math.min(1, Math.max(0, hz / topHz)) * W;
  const yAt = (db: number) => (Math.max(db, FLOOR_DB) / FLOOR_DB) * H;

  let trace = $derived.by(() => {
    if (!report?.spectrum.length) return "";
    return report.spectrum
      .map((p, i) => `${i ? "L" : "M"}${xAt(p.hz ?? 0).toFixed(1)},${yAt(p.db ?? FLOOR_DB).toFixed(1)}`)
      .join("");
  });
  let area = $derived(trace ? `${trace}L${W},${H}L0,${H}Z` : "");

  let counts = $derived(report ? tally(report.evidence) : null);

  let format = $derived.by(() => {
    const target = app.qualityTarget;
    if (target?.kind === "track") return target.track.codec.toUpperCase();
    return target?.file.name.split(".").pop()?.toUpperCase() ?? "";
  });

  let params = $derived.by(() => {
    if (!report) return [];
    const s = report.source;
    return [
      { label: t("quality.param.format"), value: format },
      { label: t("quality.param.rate"), value: s.dsd ? `DSD ${(s.rate / 1e6).toFixed(1)} MHz` : rate(s.rate) },
      { label: t("quality.param.bits"), value: s.dsd ? "1-bit" : `${s.bits}-bit` },
      { label: t("quality.param.cutoff"), value: report.cutoffHz ? rate(Math.round(report.cutoffHz / 100) * 100) : "—" },
      { label: t("quality.param.channels"), value: String(s.channels) },
      { label: t("quality.param.duration"), value: duration((report.durationSecs ?? 0) * 1000) },
    ];
  });

  let mapTop = $derived(report?.spectrogram?.topHz ?? 22_050);
  let mapTicks = $derived(report?.spectrogram ? linearTicks(mapTop).filter((_, i) => i % 2 === 1) : []);

  /** The cutoff track over the time map, broken where no loud frame fell. */
  let cutoffPath = $derived.by(() => {
    const map = report?.spectrogram;
    if (!report || !map) return "";
    let d = "";
    let pen = false;
    report.cutoffTrack.forEach((hz, i) => {
      if (!hz) {
        pen = false;
        return;
      }
      const x = ((i + 0.5) / map.cols) * W;
      const y = (1 - Math.min(hz, mapTop) / mapTop) * H;
      d += `${pen ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`;
      pen = true;
    });
    return d;
  });

  $effect(() => {
    const map = report?.spectrogram;
    if (!canvas || !map) return;
    canvas.width = map.cols;
    canvas.height = map.rows;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const image = ctx.createImageData(map.cols, map.rows);
    for (let row = 0; row < map.rows; row++) {
      // Row 0 is the lowest frequency: draw it at the bottom.
      const y = map.rows - 1 - row;
      for (let col = 0; col < map.cols; col++) {
        const [r, g, b] = heat(map.db[row * map.cols + col]);
        const at = (y * map.cols + col) * 4;
        image.data[at] = r;
        image.data[at + 1] = g;
        image.data[at + 2] = b;
        image.data[at + 3] = 255;
      }
    }
    ctx.putImageData(image, 0, 0);
  });
</script>

<div class="page scroll">
  <header class="head">
    <h1 tabindex="-1" data-view-heading>{t("quality.title")}</h1>
    <div class="tools">
      <button class="btn" disabled={!playing} onclick={() => playing && pick(playing)}>
        <Icon name="now" size={18} /> {t("quality.checkPlaying")}
      </button>
      <button class="btn primary" onclick={() => app.pickQualityFile(tracks ?? [])}>
        <Icon name="folder" size={18} /> {t("quality.pickFile")}
      </button>
    </div>
  </header>

  <div class="layout">
    <aside class="picker soft" aria-label={t("quality.library")}>
      <div class="field filter">
        <Icon name="search" size={18} />
        <input placeholder={t("quality.filter")} aria-label={t("quality.filter")} bind:value={filter} />
      </div>
      {#if tracks && !tracks.length}
        <p class="dim small pad">{t("songs.none")}</p>
      {:else if tracks && !matching.length}
        <p class="dim small pad">{t("songs.noMatch", { filter: filter.trim() })}</p>
      {:else}
        <ul class="rows">
          {#each listed as track (track.id)}
            <li class:active={selectedId === track.id}>
              <button class="row" aria-current={selectedId === track.id ? "true" : undefined} onclick={() => pick(track)}>
                <span class="row-art"><Cover hash={track.art} alt="" /></span>
                <span class="row-text">
                  <span class="row-title">{track.title}</span>
                  <span class="row-sub">{track.artist ?? t("common.unknownArtist")}</span>
                </span>
              </button>
              <QualityBadge {track} />
            </li>
          {/each}
        </ul>
        {#if listed.length < matching.length}
          <p class="dim small pad">{t("quality.moreMatches", { shown: listed.length, total: matching.length })}</p>
        {/if}
      {/if}
    </aside>

    <section class="report" aria-live="polite">
      {#if !app.qualityTarget}
        <div class="empty soft">
          <span class="empty-icon"><Icon name="verified" size={40} /></span>
          <h2>{t("quality.emptyTitle")}</h2>
          <p class="dim">{t("quality.emptyHint")}</p>
        </div>
      {:else}
        {@const target = app.qualityTarget}
        {@const revealPath = target.kind === "track" ? target.track.path : target.file.path}
        <div class="subject">
          <span class="subject-art">
            {#if target.kind === "track"}
              <Cover hash={target.track.art} alt="" />
            {:else}
              <span class="file-art"><Icon name="note" size={28} /></span>
            {/if}
          </span>
          <div class="subject-text">
            {#if target.kind === "track"}
              <h2 title={target.track.path}>{target.track.title}</h2>
              <p class="dim">{target.track.artist ?? t("common.unknownArtist")} · {target.track.album}</p>
            {:else}
              <h2 title={target.file.path}>{target.file.name}</h2>
              <p class="dim small path" title={target.file.path}>{target.file.path}</p>
            {/if}
          </div>
          <div class="subject-actions">
            <button
              type="button"
              class="btn small"
              aria-label={t("menu.revealInFolder")}
              title={revealPath}
              onclick={() => void app.revealInFolder(revealPath)}
            >
              <Icon name="folder-open" size={16} /> {t("menu.revealInFolder")}
            </button>
            {#if running}
              <button type="button" class="btn small" onclick={() => commands.qualityCancelCheck()}>{t("quality.cancel")}</button>
            {:else}
              <button type="button" class="btn small" onclick={recheck}><Icon name="refresh" size={16} /> {t("quality.recheck")}</button>
            {/if}
          </div>
        </div>
        {#if target.kind === "file"}
          <p class="dim small">{t("quality.outsideLibrary")}</p>
        {/if}

        {#if running}
          <div class="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(progress * 100)}>
            <span style:width="{(progress * 100).toFixed(1)}%"></span>
          </div>
          <p class="dim small">{t("quality.checking", { pct: Math.round(progress * 100) })}</p>
        {/if}
        {#if error}
          <p class="error" role="alert">{t("quality.failed", { error })}</p>
        {/if}

        {#if report}
          {@const tone = verdictTone(report.verdict)}
          <div class="verdict tone-{tone}">
            <span class="verdict-icon"><Icon name={report.verdict === "genuine" ? "verified" : "alert"} size={44} /></span>
            <div class="verdict-text">
              <h3>{t(verdictKey(report.verdict))}</h3>
              <p>{t(`quality.summary.${report.verdict}`)}</p>
              <p class="confidence">{t("quality.confidence", { level: t(`quality.confidence.${report.confidence}`) })}</p>
            </div>
          </div>

          <section class="block" aria-labelledby="q-spectrum">
            <h3 id="q-spectrum" class="kicker">{t("quality.spectrumKhz")}</h3>
            {#if !report.spectrum.length}
              <p class="dim small">{t("quality.chartsWaiting")}</p>
            {:else}
              <div class="well tone-{tone}">
                <svg viewBox="0 0 {W} {H + 20}" role="img" aria-label={t("quality.spectrum")}>
                  {#each [-40, -80] as db (db)}
                    <line class="grid" x1="0" x2={W} y1={yAt(db)} y2={yAt(db)} />
                    <text class="tick" x="4" y={yAt(db) - 4}>{db} dB</text>
                  {/each}
                  {#each linearTicks(topHz) as hz (hz)}
                    <line class="grid" x1={xAt(hz)} x2={xAt(hz)} y1="0" y2={H} />
                    <text class="tick" x={xAt(hz)} y={H + 15} text-anchor="middle">{hz / 1000}</text>
                  {/each}
                  {#if topHz > 20_500}
                    <line class="mark" x1={xAt(20_000)} x2={xAt(20_000)} y1="0" y2={H} />
                  {/if}
                  <path class="area" d={area} />
                  <path class="trace" d={trace} />
                  {#if report.cutoffHz}
                    {@const cx = xAt(report.cutoffHz)}
                    <line class="cutoff" x1={cx} x2={cx} y1="0" y2={H} />
                    <text class="cutoff-label" x={cx - 6} y="14" text-anchor="end">{(report.cutoffHz / 1000).toFixed(1)} kHz</text>
                  {/if}
                </svg>
              </div>
              <p class="dim small">{t("quality.spectrumHint")}</p>
            {/if}
          </section>

          <section class="block" aria-labelledby="q-measured">
            <h3 id="q-measured" class="kicker">{t("quality.measured")}</h3>
            <dl class="params soft">
              {#each params as p (p.label)}
                <div>
                  <dt class="dim">{p.label}</dt>
                  <dd>{p.value}</dd>
                </div>
              {/each}
            </dl>
          </section>

          <section class="block" aria-labelledby="q-evidence">
            <div class="block-head">
              <h3 id="q-evidence" class="kicker">{t("quality.evidence")}</h3>
              {#if counts}
                <span class="dim small">
                  {t("quality.tally", { pass: counts.pass, warn: counts.warn, fail: counts.fail })}
                  {#if counts.notMeasured}· {t("quality.notMeasuredCount", { count: counts.notMeasured })}{/if}
                </span>
              {/if}
            </div>
            <ul class="evidence soft">
              {#each report.evidence as e (e.id)}
                {@const stone = statusTone(e.status)}
                <li>
                  <div class="ev-text">
                    <span class="ev-name">{t(criterionKey(e.id))}</span>
                    <span class="ev-value">{evidenceValue(e) || "—"}</span>
                    <span class="dim small">{t(reasonKey(e))}</span>
                  </div>
                  <span class="status tone-{stone}" class:outline={e.status === "notMeasured"}>{t(statusKey(e.status))}</span>
                </li>
              {/each}
            </ul>
          </section>

          <section class="block" aria-labelledby="q-map">
            <h3 id="q-map" class="kicker">{t("quality.spectrogram")}</h3>
            {#if report.spectrogram}
              <div class="well map">
                <canvas bind:this={canvas} aria-hidden="true"></canvas>
                <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" role="img" aria-label={t("quality.spectrogram")}>
                  {#each mapTicks as hz (hz)}
                    {@const y = (1 - hz / mapTop) * H}
                    <line class="grid" x1="0" x2={W} y1={y} y2={y} />
                  {/each}
                  <path class="cutoff-track" d={cutoffPath} vector-effect="non-scaling-stroke" />
                </svg>
                <div class="map-ticks" aria-hidden="true">
                  {#each mapTicks as hz (hz)}
                    <span style:bottom="{(hz / mapTop) * 100}%">{tickLabel(hz)}</span>
                  {/each}
                </div>
              </div>
              <p class="dim small">{t("quality.spectrogramHint")}</p>
            {:else}
              <p class="dim small">{t("quality.chartsWaiting")}</p>
            {/if}
          </section>

          <div class="notes">
            <p class="note soft">{t("quality.notMeasuredNote")}</p>
            <p class="note soft">{t("quality.estimate")}</p>
          </div>
        {/if}
      {/if}
    </section>
  </div>
</div>

<style>
  .head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 16px;
    margin-bottom: 20px;
  }
  h1 {
    font-size: 40px;
    font-weight: 800;
    letter-spacing: -0.035em;
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }
  .small {
    font-size: var(--fs-small);
    margin: 0;
  }
  .layout {
    display: grid;
    grid-template-columns: minmax(260px, 320px) minmax(0, 1fr);
    gap: 20px;
    align-items: start;
  }
  @media (max-width: 1000px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
    }
    .picker {
      position: static;
      max-height: 360px;
    }
  }

  /* ---- picker ---- */
  .picker {
    position: sticky;
    top: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: calc(100vh - 220px);
    padding: 10px;
    border-radius: var(--r-md);
    overflow: hidden;
  }
  .filter input {
    height: 42px;
  }
  .pad {
    padding: 4px 8px;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: rgb(255 255 255 / 0.18) transparent;
  }
  .rows li {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-right: 8px;
    border-radius: var(--r-sm);
    transition: background var(--dur-fast) var(--ease-standard);
  }
  .rows li:hover {
    background: var(--soft-hover);
  }
  .rows li.active {
    background: var(--soft-active);
    box-shadow: inset 0 0 0 1px var(--hairline);
  }
  .row {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    text-align: left;
  }
  .row-art {
    width: 36px;
    flex: none;
  }
  .row-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .row-title,
  .row-sub {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .row-title {
    font-weight: 600;
  }
  .row-sub {
    font-size: var(--fs-small);
    color: var(--ink-2);
  }

  /* ---- report ---- */
  .report {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 56px 32px;
    border-radius: var(--r-lg);
    text-align: center;
  }
  .empty p {
    max-width: 460px;
    margin: 0;
  }
  .empty-icon {
    color: var(--ok);
  }
  .subject {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .subject-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    flex: none;
  }
  .subject-art {
    width: 72px;
    flex: none;
  }
  .file-art {
    display: grid;
    place-items: center;
    aspect-ratio: 1;
    border-radius: var(--r-sm);
    background: var(--soft);
    color: var(--ink-2);
  }
  .subject-text {
    flex: 1;
    min-width: 0;
  }
  .subject-text h2 {
    font-size: 22px;
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .subject-text p {
    margin: 2px 0 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .path {
    direction: rtl;
    text-align: left;
  }
  .error {
    color: var(--error);
    margin: 0;
  }
  .progress {
    height: 4px;
    border-radius: var(--r-pill);
    background: var(--soft);
    overflow: hidden;
  }
  .progress span {
    display: block;
    height: 100%;
    background: var(--ink);
    transition: width var(--dur-fast) linear;
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
  .verdict {
    display: flex;
    align-items: center;
    gap: 22px;
    padding: 22px 26px;
    border-radius: var(--r-lg);
    background: color-mix(in srgb, var(--tone) 9%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--tone) 40%, transparent);
  }
  .verdict-icon {
    flex: none;
    color: var(--tone);
  }
  .verdict-text {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .verdict h3 {
    font-size: 22px;
    font-weight: 750;
    color: var(--tone);
  }
  .verdict p {
    margin: 0;
    color: var(--ink-2);
  }
  .verdict .confidence {
    color: var(--tone);
    font-weight: 600;
  }

  .block {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .block-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 8px;
  }

  /* Chart wells stay dark in every theme so the heat ramp reads the same. */
  .well {
    position: relative;
    border-radius: var(--r-md);
    background: #0b0b0f;
    padding: 12px 12px 4px;
  }
  .well svg {
    display: block;
    width: 100%;
    height: auto;
  }
  .grid {
    stroke: rgb(255 255 255 / 0.08);
    stroke-width: 1;
  }
  .tick {
    fill: rgb(255 255 255 / 0.45);
    font-size: 10px;
  }
  .mark {
    stroke: rgb(255 255 255 / 0.22);
    stroke-width: 1;
  }
  .area {
    fill: color-mix(in srgb, var(--tone) 16%, transparent);
  }
  .trace {
    fill: none;
    stroke: var(--tone);
    stroke-width: 1.6;
    stroke-linejoin: round;
  }
  .cutoff {
    stroke: var(--ink);
    stroke-width: 1.5;
    stroke-dasharray: 5 4;
  }
  .cutoff-label {
    fill: var(--ink);
    font-size: 11px;
  }

  .params {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 0 28px;
    margin: 0;
    padding: 8px 20px;
    border-radius: var(--r-md);
  }
  .params div {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 0;
    border-bottom: 1px solid var(--hairline);
  }
  .params dd {
    margin: 0;
    font-family: var(--font-figures);
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }

  .evidence {
    list-style: none;
    margin: 0;
    padding: 4px 18px;
    border-radius: var(--r-md);
  }
  .evidence li {
    display: flex;
    align-items: flex-start;
    gap: 16px;
    padding: 12px 0;
  }
  .evidence li + li {
    border-top: 1px solid var(--hairline);
  }
  .ev-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .ev-name {
    font-weight: 600;
  }
  .ev-value {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: var(--fs-small);
    color: var(--ink-2);
  }
  .status {
    flex: none;
    min-width: 92px;
    padding: 6px 12px;
    border-radius: var(--r-xs);
    text-align: center;
    font-size: var(--fs-label);
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--tone);
    background: color-mix(in srgb, var(--tone) 16%, transparent);
    border: 1px solid transparent;
  }
  .status.outline {
    color: var(--ink-2);
    background: transparent;
    border-color: var(--ink-3);
  }

  .map {
    padding: 0 0 0 36px;
    overflow: hidden;
  }
  .map canvas {
    display: block;
    width: 100%;
    height: 200px;
  }
  .map svg {
    position: absolute;
    inset: 0 0 0 36px;
    width: calc(100% - 36px);
    height: 100%;
  }
  .cutoff-track {
    fill: none;
    stroke: #f2994a;
    stroke-width: 1.5;
  }
  .map-ticks {
    position: absolute;
    inset: 0 auto 0 0;
    width: 36px;
  }
  .map-ticks span {
    position: absolute;
    right: 6px;
    transform: translateY(50%);
    font-size: 10px;
    color: rgb(255 255 255 / 0.45);
  }

  .notes {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .note {
    margin: 0;
    padding: 14px 18px;
    border-radius: var(--r-md);
    font-size: var(--fs-small);
    color: var(--ink-2);
  }
</style>
