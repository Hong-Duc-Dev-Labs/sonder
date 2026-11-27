<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { app } from "../lib/app.svelte";
  import {
    commands,
    events,
    type Device,
    type GdriveSyncedFolder,
    type GdriveSyncPhase,
    type GlassQuality,
    type LanguageSetting,
    type OutputShareMode,
    type RootFolder,
  } from "../lib/bindings";
  import { rate } from "../lib/format";
  import { i18n, t } from "../lib/i18n.svelte";
  import { reducedMotion } from "../lib/motion";
  import { fadeQuick } from "../lib/transitions";
  import {
    WELCOME_STEPS,
    WELCOME_STEP_COUNT,
    canJumpToStep,
    nextStep,
    prevStep,
  } from "../lib/welcome";
  import Icon from "./Icon.svelte";

  const FOCUSABLE_SELECTOR =
    'a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';

  let wizardEl: HTMLDivElement | undefined = $state();
  let step = $state(0);
  let highestVisited = $state(0);
  let direction = $state<1 | -1>(1);
  let finishing = $state(false);
  let alive = true;
  let previousFocus: HTMLElement | null = null;

  // Step 2: Music sources
  let roots = $state<RootFolder[]>([]);
  let addingFolder = $state(false);
  let removingRoots = $state<Set<string>>(new Set());
  let gdriveFolders = $state<GdriveSyncedFolder[]>([]);
  let linkDraft = $state("");
  let addingLink = $state(false);
  let removingGdrive = $state<Set<string>>(new Set());
  let gdriveSyncPhase = $state<GdriveSyncPhase | "idle">("idle");
  let gdriveSyncFolder = $state("");
  let gdriveSyncDone = $state(0);
  let gdriveSyncTotal = $state(0);

  // Step 3: Audio devices
  let devices = $state<Device[] | null>(null);
  let devicesSeq = 0;

  let stepLabels = $derived([
    t("welcome.step.welcome"),
    t("welcome.step.sources"),
    t("welcome.step.audio"),
    t("welcome.step.finish"),
  ]);

  let languages = $derived<{ value: LanguageSetting; label: string }[]>([
    { value: "vi", label: "Tiếng Việt" },
    { value: "en", label: "English" },
    { value: "system", label: t("settings.lang.system") },
  ]);

  let glassLevels = $derived<{ value: GlassQuality; label: string }[]>([
    { value: "full", label: t("settings.look.full") },
    { value: "reduced", label: t("settings.look.reduced") },
    { value: "off", label: t("common.off") },
  ]);

  let outputMode = $derived<OutputShareMode>(app.settings.outputMode ?? "exclusive");
  let lyricsOnline = $derived(app.settings.lyricsOnline ?? true);
  let selectedDevice = $derived(devices?.find((d) => d.selected) ?? null);
  let scanning = $derived(app.scan.phase !== "idle");
  let gdriveSyncText = $derived.by(() => {
    if (gdriveSyncPhase === "listing") {
      return t("settings.gdrive.syncListing", { folder: gdriveSyncFolder || "Google Drive" });
    }
    if (gdriveSyncPhase === "reading") {
      return t("settings.gdrive.syncReading", {
        folder: gdriveSyncFolder || "Google Drive",
        done: gdriveSyncDone,
        total: gdriveSyncTotal,
      });
    }
    if (gdriveSyncPhase === "writing") return t("settings.gdrive.syncWriting");
    return null;
  });

  // Focus the active step heading on mount and on every step change.
  $effect(() => {
    void step;
    void tick().then(() => {
      if (!alive) return;
      wizardEl?.querySelector<HTMLElement>("[data-welcome-heading]")?.focus({ preventScroll: true });
    });
  });

  // Keep local folders in sync with library changes & scans.
  $effect(() => {
    void app.libraryVersion;
    void app.scan.phase;
    app.run(commands.libraryRoots()).then((r) => {
      if (alive && r) roots = r;
    });
  });

  async function refreshGdriveFolders() {
    const folders = await app.run(commands.gdriveSyncedFolders());
    if (alive && folders) gdriveFolders = folders;
  }

  async function loadDevices() {
    const seq = ++devicesSeq;
    devices = null;
    const found = (await app.run(commands.devicesList())) ?? [];
    if (alive && seq === devicesSeq) devices = found;
  }

  void refreshGdriveFolders();
  void loadDevices();

  const unlistenDevices = events.devicesChangedEvent.listen((e) => {
    if (!alive) return;
    devicesSeq++;
    devices = e.payload;
  });
  const unlistenGdriveProgress = events.gdriveSyncProgressEvent.listen((e) => {
    if (!alive) return;
    gdriveSyncPhase = e.payload.phase;
    gdriveSyncFolder = e.payload.folderName;
    gdriveSyncDone = e.payload.done;
    gdriveSyncTotal = e.payload.total;
  });
  const unlistenGdriveFinished = events.gdriveSyncFinishedEvent.listen(() => {
    if (!alive) return;
    gdriveSyncPhase = "idle";
    void refreshGdriveFolders();
  });

  onMount(() => {
    if (typeof document !== "undefined") {
      previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    }
  });

  onDestroy(() => {
    alive = false;
    unlistenDevices.then((f) => f());
    unlistenGdriveProgress.then((f) => f());
    unlistenGdriveFinished.then((f) => f());
    if (typeof document !== "undefined") {
      const target = previousFocus?.isConnected
        ? previousFocus
        : document.querySelector<HTMLElement>("[data-view-heading]");
      // Restore focus after `inert` is cleared on the underlying shell.
      queueMicrotask(() => target?.focus({ preventScroll: true }));
    }
  });

  function stepIn(_node: Element, { dir }: { dir: 1 | -1 }) {
    if (reducedMotion()) return { duration: 1, css: (t: number) => `opacity: ${t}` };
    return {
      duration: 240,
      easing: cubicOut,
      css: (t: number, u: number) => `opacity: ${t}; transform: translateX(${dir * u * 18}px)`,
    };
  }

  function onKeyDown(e: KeyboardEvent) {
    if (app.dialog) return;
    if (e.key === "Escape") {
      e.preventDefault();
      void complete();
      return;
    }
    if (e.key === "Tab" && wizardEl) {
      const focusable = [...wizardEl.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)].filter(
        (el) => el.offsetParent !== null,
      );
      if (!focusable.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement as HTMLElement | null;
      if (e.shiftKey) {
        if (!active || !wizardEl.contains(active) || active === first) {
          e.preventDefault();
          last.focus();
        }
      } else if (!active || !wizardEl.contains(active) || active === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }

  function goTo(target: number) {
    if (target === step || !canJumpToStep(target, highestVisited)) return;
    direction = target > step ? 1 : -1;
    step = target;
  }

  function goNext() {
    if (step >= WELCOME_STEP_COUNT - 1) return;
    direction = 1;
    step = nextStep(step);
    highestVisited = Math.max(highestVisited, step);
  }

  function goBack() {
    if (step <= 0) return;
    direction = -1;
    step = prevStep(step);
  }

  async function complete() {
    if (finishing) return;
    finishing = true;
    await app.completeWelcomeSetup();
    if (alive) finishing = false;
  }

  async function addLocalFolder() {
    if (addingFolder) return;
    addingFolder = true;
    const picked = await app.run(commands.libraryAddRoot(false));
    if (!alive) return;
    addingFolder = false;
    if (picked) {
      const next = await app.run(commands.libraryRoots());
      if (alive && next) roots = next;
    }
  }

  async function removeLocalFolder(path: string) {
    if (removingRoots.has(path)) return;
    removingRoots = new Set([...removingRoots, path]);
    await app.run(commands.libraryRemoveRoot(path));
    if (!alive) return;
    const next = await app.run(commands.libraryRoots());
    if (!alive) return;
    if (next) roots = next;
    const remaining = new Set(removingRoots);
    remaining.delete(path);
    removingRoots = remaining;
  }

  async function addPublicLink(e?: SubmitEvent) {
    e?.preventDefault();
    const raw = linkDraft.trim();
    if (!raw || addingLink) return;
    addingLink = true;
    const added = await app.run(commands.gdriveAddPublicLink(raw));
    if (!alive) return;
    addingLink = false;
    if (added) {
      linkDraft = "";
      app.notify("info", t("settings.gdrive.linkAdded", { name: added.name }));
      await refreshGdriveFolders();
    }
  }

  async function removeGdriveFolder(folderId: string) {
    if (removingGdrive.has(folderId)) return;
    removingGdrive = new Set([...removingGdrive, folderId]);
    await app.run(commands.gdriveRemoveFolder(folderId));
    if (!alive) return;
    await refreshGdriveFolders();
    if (!alive) return;
    const remaining = new Set(removingGdrive);
    remaining.delete(folderId);
    removingGdrive = remaining;
  }

  async function selectDevice(id: string) {
    const ok = await app.selectDevice(id);
    if (alive && ok && devices) {
      devices = devices.map((d) => ({ ...d, selected: d.id === id }));
    }
  }

  function setOutputMode(mode: OutputShareMode) {
    if (mode !== outputMode) {
      void app.patchSettings({ outputMode: mode });
    }
  }

  function formatsSummary(d: Device): string {
    if (!d.formats.length) return t("settings.formatsUnknown");
    const rates = [...new Set(d.formats.map((f) => f.rate))].sort((a, b) => a - b);
    const bits = Math.max(...d.formats.map((f) => f.bits));
    return t("settings.formatsUpTo", { bits: String(bits), rates: rates.map(rate).join(", ") });
  }

  let summaryLanguage = $derived.by(() => {
    const lang = app.settings.language ?? "system";
    if (lang === "vi") return "Tiếng Việt";
    if (lang === "en") return "English";
    const resolved = i18n.locale === "vi" ? "Tiếng Việt" : "English";
    return `${t("settings.lang.system")} (${resolved})`;
  });

  let summarySources = $derived.by(() => {
    const parts: string[] = [];
    if (roots.length > 0) parts.push(t("welcome.s4.summaryLocalCount", { count: roots.length }));
    if (gdriveFolders.length > 0) {
      parts.push(t("welcome.s4.summaryDriveCount", { count: gdriveFolders.length }));
    }
    return parts.length ? parts.join(" · ") : t("welcome.s4.summarySourcesNone");
  });

  let summaryAudio = $derived.by(() => {
    const deviceName = selectedDevice?.name ?? t("welcome.s4.systemDefault");
    const modeLabel =
      outputMode === "exclusive" ? t("welcome.s3.exclusiveTitle") : t("welcome.s3.sharedTitle");
    return `${deviceName} — ${modeLabel}`;
  });
</script>

<svelte:window onkeydown={onKeyDown} />

<div
  bind:this={wizardEl}
  class="wizard"
  role="dialog"
  aria-modal="true"
  aria-label={t("welcome.aria")}
  transition:fadeQuick={{ duration: 200 }}
>
  <div class="wizard-frame">
    <!-- Top progress bar + Skip -->
    <header class="wizard-top">
      <nav class="steps" aria-label={t("welcome.stepProgress", { current: step + 1, total: WELCOME_STEP_COUNT })}>
        {#each WELCOME_STEPS as id, i (id)}
          {@const current = i === step}
          {@const completed = i < step || (i <= highestVisited && !current)}
          {@const clickable = canJumpToStep(i, highestVisited) && !current}
          <button
            type="button"
            class="step-pill"
            class:current
            class:completed
            disabled={!clickable && !current}
            aria-label={stepLabels[i]}
            title={stepLabels[i]}
            aria-current={current ? "step" : undefined}
            onclick={() => goTo(i)}
          >
            <span class="step-dot" aria-hidden="true">
              {#if completed && i < step}
                <Icon name="check" size={11} />
              {:else}
                {i + 1}
              {/if}
            </span>
            <span class="step-label">{stepLabels[i]}</span>
          </button>
          {#if i < WELCOME_STEP_COUNT - 1}
            <span class="step-line" class:filled={i < highestVisited} aria-hidden="true"></span>
          {/if}
        {/each}
      </nav>

      <button type="button" class="skip-btn" disabled={finishing} onclick={complete}>
        {t("welcome.skip")}
      </button>
    </header>

    <!-- Scrollable step content -->
    <div class="wizard-body scroll">
      {#key step}
        <div class="step-panel" in:stepIn={{ dir: direction }}>
          {#if step === 0}
            <!-- Step 1: Welcome, Language & Appearance -->
            <div class="hero-welcome">
              <div class="emblem" aria-hidden="true">
                <svg viewBox="0 0 64 64" width="52" height="52">
                  <circle cx="32" cy="32" r="29" fill="#0b0b0d" stroke="currentColor" stroke-opacity=".24" stroke-width="1.5" />
                  <circle cx="32" cy="32" r="21" fill="none" stroke="currentColor" stroke-opacity=".16" stroke-width="1" />
                  <circle cx="32" cy="32" r="14" fill="none" stroke="currentColor" stroke-opacity=".12" stroke-width="1" />
                  <circle cx="32" cy="32" r="8.5" fill="currentColor" />
                  <circle cx="32" cy="32" r="2.2" fill="#0b0b0d" />
                </svg>
              </div>
              <h1 class="greeting" tabindex="-1" data-welcome-heading>{t("welcome.s1.greeting")}</h1>
              <p class="lead">{t("welcome.s1.subtitle")}</p>
            </div>

            <div class="section-stack">
              <section class="card soft" aria-labelledby="w-lang-label">
                <div class="row">
                  <span class="icon"><Icon name="globe" size={18} /></span>
                  <span class="info">
                    <span class="name" id="w-lang-label">{t("settings.lang.name")}</span>
                    <span class="dim small">{t("settings.lang.hint")}</span>
                  </span>
                  <div class="segmented" role="radiogroup" aria-labelledby="w-lang-label">
                    {#each languages as language (language.value)}
                      <button
                        type="button"
                        class="chip"
                        role="radio"
                        lang={language.value === "system" ? undefined : language.value}
                        aria-checked={(app.settings.language ?? "system") === language.value}
                        onclick={() => app.patchSettings({ language: language.value })}
                      >
                        {language.label}
                      </button>
                    {/each}
                  </div>
                </div>
              </section>

              <section class="card soft" aria-labelledby="w-look-label">
                <div class="card-kicker" id="w-look-label">{t("welcome.s1.appearance")}</div>
                <div class="row">
                  <span class="icon"><Icon name="palette" size={18} /></span>
                  <span class="info">
                    <span class="name" id="w-glass-label">{t("settings.look.glass")}</span>
                    <span class="dim small">{t("settings.look.glassHint")}</span>
                  </span>
                  <div class="segmented" role="radiogroup" aria-labelledby="w-glass-label">
                    {#each glassLevels as level (level.value)}
                      <button
                        type="button"
                        class="chip"
                        role="radio"
                        aria-checked={(app.settings.glass ?? "full") === level.value}
                        onclick={() => app.patchSettings({ glass: level.value })}
                      >
                        {level.label}
                      </button>
                    {/each}
                  </div>
                </div>

                <div class="row">
                  <span class="icon"><Icon name="spectrum" size={18} /></span>
                  <span class="info">
                    <span class="name" id="w-fog-label">{t("settings.look.fogBackdrop")}</span>
                    <span class="dim small">{t("settings.look.fogBackdropHint")}</span>
                  </span>
                  <div class="segmented" role="radiogroup" aria-labelledby="w-fog-label">
                    {#each [true, false] as on (on)}
                      <button
                        type="button"
                        class="chip"
                        role="radio"
                        aria-checked={(app.settings.fogBackdrop ?? true) === on}
                        onclick={() => app.patchSettings({ fogBackdrop: on })}
                      >
                        {on ? t("common.on") : t("common.off")}
                      </button>
                    {/each}
                  </div>
                </div>

                <div class="row">
                  <span class="icon"><Icon name="albums" size={18} /></span>
                  <span class="info">
                    <span class="name" id="w-grow-label">{t("settings.look.growFromCover")}</span>
                    <span class="dim small">{t("settings.look.growFromCoverHint")}</span>
                  </span>
                  <div class="segmented" role="radiogroup" aria-labelledby="w-grow-label">
                    {#each [true, false] as on (on)}
                      <button
                        type="button"
                        class="chip"
                        role="radio"
                        aria-checked={(app.settings.growFromCover ?? true) === on}
                        onclick={() => app.patchSettings({ growFromCover: on })}
                      >
                        {on ? t("common.on") : t("common.off")}
                      </button>
                    {/each}
                  </div>
                </div>
              </section>
            </div>

          {:else if step === 1}
            <!-- Step 2: Music Sources (Local & Google Drive) -->
            <div class="step-head">
              <h1 tabindex="-1" data-welcome-heading>{t("welcome.s2.title")}</h1>
              <p class="lead">{t("welcome.s2.subtitle")}</p>
            </div>

            <div class="section-stack">
              <!-- Local folders -->
              <section class="card soft" aria-labelledby="w-local-label">
                <div class="card-head">
                  <span class="card-kicker" id="w-local-label">{t("welcome.s2.localTitle")}</span>
                  <button
                    type="button"
                    class="btn primary small"
                    disabled={addingFolder}
                    onclick={addLocalFolder}
                  >
                    <Icon name="add" size={15} />
                    {t("settings.lib.addFolder")}
                  </button>
                </div>

                {#each roots as root (root.path)}
                  <div class="row">
                    <span class="icon"><Icon name="folder" size={18} /></span>
                    <span class="info">
                      <span class="path" title={root.path}>{root.path}</span>
                    </span>
                    <button
                      type="button"
                      class="btn small"
                      disabled={removingRoots.has(root.path)}
                      onclick={() => removeLocalFolder(root.path)}
                    >
                      {t("common.remove")}
                    </button>
                  </div>
                {:else}
                  <p class="dim small empty-pad">{t("welcome.s2.localEmpty")}</p>
                {/each}

                {#if scanning}
                  <div class="scan-note" aria-live="polite">
                    <span class="spinner" aria-hidden="true"></span>
                    <span class="dim small">
                      {app.scan.total
                        ? t("settings.lib.readingProgress", { done: app.scan.done, total: app.scan.total })
                        : t("settings.lib.walking")}
                    </span>
                  </div>
                {/if}
              </section>

              <!-- Google Drive public link -->
              <section class="card soft" aria-labelledby="w-gdrive-label">
                <div class="card-kicker" id="w-gdrive-label">{t("welcome.s2.gdriveTitle")}</div>
                <form class="paste-block" onsubmit={addPublicLink}>
                  <p class="dim small paste-hint">{t("settings.gdrive.pasteHint")}</p>
                  <div class="paste-row">
                    <input
                      type="text"
                      class="text-input"
                      bind:value={linkDraft}
                      placeholder={t("settings.gdrive.pastePlaceholder")}
                      autocomplete="off"
                      spellcheck="false"
                    />
                    <button
                      type="submit"
                      class="btn primary small"
                      disabled={addingLink || !linkDraft.trim()}
                    >
                      <Icon name="cloud" size={15} />
                      {addingLink ? t("settings.gdrive.addingLink") : t("settings.gdrive.addAndSync")}
                    </button>
                  </div>
                </form>

                {#each gdriveFolders as folder (folder.folderId)}
                  <div class="row">
                    <span class="icon">
                      <Icon name={folder.remoteKind === "file" ? "note" : "cloud"} size={18} />
                    </span>
                    <span class="info">
                      <span class="name">{folder.name}</span>
                      <span class="dim small">
                        {t("settings.gdrive.trackCount", { count: folder.trackCount })}
                      </span>
                    </span>
                    <button
                      type="button"
                      class="btn small"
                      disabled={removingGdrive.has(folder.folderId)}
                      onclick={() => removeGdriveFolder(folder.folderId)}
                    >
                      {t("common.remove")}
                    </button>
                  </div>
                {:else}
                  <p class="dim small empty-pad">{t("welcome.s2.gdriveEmpty")}</p>
                {/each}

                {#if gdriveSyncText}
                  <div class="scan-note" aria-live="polite">
                    <span class="spinner" aria-hidden="true"></span>
                    <span class="dim small">{gdriveSyncText}</span>
                  </div>
                {/if}
              </section>
            </div>

          {:else if step === 2}
            <!-- Step 3: Audio Output & Playback Mode -->
            <div class="step-head">
              <h1 tabindex="-1" data-welcome-heading>{t("welcome.s3.title")}</h1>
              <p class="lead">{t("welcome.s3.subtitle")}</p>
            </div>

            <div class="section-stack">
              <!-- Output devices -->
              <section class="card soft" aria-labelledby="w-dev-label">
                <div class="card-head">
                  <span class="card-kicker" id="w-dev-label">{t("settings.outputDevice")}</span>
                  <button
                    type="button"
                    class="icon-btn refresh-btn"
                    aria-label={t("settings.refreshDevices")}
                    title={t("settings.refreshDevices")}
                    onclick={loadDevices}
                  >
                    <Icon name="refresh" size={16} />
                  </button>
                </div>

                <div class="device-list" role="radiogroup" aria-labelledby="w-dev-label">
                  {#if devices === null}
                    <p class="dim small empty-pad">{t("settings.lookingForDevices")}</p>
                  {:else if !devices.length}
                    <p class="dim small empty-pad">{t("settings.noDevices")}</p>
                  {:else}
                    {#each devices as d (d.id)}
                      <label class="row choice" class:selected={d.selected}>
                        <span class="icon"><Icon name="speaker" size={18} /></span>
                        <span class="info">
                          <span class="name">
                            {d.name}{d.isDefault ? ` · ${t("settings.systemDefault")}` : ""}
                          </span>
                          <span class="dim small">{formatsSummary(d)}</span>
                        </span>
                        <input
                          class="sr"
                          type="radio"
                          name="welcome-device"
                          checked={d.selected}
                          onchange={() => selectDevice(d.id)}
                        />
                        <span class="check" aria-hidden="true"><Icon name="check" size={16} /></span>
                      </label>
                    {/each}
                  {/if}
                </div>
              </section>

              <!-- Exclusive vs Shared playback mode -->
              <section aria-labelledby="w-mode-label">
                <div class="group-kicker" id="w-mode-label">{t("welcome.s3.modeTitle")}</div>
                <div class="mode-grid" role="radiogroup" aria-labelledby="w-mode-label">
                  <button
                    type="button"
                    class="mode-card soft"
                    class:selected={outputMode === "exclusive"}
                    role="radio"
                    aria-checked={outputMode === "exclusive"}
                    onclick={() => setOutputMode("exclusive")}
                  >
                    <div class="mode-top">
                      <span class="mode-title">{t("welcome.s3.exclusiveTitle")}</span>
                      <span class="badge ok">{t("welcome.s3.exclusiveBadge")}</span>
                    </div>
                    <p class="dim small mode-desc">{t("welcome.s3.exclusiveDesc")}</p>
                  </button>

                  <button
                    type="button"
                    class="mode-card soft"
                    class:selected={outputMode === "shared"}
                    role="radio"
                    aria-checked={outputMode === "shared"}
                    onclick={() => setOutputMode("shared")}
                  >
                    <div class="mode-top">
                      <span class="mode-title">{t("welcome.s3.sharedTitle")}</span>
                    </div>
                    <p class="dim small mode-desc">{t("welcome.s3.sharedDesc")}</p>
                  </button>
                </div>
              </section>
            </div>

          {:else}
            <!-- Step 4: Online Lyrics & Finish -->
            <div class="step-head">
              <h1 tabindex="-1" data-welcome-heading>{t("welcome.s4.title")}</h1>
              <p class="lead">{t("welcome.s4.subtitle")}</p>
            </div>

            <div class="section-stack">
              <section class="card soft" aria-labelledby="w-lyrics-label">
                <div class="row">
                  <span class="icon"><Icon name="lyrics" size={18} /></span>
                  <span class="info">
                    <span class="name" id="w-lyrics-label">{t("settings.lyrics.online")}</span>
                    <span class="dim small">{t("welcome.s4.privacyNote")}</span>
                  </span>
                  <div class="segmented" role="radiogroup" aria-labelledby="w-lyrics-label">
                    {#each [true, false] as on (on)}
                      <button
                        type="button"
                        class="chip"
                        role="radio"
                        aria-checked={lyricsOnline === on}
                        onclick={() => app.patchSettings({ lyricsOnline: on })}
                      >
                        {on ? t("common.on") : t("common.off")}
                      </button>
                    {/each}
                  </div>
                </div>
              </section>

              <section class="card soft" aria-labelledby="w-summary-label">
                <div class="card-kicker" id="w-summary-label">{t("welcome.s4.summaryTitle")}</div>
                <dl class="summary-grid">
                  <div class="summary-item">
                    <dt>{t("welcome.s4.summaryLanguage")}</dt>
                    <dd>{summaryLanguage}</dd>
                  </div>
                  <div class="summary-item">
                    <dt>{t("welcome.s4.summarySources")}</dt>
                    <dd>{summarySources}</dd>
                  </div>
                  <div class="summary-item">
                    <dt>{t("welcome.s4.summaryAudio")}</dt>
                    <dd>{summaryAudio}</dd>
                  </div>
                  <div class="summary-item">
                    <dt>{t("welcome.s4.summaryLyrics")}</dt>
                    <dd>{lyricsOnline ? t("common.on") : t("common.off")}</dd>
                  </div>
                </dl>
              </section>
            </div>
          {/if}
        </div>
      {/key}
    </div>

    <!-- Bottom navigation footer -->
    <footer class="wizard-foot">
      <div>
        {#if step > 0}
          <button type="button" class="btn" onclick={goBack}>
            <Icon name="back" size={16} />
            {t("welcome.back")}
          </button>
        {/if}
      </div>

      <span class="step-counter" aria-hidden="true">
        {t("welcome.stepProgress", { current: step + 1, total: WELCOME_STEP_COUNT })}
      </span>

      <div>
        {#if step < WELCOME_STEP_COUNT - 1}
          <button type="button" class="btn primary" onclick={goNext}>
            {t("welcome.next")}
            <Icon name="chevron" size={16} />
          </button>
        {:else}
          <button type="button" class="btn primary" disabled={finishing} onclick={complete}>
            <Icon name="play" size={16} />
            {t("welcome.startListening")}
          </button>
        {/if}
      </div>
    </footer>
  </div>
</div>

<style>
  .wizard {
    position: fixed;
    top: var(--titlebar-h);
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 150;
    display: flex;
    justify-content: center;
    background:
      radial-gradient(ellipse 75% 55% at 50% 0%, rgb(255 255 255 / 0.055), transparent 72%),
      rgb(5 5 7 / 0.96);
    -webkit-backdrop-filter: blur(var(--glass-blur));
    backdrop-filter: blur(var(--glass-blur));
  }
  :global([data-glass="off"]) .wizard {
    background: var(--canvas);
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
  }

  .wizard-frame {
    width: 100%;
    max-width: 740px;
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 18px 28px 22px;
  }

  /* ---------- top progress bar ---------- */
  .wizard-top {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--hairline);
  }
  .steps {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .step-pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 11px 0 6px;
    border-radius: var(--r-pill);
    font-size: var(--fs-small);
    font-weight: 600;
    color: var(--ink-3);
    transition:
      background var(--dur-fast) var(--ease-standard),
      color var(--dur-fast) var(--ease-standard);
  }
  .step-pill:not(:disabled):hover {
    background: var(--soft-hover);
    color: var(--ink);
  }
  .step-pill:disabled {
    cursor: default;
  }
  .step-pill.completed {
    color: var(--ink-2);
  }
  .step-pill.current {
    background: var(--soft-active);
    color: var(--ink);
    box-shadow: inset 0 0 0 1px var(--hairline);
  }
  .step-dot {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-family: var(--font-figures);
    font-size: 11px;
    font-weight: 700;
    background: var(--soft);
    color: var(--ink-3);
    flex: none;
  }
  .step-pill.current .step-dot {
    background: var(--ink);
    color: #050506;
  }
  .step-pill.completed .step-dot {
    background: rgb(111 207 151 / 0.18);
    color: var(--ok);
  }
  .step-line {
    width: 18px;
    height: 1px;
    background: var(--hairline);
    flex: none;
  }
  .step-line.filled {
    background: rgb(255 255 255 / 0.24);
  }
  .skip-btn {
    padding: 6px 12px;
    border-radius: var(--r-pill);
    font-size: var(--fs-small);
    font-weight: 600;
    color: var(--ink-2);
    flex: none;
    transition:
      background var(--dur-fast) var(--ease-standard),
      color var(--dur-fast) var(--ease-standard);
  }
  .skip-btn:hover {
    background: var(--soft-hover);
    color: var(--ink);
  }

  /* ---------- scrollable step body ---------- */
  .wizard-body {
    flex: 1;
    min-height: 0;
    overflow-x: hidden;
    padding: 22px 2px 18px;
  }
  .step-panel {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  /* Step 1 hero */
  .hero-welcome {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 10px;
    padding: 4px 0 6px;
  }
  .emblem {
    width: 56px;
    height: 56px;
    display: grid;
    place-items: center;
    color: var(--ink);
  }
  .greeting {
    font-family: var(--font-signature);
    font-size: clamp(32px, 4vw, 44px);
    font-weight: 400;
    line-height: 1.15;
    letter-spacing: 0;
    color: var(--ink);
  }

  /* Steps 2–4 header */
  .step-head {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .step-head h1 {
    font-size: clamp(24px, 3vw, 30px);
    letter-spacing: -0.03em;
  }
  .lead {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--fs-body);
    line-height: 1.5;
    max-width: 580px;
  }
  .hero-welcome .lead {
    max-width: 480px;
  }

  /* Cards & rows (consistent with SettingsView) */
  .section-stack {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .card {
    border-radius: var(--r-lg);
    padding: 8px;
  }
  .card-kicker {
    font-size: var(--fs-label);
    font-weight: 700;
    letter-spacing: 0.11em;
    text-transform: uppercase;
    color: var(--ink-3);
    padding: 8px 12px 6px;
  }
  .group-kicker {
    font-size: var(--fs-label);
    font-weight: 700;
    letter-spacing: 0.11em;
    text-transform: uppercase;
    color: var(--ink-2);
    padding: 0 6px 8px;
  }
  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 4px 8px 6px 4px;
  }
  .card-head .card-kicker {
    padding: 4px 8px;
  }
  .refresh-btn {
    width: 32px;
    height: 32px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    padding: 10px 12px;
    border-radius: var(--r-md);
  }
  .row + .row {
    border-top: 1px solid var(--hairline);
    border-top-left-radius: 0;
    border-top-right-radius: 0;
  }
  .icon {
    width: 34px;
    height: 34px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: var(--r-sm);
    background: var(--soft);
    color: var(--ink-2);
  }
  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name {
    font-weight: 600;
    color: var(--ink);
  }
  .path {
    font-weight: 550;
    color: var(--ink);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .small {
    font-size: var(--fs-small);
    line-height: 1.4;
  }
  .empty-pad {
    margin: 0;
    padding: 10px 12px 12px;
  }
  .segmented {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 4px;
    padding: 3px;
    border-radius: var(--r-pill);
    background: rgb(0 0 0 / 0.28);
    box-shadow: inset 0 0 0 1px var(--hairline);
    flex: none;
  }
  .segmented .chip {
    height: 30px;
    padding: 0 12px;
    font-size: 12.5px;
    background: transparent;
    box-shadow: none;
  }
  .segmented .chip:hover {
    background: var(--soft-hover);
  }
  .segmented .chip[aria-checked="true"] {
    background: var(--ink);
    color: #050506;
  }

  /* Step 2: Google Drive paste form & scan indicator */
  .paste-block {
    padding: 4px 12px 10px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .paste-hint {
    margin: 0;
  }
  .paste-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .text-input {
    flex: 1;
    min-width: 200px;
    height: 36px;
    padding: 0 14px;
    border-radius: var(--r-pill);
    background: rgb(0 0 0 / 0.34);
    font-size: var(--fs-small);
  }
  .scan-note {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px 10px;
    border-top: 1px solid var(--hairline);
  }
  .spinner {
    width: 13px;
    height: 13px;
    border-radius: 50%;
    border: 2px solid rgb(255 255 255 / 0.2);
    border-top-color: var(--ink);
    animation: spin 0.9s linear infinite;
    flex: none;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* Step 3: Devices & Playback mode cards */
  .device-list {
    max-height: 200px;
    overflow-y: auto;
    scrollbar-width: thin;
  }
  .row.choice {
    position: relative;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-standard);
  }
  .row.choice:hover {
    background: var(--soft-hover);
  }
  .row.choice.selected {
    background: var(--soft-active);
  }
  :global(:root:not([data-modality="pointer"])) .row.choice:focus-within {
    outline: 2px solid var(--ink);
    outline-offset: -2px;
  }
  .row.choice .check {
    opacity: 0;
    color: var(--ok);
    flex: none;
    transition: opacity var(--dur-fast) var(--ease-standard);
  }
  .row.choice.selected .check {
    opacity: 1;
  }
  .sr {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .mode-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
  }
  .mode-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 16px;
    border-radius: var(--r-lg);
    text-align: left;
    transition:
      background var(--dur-fast) var(--ease-standard),
      box-shadow var(--dur-fast) var(--ease-standard),
      transform var(--dur-med) var(--ease-spring);
  }
  .mode-card:hover {
    background: var(--soft-hover);
  }
  .mode-card:active {
    transform: scale(0.985);
  }
  @media (prefers-reduced-motion: reduce) {
    .mode-card:active {
      transform: none;
    }
  }
  .mode-card.selected {
    background: var(--soft-active);
    box-shadow: inset 0 0 0 1.5px var(--ink);
  }
  .mode-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex-wrap: wrap;
  }
  .mode-title {
    font-size: 15px;
    font-weight: 700;
    color: var(--ink);
  }
  .mode-desc {
    margin: 0;
  }

  /* Step 4: Summary grid */
  .summary-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px 18px;
    margin: 0;
    padding: 8px 12px 12px;
  }
  .summary-item {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .summary-item dt {
    font-size: var(--fs-label);
    font-weight: 650;
    color: var(--ink-3);
  }
  .summary-item dd {
    margin: 0;
    font-size: var(--fs-body);
    font-weight: 600;
    color: var(--ink);
    overflow-wrap: anywhere;
  }

  /* ---------- footer ---------- */
  .wizard-foot {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding-top: 16px;
    border-top: 1px solid var(--hairline);
  }
  .step-counter {
    font-family: var(--font-figures);
    font-size: var(--fs-small);
    color: var(--ink-3);
  }

  /* ---------- responsive down to 640x540 ---------- */
  @media (max-width: 720px) {
    .wizard-frame {
      padding: 12px 16px 14px;
    }
    .step-label {
      display: none;
    }
    .step-pill {
      padding: 0 5px;
    }
    .step-line {
      width: 12px;
    }
    .row {
      flex-wrap: wrap;
      gap: 10px;
    }
    .mode-grid,
    .summary-grid {
      grid-template-columns: 1fr;
    }
  }

  @media (max-height: 620px) {
    .wizard-frame {
      padding-top: 10px;
      padding-bottom: 12px;
    }
    .wizard-top {
      padding-bottom: 10px;
    }
    .wizard-body {
      padding: 14px 2px 12px;
    }
    .wizard-foot {
      padding-top: 10px;
    }
    .hero-welcome {
      gap: 6px;
      padding: 0;
    }
    .emblem svg {
      width: 40px;
      height: 40px;
    }
    .emblem {
      width: 40px;
      height: 40px;
    }
  }
</style>
