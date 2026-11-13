<script lang="ts">
  import { onDestroy } from "svelte";
  import { app } from "../lib/app.svelte";
  import {
    commands,
    events,
    type BiquadKindSetting,
    type CoverStyle,
    type Device,
    type DsdMode,
    type DsdPcmRate,
    type FilterPhaseSetting,
    type FilterSourceSetting,
    type FilterVariantSetting,
    type GlassQuality,
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
  import Icon from "./Icon.svelte";

  let roots = $state<RootFolder[]>([]);
  let devices = $state<Device[] | null>(null);
  let followSymlinks = $state(false);
  let adding = $state(false);
  let showFailures = $state(false);
  let volumeDraft = $state(-10);

  const coverStyles: { value: CoverStyle; label: string; hint: string }[] = [
    { value: "vinyl", label: "Vinyl record", hint: "The cover spins as a record; drag the tonearm to seek" },
    { value: "square", label: "Square cover", hint: "The flat album cover" },
  ];
  const glassLevels: { value: GlassQuality; label: string }[] = [
    { value: "full", label: "Full" },
    { value: "reduced", label: "Reduced" },
    { value: "off", label: "Off" },
  ];

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
        return "Looking for music files…";
      case "reading":
        return s.total ? `Reading tags ${s.done} / ${s.total}` : "Reading tags…";
      case "writing":
        return "Updating the library…";
      default:
        return s.last;
    }
  });

  let bitPerfect = $derived(app.player.volumeDb == null);
  $effect(() => {
    if (app.player.volumeDb != null) volumeDraft = app.player.volumeDb;
  });
  let selected = $derived(devices?.find((d) => d.selected));

  function setVolume(db: number | null) {
    app.run(commands.playerSetVolume(db));
  }

  const dsdModes: { value: DsdMode; label: string; hint: string }[] = [
    { value: "dop", label: "DoP", hint: "Bit-perfect DSD over PCM; converted to PCM if the device does not take DoP" },
    { value: "native", label: "Native", hint: "Native DSD where the driver offers it (Linux ALSA), else DoP, else PCM" },
    { value: "pcm", label: "PCM", hint: "Always converted to PCM" },
  ];
  const dsdRates: { value: DsdPcmRate; label: string }[] = [
    { value: "hz176400", label: rate(176400) },
    { value: "hz352800", label: rate(352800) },
  ];
  const pcmToDsdOptions: { value: PcmToDsd; label: string }[] = [
    { value: "off", label: "Off" },
    { value: "dsd64", label: "DSD64" },
    { value: "dsd128", label: "DSD128" },
    { value: "dsd256", label: "DSD256" },
  ];
  const sdmOrders: { value: SdmOrderSetting; label: string }[] = [
    { value: "order5", label: "5th" },
    { value: "order7", label: "7th" },
  ];
  let dsdMode = $derived<DsdMode>((selected && app.settings.dsdModes?.[selected.id]) || "dop");

  function setDsdMode(mode: DsdMode) {
    if (!selected) return;
    app.saveSettings({ ...app.settings, dsdModes: { ...app.settings.dsdModes, [selected.id]: mode } });
  }

  function formatsSummary(d: Device): string {
    if (!d.formats.length) return "Formats unknown (device busy, or exclusive mode disabled)";
    const rates = [...new Set(d.formats.map((f) => f.rate))].sort((a, b) => a - b);
    const bits = Math.max(...d.formats.map((f) => f.bits));
    return `Up to ${bits}-bit · ${rates.map(rate).join(", ")}`;
  }

  // ----- DSP: PEQ, levelling, dither -----

  const levelingModes: { value: LevelingSetting; label: string; hint: string }[] = [
    { value: "off", label: "Off", hint: "No level changes" },
    { value: "track", label: "Track", hint: "Every track to the target loudness" },
    { value: "album", label: "Album", hint: "The album to the target, keeping tracks relative" },
    { value: "auto", label: "Auto", hint: "Album while an album plays in order, track otherwise" },
  ];
  const biquadKinds: { value: BiquadKindSetting; label: string }[] = [
    { value: "peak", label: "Peak" },
    { value: "lowShelf", label: "Low shelf" },
    { value: "highShelf", label: "High shelf" },
    { value: "lowPass", label: "Low pass" },
    { value: "highPass", label: "High pass" },
    { value: "bandPass", label: "Band pass" },
    { value: "notch", label: "Notch" },
  ];
  const srcTargets: { value: SrcTargetSetting; label: string }[] = [
    { value: "off", label: "Off" },
    { value: "maxDevice", label: "Device max" },
    { value: "keepFamily", label: "Keep family" },
    { value: "fixed", label: "Fixed" },
  ];
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
    <h1 tabindex="-1" data-view-heading>Settings</h1>
  </header>

  <div class="cards">
    <!-- Output: the most important state in an audiophile player, up front. -->
    <section class="card hero soft" aria-labelledby="out-title">
      <div class="hero-text">
        <span class="state" class:ok={bitPerfect}>
          <span class="dot" aria-hidden="true"></span>
          {bitPerfect ? "Bit-perfect" : `Software volume ${volumeDraft.toFixed(1)} dB`}
        </span>
        <h2 id="out-title">{selected?.name ?? "Output"}</h2>
        <p class="dim">
          Exclusive mode, each file at its own rate and bit depth. A file the device cannot take
          as-is is skipped, never converted.
        </p>
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
            <span class="name">Bit-perfect output</span>
            <span class="dim small">No software volume; use your DAC or amplifier volume</span>
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
              aria-label="Software volume"
              aria-valuetext="{volumeDraft.toFixed(1)} dB"
              bind:value={volumeDraft}
              onchange={() => setVolume(volumeDraft)}
            />
            <span class="badge warn">{volumeDraft.toFixed(1)} dB</span>
          </div>
        {/if}
      </div>
    </section>

    <section aria-labelledby="dev-title">
      <div class="group-head">
        <h3 id="dev-title">Output device</h3>
        <button class="icon-btn" aria-label="Refresh devices" onclick={loadDevices}><Icon name="refresh" size={18} /></button>
      </div>
      <div class="card soft" role="radiogroup" aria-labelledby="dev-title">
        {#if devices === null}
          <p class="dim pad">Looking for devices…</p>
        {:else if !devices.length}
          <p class="dim pad">No output devices found.</p>
        {:else}
          {#each devices as d (d.id)}
            <label class="row choice" class:selected={d.selected}>
              <span class="icon"><Icon name="speaker" size={18} /></span>
              <span class="info">
                <span class="name">{d.name}{d.isDefault ? " · system default" : ""}</span>
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
      <div class="group-head"><h3 id="dsd-title">DSD playback</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="speaker" size={18} /></span>
          <span class="info">
            <span class="name" id="dsd-mode-label">DSD on {selected?.name ?? "this device"}</span>
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
            <span class="name" id="dsd-rate-label">PCM conversion rate</span>
            <span class="dim small">Used when DSD is converted to PCM</span>
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
            <span class="name" id="pcm-dsd-label">PCM to DSD</span>
            <span class="dim small">
              Modulates PCM tracks before output. DSD is carried over DoP or native DSD when the
              device takes it; 0 dBFS PCM becomes 50 % modulation, the SACD convention.
              {#if app.settings.pcmToDsd === "dsd256"}
                DSD256 uses up to most of a CPU core; if playback drops out, choose DSD128.
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
              <span class="name" id="sdm-order-label">Modulator order</span>
              <span class="dim small">The 7th-order loop shapes harder; the 5th order is more forgiving.</span>
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

    <section aria-labelledby="peq-title">
      <div class="group-head"><h3 id="peq-title">Parametric EQ</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="palette" size={18} /></span>
          <span class="info">
            <span class="name">Preset</span>
            <span class="dim small">Bands run in order behind the preamp. Saving stores the current bands.</span>
          </span>
          <select
            class="text"
            aria-label="EQ preset"
            value={app.settings.peqPreset ?? ""}
            onchange={(e) => applyPreset(e.currentTarget.value)}
          >
            <option value="">Custom</option>
            {#each Object.keys(peqPresets) as name (name)}
              <option value={name}>{name}</option>
            {/each}
          </select>
        </div>
        <div class="row">
          <span class="icon"><Icon name="volume" size={18} /></span>
          <span class="info">
            <span class="name">Preamp</span>
            <span class="dim small">Negative headroom keeps boosted bands below full scale.</span>
          </span>
          <input
            class="range"
            type="range"
            min="-24"
            max="12"
            step="0.5"
            aria-label="EQ preamp"
            aria-valuetext="{(peq.preampDb ?? 0).toFixed(1)} dB"
            value={peq.preampDb ?? 0}
            onchange={(e) => savePeq({ ...peq, preampDb: Number(e.currentTarget.value) })}
          />
          <span class="badge warn">{(peq.preampDb ?? 0).toFixed(1)} dB</span>
        </div>
        {#each peq.bands ?? [] as band, i (i)}
          <div class="band">
            <select
              class="text"
              aria-label="Band {i + 1} type"
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
              aria-label="Band {i + 1} frequency in Hz"
              value={band.freqHz ?? 1000}
              onchange={(e) => setNumberField(e, (v) => setBand(i, { freqHz: v }))}
            />
            <input
              class="num"
              type="number"
              min="-24"
              max="24"
              step="0.5"
              aria-label="Band {i + 1} gain in dB"
              value={band.gainDb ?? 0}
              onchange={(e) => setNumberField(e, (v) => setBand(i, { gainDb: v }))}
            />
            <input
              class="num"
              type="number"
              min="0.1"
              max="20"
              step="0.1"
              aria-label="Band {i + 1} Q"
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
              On
            </label>
            <button class="link" onclick={() => removeBand(i)}>Remove</button>
          </div>
        {/each}
        <div class="row actions">
          <button class="btn small" onclick={addBand}><Icon name="add" size={16} /> Add band</button>
          <input class="text" placeholder="Preset name" aria-label="New preset name" bind:value={presetName} />
          <button class="btn small" disabled={!presetName.trim()} onclick={savePreset}>Save preset</button>
          {#if app.settings.peqPreset}
            <button class="btn small" onclick={deletePreset}>Delete preset</button>
          {/if}
        </div>
      </div>
    </section>

    <section aria-labelledby="level-title">
      <div class="group-head"><h3 id="level-title">Loudness levelling and dither</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="volume" size={18} /></span>
          <span class="info">
            <span class="name" id="level-mode-label">Levelling</span>
            <span class="dim small">
              {levelingModes.find((m) => m.value === (app.settings.leveling ?? "off"))?.hint}
              Boosts stop at the track peak.
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
            <span class="name">Target loudness</span>
            <span class="dim small">Integrated loudness levelling aims for, in LUFS.</span>
          </span>
          <input
            class="num"
            type="number"
            min="-30"
            max="-5"
            step="0.5"
            aria-label="Levelling target in LUFS"
            value={app.settings.levelingTargetLufs ?? -18}
            onchange={setTargetLufs}
          />
          <span class="badge">LUFS</span>
        </div>
        <div class="row">
          <span class="icon"><Icon name="disc" size={18} /></span>
          <span class="info">
            <span class="name">Dither</span>
            <span class="dim small">
              TPDF dither when the device takes fewer bits than the processed signal.
            </span>
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
            <span class="name">Measure loudness</span>
            <span class="dim small">
              Tracks without ReplayGain tags are decoded and measured (EBU R128) so levelling can
              use them. This runs by itself after a scan or a folder change.
              {#if app.loudness.pending != null}{app.loudness.pending} waiting.{/if}
            </span>
          </span>
          <button
            class="btn small"
            disabled={app.loudness.running || app.loudness.pending === 0}
            onclick={() => app.analyzeLoudness()}
          >
            {app.loudness.running
              ? app.loudness.total
                ? `Analysing ${app.loudness.done}/${app.loudness.total}…`
                : "Analysing…"
              : "Analyse"}
          </button>
        </div>
        {#if app.loudness.last}
          <p class="note" aria-live="polite">{app.loudness.last}</p>
        {/if}
      </div>
    </section>

    <section aria-labelledby="src-title">
      <div class="group-head"><h3 id="src-title">Upsampling</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="disc" size={18} /></span>
          <span class="info">
            <span class="name" id="src-target-label">Resample to</span>
            <span class="dim small">Off keeps the source rate unless the device refuses it.</span>
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
              <span class="name">Target rate</span>
              <span class="dim small">Must be one the device accepts, or the fallback picks another.</span>
            </span>
            <select
              class="text"
              aria-label="Fixed target rate"
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
            <span class="name" id="filter-source-label">Filters</span>
            <span class="dim small">
              Sonder designs its own at runtime. Roon tables load from a folder you own and are never
              bundled; a missing table falls back per ratio.
            </span>
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
              <span class="path" title={src.roonFolder ?? ""}>{src.roonFolder ?? "No folder chosen"}</span>
              <span class="dim small">Folder holding Roon's polyphase tables.</span>
            </span>
            <button class="btn small" onclick={pickRoonFolder}>Choose…</button>
          </div>
        {/if}
        <div class="row">
          <span class="icon"><Icon name="disc" size={18} /></span>
          <span class="info">
            <span class="name" id="phase-label">Phase</span>
            <span class="dim small">Linear keeps transients symmetric; minimum trades that for latency.</span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="phase-label">
            {#each [{ value: "linear", label: "Linear" }, { value: "minimum", label: "Minimum" }] as phase (phase.value)}
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
            <span class="name" id="variant-label">Filter</span>
            <span class="dim small">Precise has the steepest stopband; Smooth is shorter and lighter.</span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="variant-label">
            {#each [{ value: "precise", label: "Precise" }, { value: "smooth", label: "Smooth" }] as variant (variant.value)}
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
            <span class="name">Headroom</span>
            <span class="dim small">Inserted before resampling so interpolation overshoot cannot clip.</span>
          </span>
          <input
            class="num"
            type="number"
            min="-60"
            max="0"
            step="0.5"
            aria-label="Headroom in dB"
            value={src.headroomDb ?? -3}
            onchange={(e) =>
              setNumberField(e, (v) => saveSrc({ headroomDb: Number.isFinite(v) ? v : null }))}
          />
          <span class="badge">dB</span>
        </div>
        <div class="row">
          <span class="icon"><Icon name="refresh" size={18} /></span>
          <span class="info">
            <span class="name">Automatic fallback</span>
            <span class="dim small">
              Resample when the device refuses the source rate, labelled not bit-perfect.
            </span>
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
            <span class="name">Lift rates below 44.1 kHz</span>
            <span class="dim small">Bring 22.05/32 kHz material up into its own family.</span>
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
      <div class="group-head"><h3 id="lib-title">Library folders</h3></div>
      <div class="card soft">
        {#each roots as root (root.path)}
          <div class="row">
            <span class="icon"><Icon name="folder" size={18} /></span>
            <span class="info">
              <span class="path" title={root.path}>{root.path}</span>
              {#if root.followSymlinks}<span class="dim small">Follows symbolic links</span>{/if}
            </span>
            <button class="btn small" onclick={() => app.run(commands.libraryRemoveRoot(root.path))}>Remove</button>
          </div>
        {:else}
          <p class="dim pad">No folders yet. Add one to start your library.</p>
        {/each}
        <div class="row actions">
          <button class="btn primary small" disabled={adding} onclick={addRoot}><Icon name="add" size={16} /> Add folder…</button>
          <label class="inline">
            <input type="checkbox" role="switch" class="switch" bind:checked={followSymlinks} />
            Follow symbolic links
          </label>
          <span class="grow"></span>
          <button class="btn small" disabled={scanning || !roots.length} onclick={() => app.run(commands.libraryRescan())}>
            <Icon name="refresh" size={16} /> Rescan
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
                {showFailures ? "Hide" : "Show"} unreadable files
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
      <div class="group-head"><h3 id="look-title">Appearance</h3></div>
      <div class="card soft">
        <div class="tiles" role="radiogroup" aria-label="Now Playing cover">
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
            <span class="name" id="glass-label">Frosted glass</span>
            <span class="dim small">Blur behind the floating panels. Lower it if scrolling stutters.</span>
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
      <div class="group-head"><h3 id="lyrics-title">Lyrics</h3></div>
      <div class="card soft">
        <div class="row">
          <span class="icon"><Icon name="lyrics" size={18} /></span>
          <span class="info">
            <span class="name" id="lyrics-online-label">Find lyrics online</span>
            <span class="dim small">
              Lyrics in the file or in a .lrc file beside it come first. Otherwise the title, artist, album and
              length are sent to LRCLIB, BetterLyrics, LyricsPlus and Kugou. Results stay on this computer.
            </span>
          </span>
          <div class="segmented" role="radiogroup" aria-labelledby="lyrics-online-label">
            {#each [true, false] as on (on)}
              <button
                class="chip"
                role="radio"
                aria-checked={(app.settings.lyricsOnline ?? true) === on}
                onclick={() => app.saveSettings({ ...app.settings, lyricsOnline: on })}
              >
                {on ? "On" : "Off"}
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
    font-size: 15px;
    font-weight: 700;
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

  .row {
    display: grid;
    grid-template-columns: 40px 1fr auto;
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
    grid-template-columns: 40px 1fr auto;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-standard);
  }
  .choice:hover {
    background: var(--soft);
  }
  .choice.selected {
    background: var(--soft-hover);
  }
  .choice:focus-within {
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
    accent-color: var(--ink);
    padding: 0;
    background: none;
    box-shadow: none;
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
  .tile:focus-within {
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
  @media (max-width: 720px) {
    .band {
      grid-template-columns: 1fr 1fr;
    }
  }
</style>
