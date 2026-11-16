<script lang="ts">
  // Now Playing View - Optimized Layout:
  // 1. Default / Lyrics Mode: 2-column layout. Left: Vinyl Turntable (or square cover) with song title,
  //    artist, seekbar, and transport controls underneath. Right: Synced Lyrics panel (auto-scroll & karaoke words).
  // 2. Stage Mode: Full-window Folia Flow kinetic typography visualizer with sleek bottom control dock.
  // 3. Queue Mode: 2-column layout with Vinyl Turntable on left and Up Next queue on right.
  // 4. Header: Auto-revealing tabs (Lời bài hát / Sân khấu / Hàng đợi) and centered down chevron.
  import { onMount } from "svelte";
  import { app } from "../lib/app.svelte";
  import { commands, type RepeatMode } from "../lib/bindings";
  import { isBitPerfect, outputLabel } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { reducedMotion } from "../lib/motion";
  import Cover from "./Cover.svelte";
  import FormatBadges from "./FormatBadges.svelte";
  import Icon from "./Icon.svelte";
  import LyricStage from "./LyricStage.svelte";
  import LyricsPanel from "./LyricsPanel.svelte";
  import PlayPauseIcon from "./PlayPauseIcon.svelte";
  import QueuePanel from "./QueuePanel.svelte";
  import SeekBar from "./SeekBar.svelte";
  import SignalPathPanel from "./SignalPathPanel.svelte";
  import TurntableStage from "./TurntableStage.svelte";

  type NpTab = "lyrics" | "stage" | "queue";
  const TABS: NpTab[] = ["lyrics", "stage", "queue"];

  let item = $derived(app.current);
  let path = $derived(app.player.signalPath);
  let output = $derived(app.player.output);
  let playing = $derived(app.player.state === "playing");
  let bitPerfect = $derived(isBitPerfect(app.player));
  let coverStyle = $derived(app.settings.coverStyle ?? "vinyl");

  /** Codec chip (e.g. "24/96", "16/44.1", or DSD) */
  let codec = $derived.by(() => {
    const src = app.player.source;
    if (!src || !item) return null;
    const khz = src.rate / 1000;
    return `${src.bits}/${Number.isInteger(khz) ? khz : khz.toFixed(1)}`;
  });

  // Active tab for the view: defaults to Lyrics or Stage based on user setting
  let activeTab = $state<NpTab>(app.settings.coverStyle === "stage" ? "stage" : "lyrics");

  $effect(() => {
    if (app.settings.coverStyle === "stage" && activeTab !== "stage") {
      activeTab = "stage";
    }
  });

  function selectTab(tab: NpTab) {
    activeTab = tab;
  }

  function onTablistKeyDown(e: KeyboardEvent) {
    const idx = TABS.indexOf(activeTab);
    if (e.key === "ArrowRight" || e.key === "ArrowDown") {
      e.preventDefault();
      const next = TABS[(idx + 1) % TABS.length];
      selectTab(next);
      document.getElementById(`np-tab-${next}`)?.focus();
    } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
      e.preventDefault();
      const prev = TABS[(idx - 1 + TABS.length) % TABS.length];
      selectTab(prev);
      document.getElementById(`np-tab-${prev}`)?.focus();
    } else if (e.key === "Home") {
      e.preventDefault();
      selectTab(TABS[0]);
      document.getElementById(`np-tab-${TABS[0]}`)?.focus();
    } else if (e.key === "End") {
      e.preventDefault();
      selectTab(TABS[TABS.length - 1]);
      document.getElementById(`np-tab-${TABS[TABS.length - 1]}`)?.focus();
    }
  }

  function toggleCoverStyle() {
    const next = coverStyle === "vinyl" ? "square" : "vinyl";
    app.saveSettings({ ...app.settings, coverStyle: next });
  }

  // Auto-revealing header controls on mouse move / hover
  let headerHovered = $state(false);
  let dockHovered = $state(false);
  let pointerActive = $state(true);
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  function onPointerMove() {
    pointerActive = true;
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      pointerActive = false;
    }, 2800);
  }

  // Volume slider integration
  const VOL_MIN = -60;
  let volumeDb = $derived(app.player.volumeDb);
  let volumeDraft = $state(0);
  $effect(() => {
    if (volumeDb != null) volumeDraft = volumeDb;
  });
  function commitVolume() {
    app.run(commands.playerSetVolume(volumeDraft));
  }

  // Repeat toggle
  let repeat = $derived<RepeatMode>(app.player.repeat ?? "off");
  const repeatOrder: Record<RepeatMode, RepeatMode> = { off: "all", all: "one", one: "off" };
  function cycleRepeat() {
    app.run(commands.playerSetRepeat(repeatOrder[repeat]));
  }
  let repeatLabel = $derived(
    repeat === "one"
      ? t("transport.repeatOne")
      : repeat === "all"
        ? t("transport.repeatAll")
        : t("transport.repeatOff"),
  );

  // Pull-down to dismiss gesture
  let pullY = $state(0);
  let isDragging = $state(false);
  let startY = 0;
  let lastY = 0;
  let lastTime = 0;
  let velocityY = 0;

  function onDragStart(e: MouseEvent | TouchEvent) {
    if ("button" in e && e.button !== 0) return;
    const target = e.target as HTMLElement;
    if (
      target.closest("button") ||
      target.closest("input") ||
      target.closest(".scroller") ||
      target.closest(".pane-scroll") ||
      target.closest(".stage-dock")
    ) {
      return;
    }
    const clientY = "touches" in e ? e.touches[0].clientY : e.clientY;
    if (clientY > 160) return;

    isDragging = true;
    startY = clientY;
    lastY = clientY;
    lastTime = performance.now();
    velocityY = 0;
  }

  function onDragMove(e: MouseEvent | TouchEvent) {
    if (!isDragging) return;
    const clientY = "touches" in e ? e.touches[0].clientY : e.clientY;
    const now = performance.now();
    const dt = Math.max(1, now - lastTime);
    velocityY = ((clientY - lastY) / dt) * 1000;
    lastY = clientY;
    lastTime = now;

    const dy = Math.max(0, clientY - startY);
    pullY = Math.pow(dy, 0.88);
  }

  function onDragEnd() {
    if (!isDragging) return;
    isDragging = false;
    if (pullY > 120 || (pullY > 30 && velocityY > 700)) {
      dismiss();
    } else {
      pullY = 0;
    }
  }

  function dismiss() {
    if (app.history.length > 0) {
      app.back();
    } else {
      app.go({ kind: "home" });
    }
  }

  function onWindowKey(e: KeyboardEvent) {
    if (e.key === "Escape" && !app.signalPathOpen) {
      e.preventDefault();
      dismiss();
    }
  }

  let rootEl: HTMLDivElement | undefined = $state();
  onMount(() => {
    onPointerMove();
    const growEnabled = app.settings.growFromCover ?? true;
    if (growEnabled && rootEl && !reducedMotion()) {
      const fromEl = document.querySelector<HTMLElement>('[data-flight="pill"]');
      if (fromEl) {
        const fromRect = fromEl.getBoundingClientRect();
        const winW = window.innerWidth;
        const winH = window.innerHeight;
        rootEl.animate(
          [
            {
              clipPath: `inset(${fromRect.top}px ${winW - fromRect.right}px ${winH - fromRect.bottom}px ${fromRect.left}px round 8px)`,
              opacity: 0.2,
            },
            {
              clipPath: `inset(0px 0px 0px 0px round 0px)`,
              opacity: 1,
            },
          ],
          {
            duration: 480,
            easing: "cubic-bezier(0.2, 0.9, 0.2, 1)",
            fill: "forwards",
          },
        );
      }
    }
    return () => clearTimeout(hideTimer);
  });
</script>

<svelte:window
  onmousemove={onPointerMove}
  onkeydown={onWindowKey}
  ontouchmove={(e) => {
    if (isDragging) onDragMove(e);
  }}
  onmouseup={onDragEnd}
  ontouchend={onDragEnd}
/>

<!-- Screen Reader Polite Live Region for Song/Track Changes -->
<div class="sr-only" aria-live="polite" aria-atomic="true">
  {#if item}
    {t("common.nowPlayingKicker")}: {item.title} {t("common.itemBy", { title: "", artist: item.artist ?? "" })}
  {:else}
    {t("common.nothingPlaying")}
  {/if}
</div>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={rootEl}
  class="np-container"
  class:dragging={isDragging}
  style:transform={pullY > 0 ? (reducedMotion() ? `translate3d(0, ${pullY}px, 0)` : `translate3d(0, ${pullY}px, 0) scale(${1 - Math.min(0.12, pullY / 1000)})`) : undefined}
  onmousedown={onDragStart}
  onmousemove={onDragMove}
  ontouchstart={onDragStart}
  ontouchmove={onDragMove}
>
  <!-- Floating auto-revealing header -->
  <header
    class="np-header"
    class:visible={headerHovered || pointerActive || pullY > 0}
    onmouseenter={() => (headerHovered = true)}
    onmouseleave={() => (headerHovered = false)}
  >
    <div class="header-left">
      <div
        class="tabs-group"
        role="tablist"
        tabindex="0"
        aria-label={t("common.nowPlaying")}
        onkeydown={onTablistKeyDown}
      >
        <button
          role="tab"
          id="np-tab-lyrics"
          class="tab-pill"
          class:active={activeTab === "lyrics"}
          aria-selected={activeTab === "lyrics"}
          aria-controls="np-panel-lyrics"
          tabindex={activeTab === "lyrics" ? 0 : -1}
          onclick={() => selectTab("lyrics")}
        >
          <Icon name="lyrics" size={16} />
          <span>{t("np.tabLyrics")}</span>
        </button>
        <button
          role="tab"
          id="np-tab-stage"
          class="tab-pill"
          class:active={activeTab === "stage"}
          aria-selected={activeTab === "stage"}
          aria-controls="np-panel-stage"
          tabindex={activeTab === "stage" ? 0 : -1}
          onclick={() => selectTab("stage")}
        >
          <Icon name="spectrum" size={16} />
          <span>{t("np.tabStage")}</span>
        </button>
        <button
          role="tab"
          id="np-tab-queue"
          class="tab-pill"
          class:active={activeTab === "queue"}
          aria-selected={activeTab === "queue"}
          aria-controls="np-panel-queue"
          tabindex={activeTab === "queue" ? 0 : -1}
          onclick={() => selectTab("queue")}
        >
          <Icon name="queue" size={16} />
          <span>{t("np.tabQueue")}</span>
        </button>
      </div>
    </div>

    <div class="header-center">
      <button
        class="icon-btn chevron-close"
        aria-label={t("np.close")}
        title={t("np.close")}
        onclick={dismiss}
      >
        <Icon name="down" size={20} />
      </button>
    </div>

    <div class="header-right">
      {#if output}
        <button
          class="path-chip"
          class:warn={!bitPerfect}
          title={outputLabel(output)}
          aria-label={bitPerfect ? t("np.outputPerfect") : t("np.outputVolume")}
          onclick={() => (app.signalPathOpen = !app.signalPathOpen)}
        >
          <span class="chip-dot" aria-hidden="true"></span>
          <span class="chip-text">{bitPerfect ? t("transport.statePerfect") : t("np.outputVolume")}</span>
        </button>
      {/if}
    </div>
  </header>

  <!-- View Content -->
  {#if activeTab === "stage"}
    <!-- Full-screen Stage Mode (Kinetic Folia Visualizer) -->
    <div
      class="stage-fullscreen"
      role="tabpanel"
      id="np-panel-stage"
      aria-labelledby="np-tab-stage"
    >
      <div class="stage-body">
        <LyricStage />
      </div>

      <div
        class="stage-dock"
        class:idle={!pointerActive && !dockHovered}
        onmouseenter={() => (dockHovered = true)}
        onmouseleave={() => (dockHovered = false)}
      >
        <div class="dock-meta">
          <div
            class="dock-art"
            data-flight="stage"
            class:landing={app.flight?.target === "stage" && !app.flight.landed}
          >
            <Cover hash={item?.art} size={300} alt={item?.album ?? ""} />
          </div>
          <div class="dock-text">
            <div class="dock-title-row">
              <span class="dock-title" title={item?.title}>{item?.title ?? t("common.nothingPlaying")}</span>
              {#if codec}<span class="codec-chip">{codec}</span>{/if}
            </div>
            {#if item}
              <button
                class="dock-sub link-btn"
                aria-label={`${t("common.goToAlbum")}: ${item.album}`}
                onclick={() => item && app.go({ kind: "album", id: item.albumId })}
              >
                {item.artist ?? t("common.unknownArtist")} · {item.album}
              </button>
            {/if}
          </div>
        </div>

        <div class="dock-center">
          <div class="dock-transport">
            <button
              class="ctrl-btn sm"
              aria-label={t("common.previous")}
              title={t("common.previous")}
              onclick={() => app.run(commands.playerPrevious())}
            >
              <Icon name="prev" size={24} />
            </button>
            <button
              class="play-btn-main sm"
              aria-label={playing ? t("common.pause") : t("common.play")}
              title={playing ? t("common.pause") : t("common.play")}
              onclick={() => app.run(commands.playerToggle())}
            >
              <PlayPauseIcon {playing} size={22} />
            </button>
            <button
              class="ctrl-btn sm"
              aria-label={t("common.next")}
              title={t("common.next")}
              onclick={() => app.run(commands.playerNext())}
            >
              <Icon name="next" size={24} />
            </button>
          </div>
          <div class="dock-seek">
            <SeekBar inline />
          </div>
        </div>

        <div class="dock-tools">
          <button
            class="tab-pill"
            onclick={() => selectTab("lyrics")}
            title={t("np.tabLyrics")}
            aria-label={t("np.tabLyrics")}
          >
            <Icon name="lyrics" size={16} />
            <span>{t("np.tabLyrics")}</span>
          </button>
          <button
            class="tab-pill"
            onclick={() => selectTab("queue")}
            title={t("np.tabQueue")}
            aria-label={t("np.tabQueue")}
          >
            <Icon name="queue" size={16} />
            <span>{t("np.tabQueue")}</span>
          </button>
        </div>
      </div>
    </div>
  {:else}
    <!-- 2-Column Responsive Body (Lyrics or Queue Mode) -->
    <div
      class="np-body"
      role="tabpanel"
      id={`np-panel-${activeTab}`}
      aria-labelledby={`np-tab-${activeTab}`}
    >
      <!-- Left Column: Vinyl Turntable (or square cover) + Metadata + Playback Controls -->
      <section class="left-col" aria-label={t("common.nowPlaying")}>
        <div class="visual-wrapper">
          {#if coverStyle === "vinyl"}
            <div class="turntable-box">
              <TurntableStage />
            </div>
          {:else}
            <div
              class="square-art-box"
              data-flight="stage"
              class:landing={app.flight?.target === "stage" && !app.flight.landed}
            >
              <Cover hash={item?.art} size={1000} alt={item?.album ?? ""} />
            </div>
          {/if}
        </div>

        <div class="meta-section">
          <div class="title-row">
            <h1 class="track-title" title={item?.title}>{item?.title ?? t("common.nothingPlaying")}</h1>
            {#if codec}
              <span class="codec-chip">{codec}</span>
            {/if}
          </div>
          <p class="track-artist">
            {#if item}
              <button
                class="link-btn artist"
                aria-label={`${t("common.goToAlbum")}: ${item.artist}`}
                onclick={() => item && app.go({ kind: "album", id: item.albumId })}
              >
                {item.artist ?? t("common.unknownArtist")}
              </button>
              <span class="dot-sep" aria-hidden="true">·</span>
              <button
                class="link-btn album"
                aria-label={`${t("common.goToAlbum")}: ${item.album}`}
                onclick={() => item && app.go({ kind: "album", id: item.albumId })}
              >
                {item.album}
              </button>
            {:else}
              <span class="dim">{t("np.pickAlbum")}</span>
            {/if}
          </p>
        </div>

        <div class="seek-section">
          <SeekBar />
          <div class="badges-row">
            <FormatBadges />
          </div>
        </div>

        <div class="transport-section">
          <div class="transport-row">
            <button
              class="ctrl-btn sm"
              class:active={repeat !== "off"}
              aria-label={repeatLabel}
              aria-pressed={repeat !== "off"}
              title={repeatLabel}
              onclick={cycleRepeat}
            >
              <Icon name={repeat === "one" ? "repeat-one" : "repeat"} size={22} />
            </button>

            <button
              class="ctrl-btn sm"
              aria-label={t("common.previous")}
              title={t("common.previous")}
              onclick={() => app.run(commands.playerPrevious())}
            >
              <Icon name="prev" size={28} />
            </button>

            <button
              class="play-btn-main"
              aria-label={playing ? t("common.pause") : t("common.play")}
              title={playing ? t("common.pause") : t("common.play")}
              onclick={() => app.run(commands.playerToggle())}
            >
              <PlayPauseIcon {playing} size={42} />
            </button>

            <button
              class="ctrl-btn sm"
              aria-label={t("common.next")}
              title={t("common.next")}
              onclick={() => app.run(commands.playerNext())}
            >
              <Icon name="next" size={28} />
            </button>

            <button
              class="ctrl-btn sm"
              aria-label={coverStyle === "vinyl" ? t("np.showSquare") : t("np.showVinyl")}
              title={coverStyle === "vinyl" ? t("np.showSquare") : t("np.showVinyl")}
              onclick={toggleCoverStyle}
            >
              <Icon name={coverStyle === "vinyl" ? "albums" : "disc"} size={22} />
            </button>
          </div>

          {#if volumeDb != null}
            <div class="vol-row">
              <Icon name="volume" size={16} />
              <input
                class="vol-slider"
                type="range"
                min={VOL_MIN}
                max="0"
                step="0.5"
                bind:value={volumeDraft}
                onchange={commitVolume}
                aria-label={t("common.softwareVolume")}
                aria-valuetext={`${volumeDraft.toFixed(1)} dB`}
              />
            </div>
          {/if}
        </div>
      </section>

      <!-- Right Column: Lyrics / Queue -->
      <section class="right-col" aria-label={activeTab === "lyrics" ? t("common.lyrics") : t("common.queue")}>
        <div class="pane-box">
          {#if activeTab === "lyrics"}
            <div class="pane-scroll">
              <LyricsPanel />
            </div>
          {:else if activeTab === "queue"}
            <div class="pane-scroll queue-pane">
              <QueuePanel />
            </div>
          {/if}
        </div>
      </section>
    </div>
  {/if}

  {#if app.signalPathOpen && path}
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t("common.signalPath")}
      class="signal-modal-backdrop"
      onclick={() => (app.signalPathOpen = false)}
      onkeydown={(e) => e.key === "Escape" && (app.signalPathOpen = false)}
      tabindex="-1"
    >
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="signal-modal-content" onclick={(e) => e.stopPropagation()}>
        <SignalPathPanel {path} />
      </div>
    </div>
  {/if}
</div>

<style>
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  .np-container {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: 0;
    box-sizing: border-box;
    overflow: hidden;
    user-select: none;
    transition: transform 0.28s cubic-bezier(0.2, 0.9, 0.2, 1);
  }

  .np-container.dragging {
    transition: none;
  }

  /* Header with :focus-within assurance */
  .np-header {
    height: 56px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px max(32px, calc((100vw - 1320px) / 2));
    z-index: 20;
    opacity: 0;
    transform: translateY(-6px);
    transition: opacity 0.35s ease, transform 0.35s ease;
  }

  .np-header.visible,
  .np-header:focus-within {
    opacity: 1;
    transform: translateY(0);
  }

  .header-left,
  .header-right {
    flex: 1;
    display: flex;
    align-items: center;
  }

  .header-right {
    justify-content: flex-end;
  }

  .header-center {
    display: flex;
    justify-content: center;
  }

  .chevron-close {
    width: 38px;
    height: 38px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.08);
    backdrop-filter: blur(12px);
    border: 1px solid rgba(255, 255, 255, 0.12);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    color: rgba(255, 255, 255, 0.85);
    transition: all 0.2s ease;
  }

  .chevron-close:hover {
    background: rgba(255, 255, 255, 0.18);
    transform: translateY(2px);
    color: #fff;
  }

  .chevron-close:focus-visible {
    outline: 2px solid #fff;
    outline-offset: 3px;
    box-shadow: 0 0 0 6px rgba(0, 0, 0, 0.65);
  }

  .tabs-group {
    display: flex;
    align-items: center;
    gap: 6px;
    background: rgba(0, 0, 0, 0.32);
    backdrop-filter: blur(16px);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 999px;
    padding: 3px;
  }

  .tab-pill {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 14px;
    border-radius: 999px;
    border: none;
    background: transparent;
    color: rgba(255, 255, 255, 0.72);
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.18s ease;
  }

  .tab-pill:hover {
    color: #fff;
    background: rgba(255, 255, 255, 0.1);
  }

  .tab-pill.active {
    color: #fff;
    background: rgba(255, 255, 255, 0.2);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
  }

  .tab-pill:focus-visible {
    outline: 2px solid #fff;
    outline-offset: 2px;
    box-shadow: 0 0 0 5px rgba(0, 0, 0, 0.6);
  }

  .path-chip {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 12px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: rgba(255, 255, 255, 0.85);
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
  }

  .path-chip:focus-visible {
    outline: 2px solid #fff;
    outline-offset: 2px;
    box-shadow: 0 0 0 5px rgba(0, 0, 0, 0.6);
  }

  .chip-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #34d399;
  }

  .path-chip.warn .chip-dot {
    background: #fbbf24;
  }

  /* 2-Column Responsive Body */
  .np-body {
    flex: 1;
    min-height: 0;
    max-width: 1320px;
    width: 100%;
    margin: 0 auto;
    padding: 0 32px 32px 32px;
    display: grid;
    grid-template-columns: minmax(320px, 440px) 1fr;
    gap: 52px;
    align-items: center;
    box-sizing: border-box;
  }

  @media (max-width: 720px) {
    .np-body {
      grid-template-columns: 1fr;
      gap: 28px;
      overflow-y: auto;
    }
  }

  @media (max-width: 620px) {
    .tab-pill span {
      display: none;
    }
    .tab-pill {
      padding: 6px 10px;
    }
  }

  /* Left Column */
  .left-col {
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: 100%;
    max-width: 440px;
    margin: 0 auto;
  }

  .visual-wrapper {
    width: 100%;
    display: flex;
    justify-content: center;
    align-items: center;
  }

  .turntable-box {
    width: 100%;
    max-width: 400px;
    aspect-ratio: 1;
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    filter: drop-shadow(0 20px 45px rgba(0, 0, 0, 0.65));
  }

  .square-art-box {
    width: 100%;
    aspect-ratio: 1;
    max-width: 380px;
    border-radius: 18px;
    overflow: hidden;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.75), 0 0 0 1px rgba(255, 255, 255, 0.1);
  }

  .square-art-box.landing,
  .dock-art.landing {
    opacity: 0;
  }

  .meta-section {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .track-title {
    font-size: 22px;
    font-weight: 700;
    letter-spacing: -0.4px;
    color: #fff;
    margin: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-shadow: 0 2px 8px rgba(0, 0, 0, 0.85);
  }

  .codec-chip {
    font-size: 11px;
    font-weight: 700;
    padding: 2px 6px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.16);
    color: #fff;
    letter-spacing: 0.3px;
    flex-shrink: 0;
  }

  .track-artist {
    font-size: 14.5px;
    color: var(--ink-2, #b8b8c0);
    margin: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.7);
  }

  .link-btn {
    background: transparent;
    border: none;
    padding: 2px 4px;
    border-radius: 4px;
    color: inherit;
    font: inherit;
    cursor: pointer;
    text-decoration: none;
  }

  .link-btn:hover {
    color: #fff;
    text-decoration: underline;
  }

  .link-btn:focus-visible {
    outline: 2px solid #fff;
    outline-offset: 2px;
    background: rgba(255, 255, 255, 0.12);
  }

  .dot-sep {
    opacity: 0.5;
  }

  .seek-section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .badges-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }

  /* Transport */
  .transport-section {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .transport-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 18px;
  }

  .ctrl-btn {
    background: transparent;
    border: none;
    color: rgba(255, 255, 255, 0.78);
    cursor: pointer;
    border-radius: 50%;
    width: 44px;
    height: 44px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.18s ease;
  }

  .ctrl-btn:hover {
    color: #fff;
    background: rgba(255, 255, 255, 0.12);
  }

  .ctrl-btn.active {
    color: var(--accent, #38bdf8);
  }

  .ctrl-btn:focus-visible {
    outline: 2px solid #fff;
    outline-offset: 3px;
    box-shadow: 0 0 0 6px rgba(0, 0, 0, 0.65);
  }

  .play-btn-main {
    width: 58px;
    height: 58px;
    border-radius: 50%;
    background: #fff;
    color: #0b0b0d;
    border: none;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
    transition: transform 0.18s ease, box-shadow 0.18s ease;
  }

  .play-btn-main:hover {
    transform: scale(1.05);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.55);
  }

  .play-btn-main:active {
    transform: scale(0.96);
  }

  .play-btn-main:focus-visible {
    outline: 3px solid #38bdf8;
    outline-offset: 4px;
    box-shadow: 0 0 0 8px rgba(0, 0, 0, 0.7);
  }

  .vol-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 16px;
    color: rgba(255, 255, 255, 0.8);
  }

  .vol-slider {
    flex: 1;
    height: 6px;
    accent-color: #fff;
    cursor: pointer;
  }

  .vol-slider:focus-visible {
    outline: 2px solid #fff;
    outline-offset: 3px;
  }

  /* Right Column: Pane */
  .right-col {
    width: 100%;
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    container-type: inline-size;
  }

  .pane-box {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .pane-scroll {
    flex: 1;
    min-height: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  /* Fullscreen Stage Mode */
  .stage-fullscreen {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    outline: none;
  }

  .stage-body {
    flex: 1;
    min-height: 0;
    position: relative;
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .stage-dock {
    flex: none;
    height: 84px;
    display: grid;
    grid-template-columns: minmax(220px, 1fr) minmax(360px, 2fr) minmax(180px, 1fr);
    align-items: center;
    gap: 28px;
    padding: 0 28px 12px;
    background: linear-gradient(to top, rgba(11, 11, 13, 0.88) 0%, rgba(11, 11, 13, 0.45) 80%, transparent 100%);
    backdrop-filter: blur(16px);
    border-top: 1px solid rgba(255, 255, 255, 0.08);
    z-index: 10;
    transition: opacity 0.4s ease;
  }

  .stage-dock.idle {
    opacity: 0.28;
  }

  .stage-dock.idle:hover,
  .stage-dock:focus-within {
    opacity: 1;
  }

  @media (max-width: 820px) {
    .stage-dock {
      height: auto;
      grid-template-columns: 1fr auto;
      grid-template-rows: auto auto;
      row-gap: 12px;
      padding: 12px 20px;
    }
    .dock-meta {
      grid-column: 1;
      grid-row: 1;
    }
    .dock-tools {
      grid-column: 2;
      grid-row: 1;
    }
    .dock-center {
      grid-column: 1 / -1;
      grid-row: 2;
      max-width: none;
    }
  }

  .dock-meta {
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
  }

  .dock-art {
    width: 52px;
    height: 52px;
    border-radius: 8px;
    overflow: hidden;
    flex-shrink: 0;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.4);
  }

  .dock-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .dock-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .dock-title {
    font-size: 15px;
    font-weight: 700;
    color: #fff;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.8);
  }

  .dock-sub {
    font-size: 13px;
    color: var(--ink-2, #b8b8c0);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dock-center {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    width: 100%;
    max-width: 540px;
    margin: 0 auto;
  }

  .dock-transport {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .play-btn-main.sm {
    width: 44px;
    height: 44px;
  }

  .dock-seek {
    width: 100%;
  }

  .dock-tools {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
  }

  /* Signal modal */
  .signal-modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(16px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .signal-modal-content {
    background: rgba(20, 20, 26, 0.96);
    border: 1px solid rgba(255, 255, 255, 0.16);
    border-radius: 18px;
    padding: 24px;
    max-width: 500px;
    width: 90%;
    box-shadow: 0 20px 50px rgba(0, 0, 0, 0.8);
  }
</style>