<script lang="ts">
  // The quality report for one track. Opening it shows the cached result at once (when the
  // track was checked before) and runs a fresh check, which also builds the time map the
  // cache leaves out. Closing it stops a check that is still running; a batch keeps going.
  import { onMount } from "svelte";
  import { app } from "../lib/app.svelte";
  import { commands, events, type QualityReport } from "../lib/bindings";
  import { duration } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import {
    criterionKey,
    evidenceValue,
    frequencyTicks,
    heat,
    logX,
    reasonKey,
    statusKey,
    statusTone,
    tickLabel,
    verdictKey,
    verdictTone,
  } from "../lib/quality";

  let el: HTMLDialogElement | undefined = $state();
  let canvas: HTMLCanvasElement | undefined = $state();
  /** The track on screen; a different one starts over. */
  let shown: number | null = null;
  let report = $state<QualityReport | null>(null);
  let running = $state(false);
  let progress = $state(0);
  let error = $state<string | null>(null);
  /** Bumped on every run, so the answer of an older run is dropped. */
  let runs = 0;

  const SPECTRUM_MIN_HZ = 20;
  const FLOOR_DB = -120;
  const W = 600;
  const H = 180;

  onMount(() => {
    const listening = events.qualityProgressEvent.listen((e) => {
      if (running && e.payload.trackId === shown) progress = e.payload.fraction ?? 0;
    });
    return () => void listening.then((stop) => stop());
  });

  $effect(() => {
    if (!el) return;
    const track = app.qualityTrack;
    if (!track) {
      if (running) void commands.qualityCancelCheck();
      shown = null;
      running = false;
      if (el.open) el.close();
      return;
    }
    if (!el.open) el.showModal();
    if (shown === track.id) return;
    shown = track.id;
    report = null;
    error = null;
    void open(track.id);
  });

  async function open(id: number) {
    const cached = await commands.qualityCached(id).catch(() => null);
    if (shown === id && cached && !report) report = cached;
    await check(id);
  }

  async function check(id: number) {
    const run = ++runs;
    running = true;
    progress = 0;
    error = null;
    try {
      const fresh = await commands.qualityCheckTrack(id);
      if (run === runs && shown === id) report = fresh;
    } catch (err) {
      const message = typeof err === "string" ? err : String(err);
      if (run === runs && shown === id && message !== "cancelled") error = message;
    } finally {
      if (run === runs) running = false;
    }
  }

  function close() {
    el?.close();
  }

  let source = $derived.by(() => {
    if (!report) return "";
    const s = report.source;
    const format = s.dsd
      ? `DSD ${(s.rate / 1e6).toFixed(1)} MHz`
      : `${s.bits}-bit / ${(s.rate / 1000).toFixed(1)} kHz`;
    return `${format} · ${duration((report.durationSecs ?? 0) * 1000)}`;
  });

  let spectrumPath = $derived.by(() => {
    if (!report?.spectrum.length) return "";
    const top = topHz;
    return report.spectrum
      .map((p, i) => {
        const x = logX(p.hz ?? 0, SPECTRUM_MIN_HZ, top) * W;
        const y = (Math.max(p.db ?? FLOOR_DB, FLOOR_DB) / FLOOR_DB) * H;
        return `${i ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join("");
  });

  /** Top of the spectrum axis (the stream's Nyquist frequency). */
  let topHz = $derived(report?.spectrumTopHz ?? 22_050);
  /** Top of the time map's frequency axis. */
  let mapTop = $derived(report?.spectrogram?.topHz ?? 22_050);
  const xAt = (hz: number) => logX(hz, SPECTRUM_MIN_HZ, topHz) * W;

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

  let mapTicks = $derived.by(() => {
    const top = report?.spectrogram ? mapTop : 0;
    const step = top > 30_000 ? 20_000 : 5_000;
    const ticks: number[] = [];
    for (let hz = step; hz < top * 0.97; hz += step) ticks.push(hz);
    return ticks;
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

<dialog bind:this={el} class="glass" aria-labelledby="quality-title" onclose={() => app.closeQuality()}>
  {#if app.qualityTrack}
    {@const track = app.qualityTrack}
    <header>
      <div class="heading">
        <h2 id="quality-title">{t("quality.title")}</h2>
        <p class="dim track" title={track.path}>{track.title}{track.artist ? ` · ${track.artist}` : ""}</p>
      </div>
      {#if running}
        <button type="button" class="btn small" onclick={() => commands.qualityCancelCheck()}>{t("quality.cancel")}</button>
      {:else}
        <button type="button" class="btn small" onclick={() => check(track.id)}>{t("quality.recheck")}</button>
      {/if}
      <button type="button" class="btn small" onclick={close}>{t("quality.close")}</button>
    </header>

    {#if running}
      <div class="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(progress * 100)}>
        <span style:width="{(progress * 100).toFixed(1)}%"></span>
      </div>
      <p class="dim small" aria-live="polite">{t("quality.checking", { pct: Math.round(progress * 100) })}</p>
    {/if}
    {#if error}
      <p class="error" role="alert">{t("quality.failed", { error })}</p>
    {/if}

    {#if report}
      {@const tone = verdictTone(report.verdict)}
      <section class="verdict soft tone-{tone}" aria-live="polite">
        <div class="verdict-line">
          <span class="chip tone-{tone}">{t(verdictKey(report.verdict))}</span>
          <span class="dim small">
            {t("quality.confidence", { level: t(`quality.confidence.${report.confidence}`) })} · {source}
          </span>
        </div>
        <p>{t(`quality.summary.${report.verdict}`)}</p>
        <p class="dim small">{t("quality.estimate")}</p>
      </section>

      <section aria-labelledby="quality-evidence">
        <h3 id="quality-evidence">{t("quality.evidence")}</h3>
        <ul class="evidence">
          {#each report.evidence as e (e.id)}
            {@const stone = statusTone(e.status)}
            <li>
              <span class="status tone-{stone}" class:outline={e.status === "notMeasured"}>{t(statusKey(e.status))}</span>
              <span class="name">{t(criterionKey(e.id))}</span>
              <span class="value">{evidenceValue(e)}</span>
              <span class="reason dim">{t(reasonKey(e))}</span>
            </li>
          {/each}
        </ul>
      </section>

      <section aria-labelledby="quality-spectrum">
        <h3 id="quality-spectrum">{t("quality.spectrum")}</h3>
        {#if !report.spectrum.length}
          <p class="dim small">{t("quality.chartsWaiting")}</p>
        {:else}
          <div class="well">
            <svg viewBox="0 0 {W} {H + 18}" role="img" aria-label={t("quality.spectrum")}>
              {#each [-40, -80] as db (db)}
                <line class="grid" x1="0" x2={W} y1={(db / FLOOR_DB) * H} y2={(db / FLOOR_DB) * H} />
                <text class="tick" x="4" y={(db / FLOOR_DB) * H - 3}>{db} dB</text>
              {/each}
              {#each frequencyTicks(topHz) as hz (hz)}
                <line class="grid" x1={xAt(hz)} x2={xAt(hz)} y1="0" y2={H} />
                <text class="tick" x={xAt(hz)} y={H + 14} text-anchor="middle">{tickLabel(hz)}</text>
              {/each}
              {#if topHz > 20_000}
                <line class="mark" x1={xAt(20_000)} x2={xAt(20_000)} y1="0" y2={H} />
              {/if}
              <path class="trace" d={spectrumPath} />
              {#if report.cutoffHz}
                <line class="cutoff" x1={xAt(report.cutoffHz)} x2={xAt(report.cutoffHz)} y1="0" y2={H} />
              {/if}
            </svg>
          </div>
          <p class="dim small">{t("quality.spectrumHint")}</p>
        {/if}
      </section>

      <section aria-labelledby="quality-map">
        <h3 id="quality-map">{t("quality.spectrogram")}</h3>
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
    {/if}
  {/if}
</dialog>

<style>
  dialog {
    width: min(760px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    overflow-y: auto;
    padding: 24px;
    border: none;
    border-radius: var(--r-lg);
    color: var(--ink);
  }
  dialog::backdrop {
    background: rgb(0 0 0 / 0.5);
  }
  dialog > :global(* + *) {
    margin-top: 14px;
  }
  header {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }
  .heading {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  h2 {
    font-size: var(--fs-title);
  }
  h3 {
    font-size: var(--fs-body);
    font-weight: 600;
    margin-bottom: 8px;
  }
  .track {
    margin: 0;
    font-size: var(--fs-small);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .small {
    font-size: var(--fs-small);
    margin: 0;
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
  .verdict {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px 16px;
    border-radius: var(--r-md);
    border-left: 3px solid var(--tone);
  }
  .verdict p {
    margin: 0;
  }
  .verdict-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
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
  .chip {
    padding: 3px 10px;
    border-radius: var(--r-pill);
    font-weight: 600;
    color: var(--tone);
    background: color-mix(in srgb, var(--tone) 16%, transparent);
  }
  .evidence {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .evidence li {
    display: grid;
    grid-template-columns: 96px minmax(0, 1fr) auto;
    column-gap: 12px;
    row-gap: 2px;
    padding: 8px 10px;
    border-radius: var(--r-sm);
  }
  .evidence li:nth-child(odd) {
    background: var(--soft);
  }
  .status {
    align-self: start;
    justify-self: start;
    padding: 1px 8px;
    border-radius: var(--r-pill);
    font-size: var(--fs-label);
    font-weight: 600;
    color: var(--tone);
    background: color-mix(in srgb, var(--tone) 16%, transparent);
    border: 1px solid transparent;
  }
  .status.outline {
    background: transparent;
    border-color: var(--tone);
  }
  .name {
    font-weight: 500;
  }
  .value {
    font-family: var(--font-figures);
    font-variant-numeric: tabular-nums;
  }
  .reason {
    grid-column: 2 / 4;
    font-size: var(--fs-small);
  }
  /* Chart wells stay dark in every theme so the heat ramp reads the same. */
  .well {
    position: relative;
    border-radius: var(--r-sm);
    background: #0b0b0f;
    padding: 8px;
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
    stroke: rgb(255 255 255 / 0.25);
    stroke-width: 1;
  }
  .trace {
    fill: none;
    stroke: #e8e8ee;
    stroke-width: 1.4;
  }
  .cutoff {
    stroke: #f2994a;
    stroke-width: 1.5;
    stroke-dasharray: 5 4;
  }
  .map {
    padding: 0 0 0 36px;
    overflow: hidden;
  }
  .map canvas {
    display: block;
    width: 100%;
    height: 180px;
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
</style>
