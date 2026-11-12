<script lang="ts">
  import { onDestroy } from "svelte";
  import { app } from "../lib/app.svelte";
  import {
    commands,
    events,
    type CoverStyle,
    type Device,
    type GlassQuality,
    type RootFolder,
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

  function formatsSummary(d: Device): string {
    if (!d.formats.length) return "Formats unknown (device busy, or exclusive mode disabled)";
    const rates = [...new Set(d.formats.map((f) => f.rate))].sort((a, b) => a - b);
    const bits = Math.max(...d.formats.map((f) => f.bits));
    return `Up to ${bits}-bit · ${rates.map(rate).join(", ")}`;
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
</style>
