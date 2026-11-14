<script lang="ts">
  import { onDestroy } from "svelte";
  import { app } from "../lib/app.svelte";
  import {
    commands,
    events,
    type BiquadKindSetting,
    type ConvolutionSetting,
    type CoverStyle,
    type CrossfeedPresetSetting,
    type CrossfeedSetting,
    type CrossfadeSetting,
    type Device,
    type DsdMode,
    type DsdPcmRate,
    type FilterPhaseSetting,
    type FilterSourceSetting,
    type FilterVariantSetting,
    type GlassQuality,
    type ImpulseResponseInfo,
    type LanguageSetting,
    type LevelingSetting,
    type PeqBandSetting,
    type PcmToDsd,
    type PeqSetting,
    type RootFolder,
    type SdmOrderSetting,
    type SrcSetting,
    type SrcTargetSetting,
  } from "../lib/bindings";
  import { rate } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import Icon from "./Icon.svelte";
  import { rangeFill } from "../lib/rangeFill";

  let roots = $state<RootFolder[]>([]);
  let devices = $state<Device[] | null>(null);
  let followSymlinks = $state(false);
  let adding = $state(false);
  let showFailures = $state(false);
  let volumeDraft = $state(-10);

  let coverStyles = $derived<{ value: CoverStyle; label: string; hint: string }[]>([
    { value: "vinyl", label: t("settings.look.vinyl"), hint: t("settings.look.vinylHint") },
    { value: "square", label: t("settings.look.square"), hint: t("settings.look.squareHint") },
  ]);
  let glassLevels = $derived<{ value: GlassQuality; label: string }[]>([
    { value: "full", label: t("settings.look.full") },
    { value: "reduced", label: t("settings.look.reduced") },
    { value: "off", label: t("common.off") },
  ]);
  /** Each language is named in itself, so it can be found whatever the interface shows now. */
  let languages = $derived<{ value: LanguageSetting; label: string }[]>([
    { value: "system", label: t("settings.lang.system") },
    { value: "en", label: "English" },
    { value: "vi", label: "Tiếng Việt" },
  ]);

  $effect(() => {
    void app.libraryVersion;
    void app.scan.phase;
    app.run(commands.libraryRoots()).then((r) => {
      if (r) roots = r;
    });
  });

  async function loadDevices() {
    devices = null;
    devices = (await app.run(commands.devicesList())) ?? [];
  }
  loadDevices();
  const unlisten = events.devicesChangedEvent.listen((e) => (devices = e.payload));
  onDestroy(() => unlisten.then((f) => f()));

  async function addRoot() {
    adding = true;
    await app.run(commands.libraryAddRoot(followSymlinks));
    adding = false;
    roots = (await app.run(commands.libraryRoots())) ?? roots;
  }

  async function selectDevice(id: string) {
    await app.run(commands.deviceSelect(id));
    if (devices) devices = devices.map((d) => ({ ...d, selected: d.id === id }));
  }

  let scanning = $derived(app.scan.phase !== "idle");
  let scanText = $derived.by(() => {
    const s = app.scan;
    switch (s.phase) {
      case "walking":
        return t("settings.lib.walking");
      case "reading":
        return s.total
          ? t("settings.lib.readingProgress", { done: s.done, total: s.total })
          : t("settings.lib.reading");
      case "writing":
        return t("settings.lib.writing");
      default:
        return s.last;
    }
  });

  // ----- updates -----

  let appVersion = $state("");
  app.run(commands.appVersion()).then((v) => {
    if (v) appVersion = v;
  });
  let checking = $state(false);
  /** What the last check from this page said, until the next one. */
  let checkResult = $state<"failed" | "newest" | null>(null);

  async function checkForUpdate() {
    checking = true;
    const found = await app.checkForUpdate();
    checking = false;
    checkResult = found === undefined ? "failed" : found ? null : "newest";
  }

  let updateStatus = $derived.by(() => {
    const p = app.updateProgress;
    if (p) {
      return p.total
        ? t("settings.update.downloadingPct", { pct: Math.min(100, Math.round((p.downloaded / p.total) * 100)) })
        : t("settings.update.downloading");
    }
    if (app.update) return t("settings.update.available", { version: app.update.version });
    if (checkResult === "failed") return t("settings.update.checkFailed");
    if (checkResult === "newest") return t("settings.update.newest");
    return t("settings.update.source");
  });

  let bitPerfect = $derived(app.player.volumeDb == null);
  $effect(() => {
    if (app.player.volumeDb != null) volumeDraft = app.player.volumeDb;
  });
  let selected = $derived(devices?.find((d) => d.selected));

  function setVolume(db: number | null) {
    app.run(commands.playerSetVolume(db));
  }

  let dsdModes = $derived<{ value: DsdMode; label: string; hint: string }[]>([
    { value: "dop", label: "DoP", hint: t("settings.dsd.dopHint") },
    { value: "native", label: t("settings.dsd.native"), hint: t("settings.dsd.nativeHint") },
    { value: "pcm", label: "PCM", hint: t("settings.dsd.pcmHint") },
  ]);
  const dsdRates: { value: DsdPcmRate; label: string }[] = [
    { value: "hz176400", label: rate(176400) },
    { value: "hz352800", label: rate(352800) },
  ];
  let pcmToDsdOptions = $derived<{ value: PcmToDsd; label: string }[]>([
    { value: "off", label: t("common.off") },
    { value: "dsd64", label: "DSD64" },
    { value: "dsd128", label: "DSD128" },
    { value: "dsd256", label: "DSD256" },
  ]);
  let sdmOrders = $derived<{ value: SdmOrderSetting; label: string }[]>([
    { value: "order5", label: t("settings.order5") },
    { value: "order7", label: t("settings.order7") },
  ]);
  let dsdMode = $derived<DsdMode>((selected && app.settings.dsdModes?.[selected.id]) || "dop");

  function setDsdMode(mode: DsdMode) {
    if (!selected) return;
    app.saveSettings({ ...app.settings, dsdModes: { ...app.settings.dsdModes, [selected.id]: mode } });
  }

  function formatsSummary(d: Device): string {
    if (!d.formats.length) return t("settings.formatsUnknown");
    const rates = [...new Set(d.formats.map((f) => f.rate))].sort((a, b) => a - b);
    const bits = Math.max(...d.formats.map((f) => f.bits));
    return t("settings.formatsUpTo", { bits: String(bits), rates: rates.map(rate).join(", ") });
  }

  // ----- DSP: PEQ, levelling, dither -----

  let levelingModes = $derived<{ value: LevelingSetting; label: string; hint: string }[]>([
    { value: "off", label: t("common.off"), hint: t("settings.level.offHint") },
    { value: "track", label: t("settings.level.track"), hint: t("settings.level.trackHint") },
    { value: "album", label: t("settings.level.album"), hint: t("settings.level.albumHint") },
    { value: "auto", label: t("settings.level.auto"), hint: t("settings.level.autoHint") },
  ]);
  let biquadKinds = $derived<{ value: BiquadKindSetting; label: string }[]>([
    { value: "peak", label: t("settings.biquad.peak") },
    { value: "lowShelf", label: t("settings.biquad.lowShelf") },
    { value: "highShelf", label: t("settings.biquad.highShelf") },
    { value: "lowPass", label: t("settings.biquad.lowPass") },
    { value: "highPass", label: t("settings.biquad.highPass") },
    { value: "bandPass", label: t("settings.biquad.bandPass") },
    { value: "notch", label: t("settings.biquad.notch") },
  ]);
  let srcTargets = $derived<{ value: SrcTargetSetting; label: string }[]>([
    { value: "off", label: t("common.off") },
    { value: "maxDevice", label: t("settings.src.maxDevice") },
    { value: "keepFamily", label: t("settings.src.keepFamily") },
    { value: "fixed", label: t("settings.src.fixed") },
  ]);
  const fixedRates = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000, 705_600, 768_000];

  let peq = $derived<PeqSetting>(app.settings.peq ?? { preampDb: 0, bands: [] });
  let peqPresets = $derived(app.settings.peqPresets ?? {});
  let presetName = $state("");

  function savePeq(next: PeqSetting) {
    app.saveSettings({ ...app.settings, peq: next, peqPreset: null });
  }

  function setBand(index: number, change: Partial<PeqBandSetting>) {
    const bands = (peq.bands ?? []).map((band, i) => (i === index ? { ...band, ...change } : band));
    savePeq({ ...peq, bands });
  }

  function addBand() {
    savePeq({
      ...peq,
      bands: [...(peq.bands ?? []), { kind: "peak", freqHz: 1000, gainDb: 0, q: 1, enabled: true }],
    });
  }

  function removeBand(index: number) {
    savePeq({ ...peq, bands: (peq.bands ?? []).filter((_, i) => i !== index) });
  }

  function applyPreset(name: string) {
    if (!name) return;
    const preset = peqPresets[name];
    if (preset) app.saveSettings({ ...app.settings, peq: preset, peqPreset: name });
  }

  function savePreset() {
    const name = presetName.trim();
    if (!name) return;
    app.saveSettings({
      ...app.settings,
      peq: peq,
      peqPresets: { ...peqPresets, [name]: peq },
      peqPreset: name,
    });
    presetName = "";
  }

  function deletePreset() {
    const name = app.settings.peqPreset;
    if (!name) return;
    const next = { ...peqPresets };
    delete next[name];
    app.saveSettings({ ...app.settings, peqPresets: next, peqPreset: null });
  }

  /** An EQ with no bands and no preamp has nothing to export. */
  let peqEmpty = $derived((peq.bands?.length ?? 0) === 0 && (peq.preampDb ?? 0) === 0);

  /** `base`, or `base (2)`, `base (3)`… while that name is taken. */
  function freePresetName(base: string): string {
    let name = base;
    for (let n = 2; Object.hasOwn(peqPresets, name); n += 1) name = `${base} (${n})`;
    return name;
  }

  /** Reads an Equalizer APO parametric file, saves it as a preset and applies it. */
  async function importPeq() {
    const imported = await app.run(commands.peqImport());
    if (!imported) return;
    let name = imported.name;
    if (Object.hasOwn(peqPresets, name)) {
      const kept = freePresetName(name);
      const replace = await app.ask({
        title: t("settings.peq.exists"),
        message: t("settings.peq.existsMessage", { name, kept }),
        confirm: t("settings.peq.replace"),
      });
      if (replace == null) name = kept;
    }
    const saved = await app.saveSettings({
      ...app.settings,
      peq: imported.peq,
      peqPresets: { ...peqPresets, [name]: imported.peq },
      peqPreset: name,
    });
    if (!saved) return;
    const bands = imported.peq.bands?.length ?? 0;
    const warnings = imported.warnings ?? [];
    const count = t("settings.peq.bands", { count: bands });
    // One notice, but a broken file can carry a hundred reasons.
    const shown = warnings.slice(0, 3);
    const more = warnings.length - shown.length;
    const reasons = more > 0 ? [...shown, t("settings.peq.andMore", { count: more })] : shown;
    const skipped = warnings.length ? ` — ${reasons.join(" · ")}` : "";
    app.notify(warnings.length ? "warning" : "info", t("settings.peq.imported", { name, bands: count }) + skipped);
  }

  /** Writes the current EQ as Equalizer APO parametric text. */
  async function exportPeq() {
    const name = app.settings.peqPreset ?? "Sonder";
    const saved = await app.run(commands.peqExport(peq, name));
    if (!saved) return;
    app.notify("info", t("settings.peq.exported", { file: saved.split(/[\\/]/).pop() ?? saved }));
  }

  // ----- DSP: convolution and crossfeed -----

  let convolution = $derived<ConvolutionSetting>(app.settings.convolution ?? {});
  let crossfeed = $derived<CrossfeedSetting>(app.settings.crossfeed ?? {});
  let irInfo = $state<ImpulseResponseInfo | null>(null);
  let irError = $state<string | null>(null);
  let crossfeedPresets = $derived<{ value: CrossfeedPresetSetting; label: string; hint: string }[]>([
    { value: "default", label: t("settings.xfeed.default"), hint: t("settings.xfeed.defaultHint") },
    { value: "chuMoy", label: "Chu Moy", hint: "700 Hz, 6 dB" },
    { value: "janMeier", label: "Jan Meier", hint: t("settings.xfeed.janMeierHint") },
  ]);

  // Path last described; plain (not reactive), so saving any other setting does not re-read
  // the file, even while it is unreadable.
  let describedPath: string | null = null;

  // Describe the chosen file when Settings opens or the file changes.
  $effect(() => {
    const path = convolution.path ?? null;
    if (path === describedPath) return;
    describedPath = path;
    if (!path) {
      irInfo = null;
      irError = null;
      return;
    }
    commands.impulseResponseInfo(path).then(
      (info) => {
        irInfo = info;
        irError = null;
      },
      (err) => {
        irInfo = null;
        irError = typeof err === "string" ? err : String(err);
      },
    );
  });

  function saveConvolution(change: Partial<ConvolutionSetting>) {
    app.saveSettings({ ...app.settings, convolution: { ...convolution, ...change } });
  }

  async function pickImpulseResponse() {
    const info = await app.run(commands.settingsPickImpulseResponse());
    if (!info) return;
    irInfo = info;
    irError = null;
    describedPath = info.path;
    saveConvolution({ path: info.path, enabled: true });
  }

  function saveCrossfeed(change: Partial<CrossfeedSetting>) {
    app.saveSettings({ ...app.settings, crossfeed: { ...crossfeed, ...change } });
  }

  // ----- playback: crossfade -----

  let crossfade = $derived<CrossfadeSetting>(app.settings.crossfade ?? { enabled: false, seconds: 4 });
  let fadeSeconds = $state(4);
  $effect(() => {
    if (app.settings.crossfade) fadeSeconds = app.settings.crossfade.seconds ?? 4;
  });

  function saveCrossfade(change: Partial<CrossfadeSetting>) {
    app.saveSettings({ ...app.settings, crossfade: { ...crossfade, ...change } });
  }

  function setLeveling(mode: LevelingSetting) {
    app.saveSettings({ ...app.settings, leveling: mode });
  }

  function setTargetLufs(event: Event) {
    const value = Number((event.currentTarget as HTMLInputElement).value);
    if (Number.isFinite(value)) app.saveSettings({ ...app.settings, levelingTargetLufs: value });
  }

  // ----- DSP: rate conversion -----

  let src = $derived<SrcSetting>(app.settings.src ?? {});

  function saveSrc(change: Partial<SrcSetting>) {
    app.saveSettings({ ...app.settings, src: { ...src, ...change } });
  }

  function setNumberField(event: Event, apply: (value: number) => void) {
    const value = Number((event.currentTarget as HTMLInputElement).value);
    if (Number.isFinite(value)) apply(value);
  }

  async function pickRoonFolder() {
    const folder = await app.run(commands.settingsPickRoonFolder());
    if (folder) saveSrc({ roonFolder: folder, filterSource: "roon" });
  }
</script>

<div class="page scroll">
  <header class="page-head">
    <h1 tabindex="-1" data-view-heading>{t("common.settings")}</h1>
  </header>

  <div class="cards">
    <!-- Output: the most important state in an audiophile player, up front. -->
    <section class="card hero soft" aria-labelledby="out-title">
      <div class="hero-text">
        <span class="state" class:ok={bitPerfect}>
          <span class="dot" aria-hidden="true"></span>
          {bitPerfect ? t("common.bitPerfect") : t("settings.softwareVolumeState", { db: volumeDraft.toFixed(1) })}
        </span>
        <h2 id="out-title">{selected?.name ?? t("settings.output")}</h2>
        <p class="dim">{t("settings.outputIntro")}</p>
      </div>
      <svg class="hero-disc" viewBox="0 0 100 100" aria-hidden="true">
        <circle cx="50" cy="50" r="48" fill="none" stroke="currentColor" stroke-opacity=".18" />
        <circle cx="50" cy="50" r="36" fill="none" stroke="currentColor" stroke-opacity=".12" />
        <circle cx="50" cy="50" r="24" fill="none" stroke="currentColor" stroke-opacity=".08" />
        <circle cx="50" cy="50" r="12" fill="currentColor" fill-opacity=".14" />
      </svg>
      <div class="rows">
        <label class="row">
          <span class="icon"><Icon name="volume" size={18} /></span>
          <span class="info">
            <span class="name">{t("settings.bitPerfectOutput")}</span>
            <span class="dim small">{t("settings.bitPerfectHint")}</span>
          </span>
          <input
            type="checkbox"
            role="switch"
            class="switch"
            checked={bitPerfect}
            onchange={(e) => setVolume(e.currentTarget.checked ? null : volumeDraft)}
          />
        </label>
        {#if !bitPerfect}
          <div class="row">
            <span class="icon"><Icon name="volume" size={18} /></span>
            <input
              class="range"
              type="range"
              min="-60"
              max="0"
              step="0.5"
              aria-label={t("common.softwareVolume")}
              aria-valuetext="{volumeDraft.toFixed(1)} dB"
              bind:value={volumeDraft}
              use:rangeFill={volumeDraft}
              onchange={() => setVolume(volumeDraft)}
            />
            <span class="badge value warn">{volumeDraft.toFixed(1)} dB</span>
          </div>
        {/if}
      </div>
    </section>

    <section aria-labelledby="dev-title">
      <div class="group-head">
        <h3 id="dev-title">{t("settings.outputDevice")}</h3>
        <button class="icon-btn" aria-label={t("settings.refreshDevices")} onclick={loadDevices}><Icon name="refresh" size={18} /></button>
      </div>
      <div class="card soft" role="radiogroup" aria-labelledby="dev-title">
        {#if devices === null}
          <p class="dim pad">{t("settings.lookingForDevices")}</p>
        {:else if !devices.length}
          <p class="dim pad">{t("settings.noDevices")}</p>
        {:else}
          {#each devices as d (d.id)}
            <label class="row choice" class:selected={d.selected}>
              <span class="icon"><Icon name="speaker" size={18} /></span>
              <span class="info">
                <span class="name">{d.name}{d.isDefault ? ` · ${t("settings.systemDefault")}` : ""}</span>
                <span class="dim small">{formatsSummary(d)}</span>
              </span>
              <input class="sr" type="radio" name="device" checked={d.selected} onchange={() => selectDevice(d.id)} />
              <span class="check" aria-hidden="true"><Icon name="check" size={16} /></span>
            </label>
          {/each}
        {/if}
      </div>
    </section>

    <section aria-labelledby="dsd-title">
      <div class="group-head"><h3 id="dsd-title">{t("settings.dsd.title")}</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="speaker" size={18} /></span>
          <span class="info">
            <span class="name" id="dsd-mode-label">{t("settings.dsd.on", { device: selected?.name ?? t("settings.dsd.thisDevice") })}</span>
            <span class="dim small">{dsdModes.find((m) => m.value === dsdMode)?.hint}</span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="dsd-mode-label">
            {#each dsdModes as mode (mode.value)}
              <button
                class="chip"
                role="radio"
                aria-checked={dsdMode === mode.value}
                disabled={!selected}
                onclick={() => setDsdMode(mode.value)}
              >
                {mode.label}
              </button>
            {/each}
          </div>
        </div>
        <div class="row">
          <span class="icon"><Icon name="volume" size={18} /></span>
          <span class="info">
            <span class="name" id="dsd-rate-label">{t("settings.dsd.pcmRate")}</span>
            <span class="dim small">{t("settings.dsd.pcmRateHint")}</span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="dsd-rate-label">
            {#each dsdRates as r (r.value)}
              <button
                class="chip"
                role="radio"
                aria-checked={(app.settings.dsdPcmRate ?? "hz176400") === r.value}
                onclick={() => app.saveSettings({ ...app.settings, dsdPcmRate: r.value })}
              >
                {r.label}
              </button>
            {/each}
          </div>
        </div>
        <div class="row">
          <span class="icon"><Icon name="signal" size={18} /></span>
          <span class="info">
            <span class="name" id="pcm-dsd-label">{t("settings.pcmToDsd")}</span>
            <span class="dim small">
              {t("settings.pcmToDsdHint")}
              {#if app.settings.pcmToDsd === "dsd256"}
                {t("settings.pcmToDsdDsd256")}
              {/if}
            </span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="pcm-dsd-label">
            {#each pcmToDsdOptions as option (option.value)}
              <button
                class="chip"
                role="radio"
                aria-checked={(app.settings.pcmToDsd ?? "off") === option.value}
                onclick={() => app.saveSettings({ ...app.settings, pcmToDsd: option.value })}
              >
                {option.label}
              </button>
            {/each}
          </div>
        </div>
        {#if (app.settings.pcmToDsd ?? "off") !== "off"}
          <div class="row">
            <span class="icon"><Icon name="signal" size={18} /></span>
            <span class="info">
              <span class="name" id="sdm-order-label">{t("settings.sdmOrder")}</span>
              <span class="dim small">{t("settings.sdmOrderHint")}</span>
            </span>
            <div class="segmented" role="radiogroup" aria-labelledby="sdm-order-label">
              {#each sdmOrders as order (order.value)}
                <button
                  class="chip"
                  role="radio"
                  aria-checked={(app.settings.sdmOrder ?? "order5") === order.value}
                  onclick={() => app.saveSettings({ ...app.settings, sdmOrder: order.value })}
                >
                  {order.label}
                </button>
              {/each}
            </div>
          </div>
        {/if}
      </div>
    </section>

    <section aria-labelledby="fade-title">
      <div class="group-head"><h3 id="fade-title">{t("settings.fade.title")}</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="next" size={18} /></span>
          <span class="info">
            <span class="name">{t("settings.fade.toggle")}</span>
            <span class="dim small">{t("settings.fade.hint")}</span>
          </span>
          <input
            type="checkbox"
            role="switch"
            class="switch"
            aria-label={t("settings.fade.title")}
            checked={crossfade.enabled ?? false}
            onchange={(e) => saveCrossfade({ enabled: e.currentTarget.checked })}
          />
        </div>
        <div class="row">
          <span class="icon"><Icon name="refresh" size={18} /></span>
          <span class="info">
            <span class="name">{t("settings.fade.length")}</span>
            <span class="dim small">{t("settings.fade.lengthHint")}</span>
          </span>
          <input
            class="range"
            type="range"
            min="1"
            max="10"
            step="0.5"
            aria-label={t("settings.fade.lengthLabel")}
            aria-valuetext={t("settings.fade.seconds", { n: fadeSeconds.toFixed(1) })}
            disabled={!(crossfade.enabled ?? false)}
            bind:value={fadeSeconds}
            use:rangeFill={fadeSeconds}
            onchange={() => saveCrossfade({ seconds: fadeSeconds })}
          />
          <span class="badge value">{fadeSeconds.toFixed(1)} s</span>
        </div>
      </div>
    </section>

    <section aria-labelledby="conv-title">
      <div class="group-head"><h3 id="conv-title">{t("settings.conv.title")}</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="signal" size={18} /></span>
          <span class="info">
            <span class="name">{t("settings.conv.ir")}</span>
            <span class="dim small">
              {#if irInfo}
                {t("settings.conv.irInfo", {
                  name: irInfo.name,
                  rate: rate(irInfo.rate),
                  channels: irInfo.channels,
                  seconds: (irInfo.seconds ?? 0).toFixed(2),
                })}
              {:else if irError}
                {irError}
              {:else}
                {t("settings.conv.irHint")}
              {/if}
            </span>
          </span>
          <button class="btn small" onclick={pickImpulseResponse}>{t("common.choose")}</button>
          <input
            type="checkbox"
            role="switch"
            class="switch"
            aria-label={t("settings.conv.title")}
            disabled={!convolution.path}
            checked={convolution.enabled ?? false}
            onchange={(e) => saveConvolution({ enabled: e.currentTarget.checked })}
          />
        </div>
        {#if irInfo && (irInfo.peakGainDb ?? 0) > 0.1}
          <p class="note">{t("settings.conv.boost", { db: (irInfo.peakGainDb ?? 0).toFixed(1) })}</p>
        {/if}
      </div>
    </section>

    <section aria-labelledby="peq-title">
      <div class="group-head"><h3 id="peq-title">{t("settings.peq.title")}</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="palette" size={18} /></span>
          <span class="info">
            <span class="name">{t("settings.peq.preset")}</span>
            <span class="dim small">{t("settings.peq.presetHint")}</span>
          </span>
          <select
            class="text"
            aria-label={t("settings.peq.presetLabel")}
            value={app.settings.peqPreset ?? ""}
            onchange={(e) => applyPreset(e.currentTarget.value)}
          >
            <option value="">{t("settings.peq.custom")}</option>
            {#each Object.keys(peqPresets) as name (name)}
              <option value={name}>{name}</option>
            {/each}
          </select>
        </div>
        <div class="row">
          <span class="icon"><Icon name="volume" size={18} /></span>
          <span class="info">
            <span class="name">{t("settings.peq.preamp")}</span>
            <span class="dim small">{t("settings.peq.preampHint")}</span>
          </span>
          <input
            class="range"
            type="range"
            min="-60"
            max="12"
            step="0.5"
            aria-label={t("settings.peq.preampLabel")}
            aria-valuetext="{(peq.preampDb ?? 0).toFixed(1)} dB"
            value={peq.preampDb ?? 0}
            use:rangeFill={peq.preampDb ?? 0}
            onchange={(e) => savePeq({ ...peq, preampDb: Number(e.currentTarget.value) })}
          />
          <span class="badge value" class:warn={(peq.preampDb ?? 0) > 0}>{(peq.preampDb ?? 0).toFixed(1)} dB</span>
        </div>
        {#each peq.bands ?? [] as band, i (i)}
          <div class="band">
            <select
              class="text"
              aria-label={t("settings.peq.bandType", { n: i + 1 })}
              value={band.kind ?? "peak"}
              onchange={(e) => setBand(i, { kind: e.currentTarget.value as BiquadKindSetting })}
            >
              {#each biquadKinds as kind (kind.value)}
                <option value={kind.value}>{kind.label}</option>
              {/each}
            </select>
            <input
              class="num"
              type="number"
              min="10"
              max="24000"
              step="1"
              aria-label={t("settings.peq.bandFreq", { n: i + 1 })}
              value={band.freqHz ?? 1000}
              onchange={(e) => setNumberField(e, (v) => setBand(i, { freqHz: v }))}
            />
            <input
              class="num"
              type="number"
              min="-24"
              max="24"
              step="0.5"
              aria-label={t("settings.peq.bandGain", { n: i + 1 })}
              value={band.gainDb ?? 0}
              onchange={(e) => setNumberField(e, (v) => setBand(i, { gainDb: v }))}
            />
            <input
              class="num"
              type="number"
              min="0.1"
              max="20"
              step="0.1"
              aria-label={t("settings.peq.bandQ", { n: i + 1 })}
              value={band.q ?? 1}
              onchange={(e) => setNumberField(e, (v) => setBand(i, { q: v }))}
            />
            <label class="inline">
              <input
                type="checkbox"
                role="switch"
                class="switch"
                checked={band.enabled ?? true}
                onchange={(e) => setBand(i, { enabled: e.currentTarget.checked })}
              />
              {t("common.on")}
            </label>
            <button class="link" onclick={() => removeBand(i)}>{t("common.remove")}</button>
          </div>
        {/each}
        <div class="row actions">
          <button class="btn small" onclick={addBand}><Icon name="add" size={16} /> {t("settings.peq.addBand")}</button>
          <input
            class="text"
            placeholder={t("settings.peq.presetName")}
            aria-label={t("settings.peq.newPresetName")}
            bind:value={presetName}
          />
          <button class="btn small" disabled={!presetName.trim()} onclick={savePreset}>{t("settings.peq.savePreset")}</button>
          {#if app.settings.peqPreset}
            <button class="btn small" onclick={deletePreset}>{t("settings.peq.deletePreset")}</button>
          {/if}
          <button class="btn small" onclick={importPeq}>{t("settings.peq.import")}</button>
          <button class="btn small" disabled={peqEmpty} onclick={exportPeq}>{t("settings.peq.export")}</button>
        </div>
        <p class="note">{t("settings.peq.importNote")}</p>
      </div>
    </section>

    <section aria-labelledby="xfeed-title">
      <div class="group-head"><h3 id="xfeed-title">{t("settings.xfeed.title")}</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="speaker" size={18} /></span>
          <span class="info">
            <span class="name">{t("settings.xfeed.name")}</span>
            <span class="dim small">{t("settings.xfeed.hint")}</span>
          </span>
          <input
            type="checkbox"
            role="switch"
            class="switch"
            aria-label={t("settings.xfeed.name")}
            checked={crossfeed.enabled ?? false}
            onchange={(e) => saveCrossfeed({ enabled: e.currentTarget.checked })}
          />
        </div>
        <div class="row">
          <span class="icon"><Icon name="speaker" size={18} /></span>
          <span class="info">
            <span class="name" id="xfeed-preset-label">{t("settings.peq.preset")}</span>
            <span class="dim small">
              {crossfeedPresets.find((p) => p.value === (crossfeed.preset ?? "default"))?.hint}
            </span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="xfeed-preset-label">
            {#each crossfeedPresets as preset (preset.value)}
              <button
                class="chip"
                role="radio"
                aria-checked={(crossfeed.preset ?? "default") === preset.value}
                onclick={() => saveCrossfeed({ preset: preset.value })}
              >
                {preset.label}
              </button>
            {/each}
          </div>
        </div>
      </div>
    </section>

    <section aria-labelledby="level-title">
      <div class="group-head"><h3 id="level-title">{t("settings.level.title")}</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="volume" size={18} /></span>
          <span class="info">
            <span class="name" id="level-mode-label">{t("settings.level.mode")}</span>
            <span class="dim small">
              {levelingModes.find((m) => m.value === (app.settings.leveling ?? "off"))?.hint}.
              {t("settings.level.peakNote")}
            </span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="level-mode-label">
            {#each levelingModes as mode (mode.value)}
              <button
                class="chip"
                role="radio"
                aria-checked={(app.settings.leveling ?? "off") === mode.value}
                onclick={() => setLeveling(mode.value)}
              >
                {mode.label}
              </button>
            {/each}
          </div>
        </div>
        <div class="row">
          <span class="icon"><Icon name="volume" size={18} /></span>
          <span class="info">
            <span class="name">{t("settings.level.target")}</span>
            <span class="dim small">{t("settings.level.targetHint")}</span>
          </span>
          <input
            class="num"
            type="number"
            min="-30"
            max="-5"
            step="0.5"
            aria-label={t("settings.level.targetLabel")}
            value={app.settings.levelingTargetLufs ?? -18}
            onchange={setTargetLufs}
          />
          <span class="badge">LUFS</span>
        </div>
        <div class="row">
          <span class="icon"><Icon name="disc" size={18} /></span>
          <span class="info">
            <span class="name">{t("settings.level.dither")}</span>
            <span class="dim small">{t("settings.level.ditherHint")}</span>
          </span>
          <input
            type="checkbox"
            role="switch"
            class="switch"
            checked={app.settings.dither ?? true}
            onchange={(e) => app.saveSettings({ ...app.settings, dither: e.currentTarget.checked })}
          />
        </div>
        <div class="row">
          <span class="icon"><Icon name="refresh" size={18} /></span>
          <span class="info">
            <span class="name">{t("settings.level.measure")}</span>
            <span class="dim small">
              {t("settings.level.measureHint")}
              {#if app.loudness.pending != null}{t("settings.level.waiting", { count: app.loudness.pending })}{/if}
            </span>
          </span>
          <button
            class="btn small"
            disabled={app.loudness.running || app.loudness.pending === 0}
            onclick={() => app.analyzeLoudness()}
          >
            {app.loudness.running
              ? app.loudness.total
                ? t("settings.level.analysingProgress", { done: app.loudness.done, total: app.loudness.total })
                : t("settings.level.analysing")
              : t("settings.level.analyse")}
          </button>
        </div>
        {#if app.loudness.last}
          <p class="note" aria-live="polite">{app.loudness.last}</p>
        {/if}
      </div>
    </section>

    <section aria-labelledby="src-title">
      <div class="group-head"><h3 id="src-title">{t("settings.src.title")}</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="disc" size={18} /></span>
          <span class="info">
            <span class="name" id="src-target-label">{t("settings.src.resampleTo")}</span>
            <span class="dim small">{t("settings.src.resampleHint")}</span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="src-target-label">
            {#each srcTargets as target (target.value)}
              <button
                class="chip"
                role="radio"
                aria-checked={(src.target ?? "off") === target.value}
                onclick={() => saveSrc({ target: target.value })}
              >
                {target.label}
              </button>
            {/each}
          </div>
        </div>
        {#if (src.target ?? "off") === "fixed"}
          <div class="row">
            <span class="icon"><Icon name="speaker" size={18} /></span>
            <span class="info">
              <span class="name">{t("settings.src.targetRate")}</span>
              <span class="dim small">{t("settings.src.targetRateHint")}</span>
            </span>
            <select
              class="text"
              aria-label={t("settings.src.fixedRateLabel")}
              value={src.fixedRate ?? 192000}
              onchange={(e) => saveSrc({ fixedRate: Number(e.currentTarget.value) })}
            >
              {#each fixedRates as hz (hz)}
                <option value={hz}>{rate(hz)}</option>
              {/each}
            </select>
          </div>
        {/if}
        <div class="row">
          <span class="icon"><Icon name="folder" size={18} /></span>
          <span class="info">
            <span class="name" id="filter-source-label">{t("settings.src.filters")}</span>
            <span class="dim small">{t("settings.src.filtersHint")}</span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="filter-source-label">
            {#each [{ value: "sonder", label: "Sonder" }, { value: "roon", label: "Roon" }] as source (source.value)}
              <button
                class="chip"
                role="radio"
                aria-checked={(src.filterSource ?? "sonder") === source.value}
                onclick={() => saveSrc({ filterSource: source.value as FilterSourceSetting })}
              >
                {source.label}
              </button>
            {/each}
          </div>
        </div>
        {#if (src.filterSource ?? "sonder") === "roon"}
          <div class="row">
            <span class="icon"><Icon name="folder" size={18} /></span>
            <span class="info">
              <span class="path" title={src.roonFolder ?? ""}>{src.roonFolder ?? t("settings.src.noFolder")}</span>
              <span class="dim small">{t("settings.src.roonFolderHint")}</span>
            </span>
            <button class="btn small" onclick={pickRoonFolder}>{t("common.choose")}</button>
          </div>
        {/if}
        <div class="row">
          <span class="icon"><Icon name="disc" size={18} /></span>
          <span class="info">
            <span class="name" id="phase-label">{t("settings.src.phase")}</span>
            <span class="dim small">{t("settings.src.phaseHint")}</span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="phase-label">
            {#each [{ value: "linear", label: t("settings.src.linear") }, { value: "minimum", label: t("settings.src.minimum") }] as phase (phase.value)}
              <button
                class="chip"
                role="radio"
                aria-checked={(src.phase ?? "linear") === phase.value}
                onclick={() => saveSrc({ phase: phase.value as FilterPhaseSetting })}
              >
                {phase.label}
              </button>
            {/each}
          </div>
        </div>
        <div class="row">
          <span class="icon"><Icon name="disc" size={18} /></span>
          <span class="info">
            <span class="name" id="variant-label">{t("settings.src.filter")}</span>
            <span class="dim small">{t("settings.src.filterHint")}</span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="variant-label">
            {#each [{ value: "precise", label: t("settings.src.precise") }, { value: "smooth", label: t("settings.src.smooth") }] as variant (variant.value)}
              <button
                class="chip"
                role="radio"
                aria-checked={(src.variant ?? "precise") === variant.value}
                onclick={() => saveSrc({ variant: variant.value as FilterVariantSetting })}
              >
                {variant.label}
              </button>
            {/each}
          </div>
        </div>
        <div class="row">
          <span class="icon"><Icon name="volume" size={18} /></span>
          <span class="info">
            <span class="name">{t("settings.src.headroom")}</span>
            <span class="dim small">{t("settings.src.headroomHint")}</span>
          </span>
          <input
            class="num"
            type="number"
            min="-60"
            max="0"
            step="0.5"
            aria-label={t("settings.src.headroomLabel")}
            value={src.headroomDb ?? -3}
            onchange={(e) =>
              setNumberField(e, (v) => saveSrc({ headroomDb: Number.isFinite(v) ? v : null }))}
          />
          <span class="badge">dB</span>
        </div>
        <div class="row">
          <span class="icon"><Icon name="refresh" size={18} /></span>
          <span class="info">
            <span class="name">{t("settings.src.fallback")}</span>
            <span class="dim small">{t("settings.src.fallbackHint")}</span>
          </span>
          <input
            type="checkbox"
            role="switch"
            class="switch"
            checked={src.autoFallback ?? true}
            onchange={(e) => saveSrc({ autoFallback: e.currentTarget.checked })}
          />
        </div>
        <div class="row">
          <span class="icon"><Icon name="refresh" size={18} /></span>
          <span class="info">
            <span class="name">{t("settings.src.lift")}</span>
            <span class="dim small">{t("settings.src.liftHint")}</span>
          </span>
          <input
            type="checkbox"
            role="switch"
            class="switch"
            checked={src.forceUpsampleBelow44k ?? false}
            onchange={(e) => saveSrc({ forceUpsampleBelow44k: e.currentTarget.checked })}
          />
        </div>
      </div>
    </section>

    <section aria-labelledby="lib-title">
      <div class="group-head"><h3 id="lib-title">{t("settings.lib.title")}</h3></div>
      <div class="card soft">
        {#each roots as root (root.path)}
          <div class="row">
            <span class="icon"><Icon name="folder" size={18} /></span>
            <span class="info">
              <span class="path" title={root.path}>{root.path}</span>
              {#if root.followSymlinks}<span class="dim small">{t("settings.lib.followsLinks")}</span>{/if}
            </span>
            <button class="btn small" onclick={() => app.run(commands.libraryRemoveRoot(root.path))}>{t("common.remove")}</button>
          </div>
        {:else}
          <p class="dim pad">{t("settings.lib.empty")}</p>
        {/each}
        <div class="row actions">
          <button class="btn primary small" disabled={adding} onclick={addRoot}><Icon name="add" size={16} /> {t("settings.lib.addFolder")}</button>
          <label class="inline">
            <input type="checkbox" role="switch" class="switch" bind:checked={followSymlinks} />
            {t("settings.lib.followLinks")}
          </label>
          <span class="grow"></span>
          <button class="btn small" disabled={scanning || !roots.length} onclick={() => app.run(commands.libraryRescan())}>
            <Icon name="refresh" size={16} /> {t("settings.lib.rescan")}
          </button>
        </div>
        {#if scanText}
          <div class="scan" aria-live="polite">
            {#if scanning}
              <div class="progress">
                <div
                  class="fill"
                  class:indeterminate={app.scan.phase !== "reading" || !app.scan.total}
                  style:width={app.scan.total ? `${(100 * app.scan.done) / app.scan.total}%` : "30%"}
                ></div>
              </div>
            {/if}
            <span class="dim small">{scanText}</span>
            {#if !scanning && app.scan.failures.length}
              <button class="link" onclick={() => (showFailures = !showFailures)}>
                {showFailures ? t("settings.lib.hideFailures") : t("settings.lib.showFailures")}
              </button>
            {/if}
          </div>
          {#if showFailures && !scanning}
            <ul class="failures">
              {#each app.scan.failures as f (f)}<li>{f}</li>{/each}
            </ul>
          {/if}
        {/if}
      </div>
    </section>

    <section aria-labelledby="look-title">
      <div class="group-head"><h3 id="look-title">{t("settings.look.title")}</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="globe" size={18} /></span>
          <span class="info">
            <span class="name" id="language-label">{t("settings.lang.name")}</span>
            <span class="dim small">{t("settings.lang.hint")}</span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="language-label">
            {#each languages as language (language.value)}
              <button
                class="chip"
                role="radio"
                lang={language.value === "system" ? undefined : language.value}
                aria-checked={(app.settings.language ?? "system") === language.value}
                onclick={() => app.saveSettings({ ...app.settings, language: language.value })}
              >
                {language.label}
              </button>
            {/each}
          </div>
        </div>
        <div class="tiles" role="radiogroup" aria-label={t("settings.look.coverLabel")}>
          {#each coverStyles as style (style.value)}
            {@const on = (app.settings.coverStyle ?? "vinyl") === style.value}
            <label class="tile" class:selected={on}>
              <input
                class="sr"
                type="radio"
                name="cover-style"
                checked={on}
                onchange={() => app.saveSettings({ ...app.settings, coverStyle: style.value })}
              />
              <span class="preview {style.value}" aria-hidden="true"><span></span></span>
              <span class="name">{style.label}</span>
              <span class="dim small">{style.hint}</span>
            </label>
          {/each}
        </div>
        <div class="row">
          <span class="icon"><Icon name="palette" size={18} /></span>
          <span class="info">
            <span class="name" id="glass-label">{t("settings.look.glass")}</span>
            <span class="dim small">{t("settings.look.glassHint")}</span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="glass-label">
            {#each glassLevels as level (level.value)}
              <button
                class="chip"
                role="radio"
                aria-checked={(app.settings.glass ?? "full") === level.value}
                onclick={() => app.saveSettings({ ...app.settings, glass: level.value })}
              >
                {level.label}
              </button>
            {/each}
          </div>
        </div>
      </div>
    </section>

    <section aria-labelledby="lyrics-title">
      <div class="group-head"><h3 id="lyrics-title">{t("common.lyrics")}</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="lyrics" size={18} /></span>
          <span class="info">
            <span class="name" id="lyrics-online-label">{t("settings.lyrics.online")}</span>
            <span class="dim small">{t("settings.lyrics.onlineHint")}</span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="lyrics-online-label">
            {#each [true, false] as on (on)}
              <button
                class="chip"
                role="radio"
                aria-checked={(app.settings.lyricsOnline ?? true) === on}
                onclick={() => app.saveSettings({ ...app.settings, lyricsOnline: on })}
              >
                {on ? t("common.on") : t("common.off")}
              </button>
            {/each}
          </div>
        </div>
      </div>
    </section>

    <section aria-labelledby="update-title">
      <div class="group-head"><h3 id="update-title">{t("settings.update.title")}</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="refresh" size={18} /></span>
          <span class="info">
            <span class="name">Sonder {appVersion}</span>
            <span class="dim small" aria-live="polite">{updateStatus}</span>
          </span>
          {#if app.update}
            <button class="btn small" disabled={app.updateProgress != null} onclick={() => app.installUpdate()}>
              {t("settings.update.install", { version: app.update.version })}
            </button>
          {:else}
            <button class="btn small" disabled={checking} onclick={checkForUpdate}>
              {checking ? t("settings.update.checking") : t("settings.update.checkNow")}
            </button>
          {/if}
        </div>
        {#if app.update?.notes}
          <p class="note notes">{app.update.notes}</p>
        {/if}
        <div class="row">
          <span class="icon"><Icon name="settings" size={18} /></span>
          <span class="info">
            <span class="name" id="update-check-label">{t("settings.update.onLaunch")}</span>
            <span class="dim small">{t("settings.update.onLaunchHint")}</span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="update-check-label">
            {#each [true, false] as on (on)}
              <button
                class="chip"
                role="radio"
                aria-checked={(app.settings.updateCheck ?? true) === on}
                onclick={() => app.saveSettings({ ...app.settings, updateCheck: on })}
              >
                {on ? t("common.on") : t("common.off")}
              </button>
            {/each}
          </div>
        </div>
      </div>
    </section>
  </div>
</div>

<style>
  .cards {
    display: flex;
    flex-direction: column;
    gap: 22px;
    max-width: 780px;
  }
  .card {
    border-radius: var(--r-lg);
    padding: 8px;
  }
  .group-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 40px;
    padding: 0 6px 6px 8px;
  }
  h3 {
    font-size: var(--fs-label);
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ink-2);
  }

  .hero {
    position: relative;
    overflow: hidden;
    padding: 22px 22px 10px;
  }
  .hero-text {
    position: relative;
    max-width: 520px;
  }
  .state {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-family: var(--mono);
    font-size: var(--fs-label);
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--warn);
  }
  .state.ok {
    color: var(--ok);
  }
  .state .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: currentColor;
    box-shadow: 0 0 12px currentColor;
  }
  .hero h2 {
    margin-top: 8px;
    font-size: 28px;
    font-weight: 800;
    letter-spacing: -0.03em;
  }
  .hero p {
    margin: 8px 0 12px;
    font-size: var(--fs-small);
    line-height: 1.55;
  }
  .hero-disc {
    position: absolute;
    right: -40px;
    top: -40px;
    width: 220px;
    color: var(--ink);
    pointer-events: none;
  }
  .rows {
    position: relative;
  }

  /* Icon, text, then every control on the same line: extra children add columns instead of
     wrapping under the icon. */
  .row {
    display: grid;
    grid-template-columns: 40px minmax(0, 1fr);
    grid-auto-flow: column;
    grid-auto-columns: auto;
    align-items: center;
    gap: 14px;
    min-height: 60px;
    padding: 10px 12px;
    border-radius: var(--r-md);
  }
  .row.actions {
    display: flex;
    flex-wrap: wrap;
  }
  .icon {
    width: 40px;
    height: 40px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--soft-hover);
    color: var(--ink-2);
  }
  .info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .name {
    font-weight: 600;
  }
  .small {
    font-size: var(--fs-small);
  }
  .pad {
    padding: 14px 16px;
    margin: 0;
  }
  .path {
    font-family: var(--mono);
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    user-select: text;
  }
  .grow {
    flex: 1;
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--ink-2);
    font-size: var(--fs-small);
  }

  .choice {
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-standard);
  }
  .choice:hover {
    background: var(--soft);
  }
  .choice.selected {
    background: var(--soft-hover);
  }
  :global(:root:not([data-modality="pointer"])) .choice:focus-within {
    outline: 2px solid var(--ink);
    outline-offset: -2px;
  }
  .check {
    width: 26px;
    height: 26px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1.5px var(--hairline);
    color: transparent;
  }
  .selected .check {
    background: var(--ink);
    color: #050506;
    box-shadow: none;
  }
  .sr {
    position: absolute;
    opacity: 0;
    pointer-events: none;
    width: 1px;
    height: 1px;
  }

  /* Pill switch. */
  .switch {
    appearance: none;
    position: relative;
    width: 46px;
    height: 28px;
    padding: 0;
    border-radius: var(--r-pill);
    background: var(--soft-active);
    box-shadow: inset 0 0 0 1px var(--hairline);
    cursor: pointer;
    flex: none;
  }
  .switch::after {
    content: "";
    position: absolute;
    top: 3px;
    left: 3px;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--ink-2);
    transition:
      transform var(--dur-med) var(--ease-spring),
      background var(--dur-fast) var(--ease-standard);
  }
  .switch:checked {
    background: var(--ink);
  }
  .switch:checked::after {
    transform: translateX(18px);
    background: #050506;
  }

  .range {
    width: 100%;
  }
  /* beside a description the slider takes a fixed length, its value label after it */
  .info ~ .range {
    width: 180px;
  }
  .value {
    min-width: 64px;
    justify-content: center;
    font-variant-numeric: tabular-nums;
  }
  /* rows in one card read as one list, split by hairlines that start after the icon */
  .card > .row:not(.choice) + .row:not(.choice) {
    position: relative;
  }
  .card > .row:not(.choice) + .row:not(.choice)::before {
    content: "";
    position: absolute;
    top: 0;
    left: 66px;
    right: 12px;
    height: 1px;
    background: var(--hairline);
  }

  .scan {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 14px 12px;
  }
  .progress {
    width: 200px;
    height: 6px;
    border-radius: var(--r-pill);
    background: var(--soft-active);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: inherit;
    background: var(--ink);
    transition: width 0.2s;
  }
  .fill.indeterminate {
    animation: slide 1.2s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(330%);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .fill.indeterminate {
      animation: none;
      width: 100% !important;
      opacity: 0.45;
    }
  }
  .link {
    color: var(--ink);
    font-size: var(--fs-small);
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .failures {
    margin: 0 14px 12px;
    padding-left: 18px;
    font-family: var(--mono);
    font-size: 12px;
    color: var(--ink-2);
    user-select: text;
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 8px;
    padding: 6px;
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 14px;
    border-radius: var(--r-md);
    cursor: pointer;
    box-shadow: inset 0 0 0 1px var(--hairline);
    transition:
      background var(--dur-fast) var(--ease-standard),
      box-shadow var(--dur-fast) var(--ease-standard);
  }
  .tile:hover {
    background: var(--soft);
  }
  .tile.selected {
    background: var(--soft-hover);
    box-shadow: inset 0 0 0 2px var(--ink);
  }
  :global(:root:not([data-modality="pointer"])) .tile:focus-within {
    outline: 2px solid var(--ink);
    outline-offset: 2px;
  }
  .preview {
    display: grid;
    place-items: center;
    height: 96px;
    margin-bottom: 8px;
    border-radius: var(--r-sm);
    background: rgb(0 0 0 / 0.35);
  }
  .preview span {
    width: 72px;
    height: 72px;
    background: linear-gradient(135deg, #6a7bd8, #c45c8a);
  }
  .preview.vinyl span {
    border-radius: 50%;
    background:
      radial-gradient(circle, #0b0b0d 0 5%, transparent 5.5%),
      radial-gradient(circle, transparent 0 30%, #0b0b0d 31%),
      repeating-radial-gradient(circle, rgb(255 255 255 / 0.06) 0 1px, transparent 1px 3px),
      linear-gradient(135deg, #6a7bd8, #c45c8a);
    box-shadow: 0 6px 16px rgb(0 0 0 / 0.5);
  }
  .preview.square span {
    border-radius: var(--r-xs);
    box-shadow: 0 6px 16px rgb(0 0 0 / 0.5);
  }
  .segmented {
    display: flex;
    gap: 4px;
    padding: 4px;
    border-radius: var(--r-pill);
    background: rgb(0 0 0 / 0.25);
  }
  .segmented .chip {
    height: 30px;
    box-shadow: none;
    background: transparent;
  }
  .segmented .chip[aria-checked="true"] {
    background: var(--ink);
    color: #050506;
  }
  .segmented .chip:disabled {
    opacity: 0.5;
    cursor: default;
  }

  /* DSP editors: a compact grid of inputs. */
  .text,
  .num {
    background: rgb(0 0 0 / 0.25);
    color: var(--ink);
    font: inherit;
    font-size: var(--fs-small);
    padding: 8px 10px;
    border-radius: var(--r-sm);
    box-shadow: inset 0 0 0 1px var(--hairline);
  }
  .text {
    min-width: 140px;
  }
  .num {
    width: 96px;
  }
  .band {
    display: grid;
    grid-template-columns: minmax(110px, 1.2fr) 1fr 1fr 1fr auto auto;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-radius: var(--r-md);
  }
  .band:hover {
    background: var(--soft);
  }
  .band .num,
  .band .text {
    width: 100%;
    min-width: 0;
  }
  .note {
    margin: 0;
    padding: 0 14px 14px;
    color: var(--ink-2);
    font-size: var(--fs-small);
  }
  .notes {
    white-space: pre-wrap;
    max-height: 12em;
    overflow-y: auto;
  }
  @media (max-width: 720px) {
    .band {
      grid-template-columns: 1fr 1fr;
    }
  }
</style>
