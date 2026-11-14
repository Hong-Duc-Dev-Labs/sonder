<script lang="ts">
  // The mini player window's whole UI: the playing cover as a polaroid on a card washed with its
  // own blurred art, a record peeking out behind it, and the transport over the card on hover.
  // It shares the `app` store with the main window's code (so the record spins in step), but
  // only feeds it the player and the queue.
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { app } from "../lib/app.svelte";
  import { artUrl } from "../lib/art";
  import { commands, events } from "../lib/bindings";
  import { playbackMs } from "../lib/clock.svelte";
  import { applyLanguage, t } from "../lib/i18n.svelte";
  import { onFrame } from "../lib/motion";
  import Icon from "./Icon.svelte";
  import MiniDisc from "./MiniDisc.svelte";
  import PlayPauseIcon from "./PlayPauseIcon.svelte";

  let item = $derived(app.current);
  let playing = $derived(app.player.state === "playing");
  let total = $derived(app.player.durationMs ?? item?.durationMs ?? 0);
  let art = $derived(artUrl(item?.art, 300));
  let artFailed = $state(false);
  $effect(() => {
    void art;
    artFailed = false;
  });

  /** Fetches everything the window shows; also run whenever it is shown again. */
  async function refresh() {
    const [player, queue, settings] = await Promise.all([
      commands.playerSnapshot(),
      commands.queueItems(),
      commands.settingsGet(),
    ]);
    app.player = player;
    app.queue = queue;
    applyLanguage(settings.language);
  }

  let progress: HTMLDivElement | undefined = $state();

  // The progress line is written straight to the element, every frame only while playing.
  $effect(() => {
    void app.player;
    const el = progress;
    const length = total;
    if (!el) return;
    const draw = () => {
      const pct = length > 0 ? Math.min(1, playbackMs() / length) : 0;
      el.style.transform = `scaleX(${pct.toFixed(4)})`;
    };
    draw();
    return playing ? onFrame(draw) : undefined;
  });

  onMount(() => {
    const unlisten = Promise.all([
      events.playerStateEvent.listen((e) => (app.player = e.payload)),
      events.queueChangedEvent.listen((e) => (app.queue = e.payload)),
    ]);
    void refresh();
    // The window is hidden and shown again rather than rebuilt; the language may have changed.
    const onVisible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", onVisible);
    // Showing the window focuses it, which also covers webviews that stay "visible" while hidden.
    window.addEventListener("focus", onVisible);
    return () => {
      window.removeEventListener("focus", onVisible);
      document.removeEventListener("visibilitychange", onVisible);
      void unlisten.then((fns) => fns.forEach((f) => f()));
    };
  });

  const run = (work: Promise<unknown>) => work.catch(() => {});
  const toggle = () => run(commands.playerToggle());
  const openMain = () => run(commands.windowShowMain());

  /** Drags the frameless window from anywhere that is not a button. */
  function drag(e: PointerEvent) {
    if (e.button !== 0 || (e.target as Element).closest("button")) return;
    if (e.detail === 2) {
      void openMain();
      return;
    }
    void getCurrentWindow().startDragging();
  }

  function onKey(e: KeyboardEvent) {
    if (e.target instanceof HTMLButtonElement && (e.key === " " || e.key === "Enter")) return;
    if (e.key === " ") {
      e.preventDefault();
      void toggle();
    } else if (e.key === "ArrowRight") void run(commands.playerNext());
    else if (e.key === "ArrowLeft") void run(commands.playerPrevious());
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<main class="stage" class:playing onpointerdown={drag} aria-label={t("transport.player")}>
  <div class="record" aria-hidden="true">
    <MiniDisc art={item?.art} paused={!playing} />
  </div>

  <div class="card">
    <!-- The canvas lit by the cover: its colours, blurred and pushed toward black. -->
    <div class="wash" aria-hidden="true">
      {#if art && !artFailed}
        <img src={art} alt="" draggable="false" />
      {/if}
    </div>

    <figure class="polaroid">
      <div class="photo">
        {#if art && !artFailed}
          <img src={art} alt="" draggable="false" onerror={() => (artFailed = true)} />
        {:else}
          <div class="blank"><Icon name="note" size={40} /></div>
        {/if}
        <div class="controls">
          <button class="ctl" aria-label={t("common.previous")} onclick={() => run(commands.playerPrevious())}>
            <Icon name="prev" size={18} />
          </button>
          <button class="ctl main" aria-label={playing ? t("common.pause") : t("common.play")} onclick={toggle}>
            <PlayPauseIcon {playing} size={20} />
          </button>
          <button class="ctl" aria-label={t("common.next")} onclick={() => run(commands.playerNext())}>
            <Icon name="next" size={18} />
          </button>
        </div>
        <div class="track" aria-hidden="true"><div class="fill" bind:this={progress}></div></div>
      </div>
      <figcaption>
        <span class="title" title={item?.title}>{item?.title ?? t("common.nothingPlaying")}</span>
        <span class="meta">
          {#if item?.artist}
            <span class="artist" title={item.artist}>{item.artist}</span>
          {/if}
          <span class="bars" aria-hidden="true"><i></i><i></i><i></i></span>
        </span>
      </figcaption>
    </figure>

    <div class="corner">
      <button class="round" aria-label={t("mini.open")} title={t("mini.open")} onclick={openMain}>
        <Icon name="expand" size={13} />
      </button>
      <button class="round" aria-label={t("mini.quit")} title={t("mini.quit")} onclick={() => run(commands.appQuit())}>
        <Icon name="close" size={13} />
      </button>
    </div>
  </div>
</main>

<style>
  :global(html[data-window="mini"]),
  :global(html[data-window="mini"] body) {
    background: transparent;
  }

  /* The window is transparent and frameless: every shadow below stays inside the 10 px
     margin around the card, or the window edge would cut it off. */
  .stage {
    position: relative;
    width: 100%;
    height: 100%;
    cursor: grab;
  }
  .stage:active {
    cursor: grabbing;
  }

  /* The record sits behind the card and slides further out while it plays. */
  .record {
    position: absolute;
    top: 40px;
    left: 92px;
    width: 220px;
    filter: drop-shadow(0 4px 8px rgb(0 0 0 / 0.5));
    transition: transform var(--dur-slow) var(--ease-spring);
  }
  .playing .record {
    transform: translateX(20px);
  }

  .card {
    position: absolute;
    top: 10px;
    left: 10px;
    width: 260px;
    height: 280px;
    display: grid;
    place-items: center;
    isolation: isolate;
    overflow: hidden;
    border-radius: var(--r-lg);
    background: #0c0c0f;
    box-shadow: 0 4px 10px rgb(0 0 0 / 0.45);
  }
  /* Grain over everything (the paper too), then the hairline and top light the app's glass
     has. */
  .card::before,
  .card::after {
    content: "";
    position: absolute;
    inset: 0;
    z-index: 3;
    border-radius: inherit;
    pointer-events: none;
  }
  .card::before {
    background-image: var(--grain);
    background-size: 120px 120px;
    mix-blend-mode: overlay;
    opacity: 0.08;
  }
  .card::after {
    background: linear-gradient(180deg, rgb(255 255 255 / 0.07), transparent 35%);
    box-shadow: inset 0 0 0 1px var(--glass-line);
  }

  /* Overscanned so the blur never thins out into a dark rim at the card's edges. */
  .wash {
    position: absolute;
    inset: -40px;
    z-index: -1;
  }
  .wash img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    filter: blur(28px) saturate(1.5) brightness(0.72);
  }
  .wash::after {
    content: "";
    position: absolute;
    inset: 0;
    background:
      radial-gradient(120% 90% at 50% 20%, transparent, rgb(0 0 0 / 0.55)),
      rgb(0 0 0 / 0.18);
  }

  .polaroid {
    width: 182px;
    margin: 0;
    padding: 9px 9px 0;
    background: linear-gradient(180deg, #fbfaf6, #f1eee6);
    border-radius: 3px;
    color: #24221f;
    box-shadow:
      0 12px 22px rgb(0 0 0 / 0.42),
      0 2px 4px rgb(0 0 0 / 0.3),
      inset 0 0 0 1px rgb(255 255 255 / 0.6);
    rotate: -2deg;
    transition: rotate var(--dur-slow) var(--ease-spring);
  }
  .card:hover .polaroid,
  .card:focus-within .polaroid {
    rotate: 0deg;
  }
  .photo {
    position: relative;
    aspect-ratio: 1;
    overflow: hidden;
    background: #e6e2d9;
    box-shadow: inset 0 0 0 1px rgb(0 0 0 / 0.08);
  }
  .photo > img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  /* A scrim under the controls, shown with them. */
  .photo::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, transparent 45%, rgb(0 0 0 / 0.5));
    opacity: 0;
    transition: opacity var(--dur-med) var(--ease-standard);
    pointer-events: none;
  }
  .blank {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    color: #b5afa3;
  }
  .track {
    position: absolute;
    z-index: 2;
    left: 0;
    right: 0;
    bottom: 0;
    height: 3px;
    background: rgb(0 0 0 / 0.22);
  }
  .fill {
    height: 100%;
    background: rgb(255 255 255 / 0.92);
    transform-origin: left;
    transform: scaleX(0);
  }

  figcaption {
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 10px 3px 13px;
  }
  .title {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    font-size: 13px;
    font-weight: 600;
    line-height: 1.25;
    letter-spacing: -0.005em;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 12px;
  }
  .artist {
    flex: 1;
    min-width: 0;
    font-family: var(--font);
    font-size: 10px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: #6a645a;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Three small level bars, moving only while the track plays. */
  .bars {
    margin-left: auto;
    display: flex;
    align-items: flex-end;
    gap: 2px;
    height: 10px;
    opacity: 0.35;
    transition: opacity var(--dur-med) var(--ease-standard);
  }
  .bars i {
    width: 2px;
    height: 100%;
    border-radius: 1px;
    background: currentColor;
    transform-origin: bottom;
    transform: scaleY(0.3);
  }
  .playing .bars {
    opacity: 0.75;
  }
  .playing .bars i {
    animation: level 1.1s var(--ease-standard) infinite alternate;
  }
  .playing .bars i:nth-child(2) {
    animation-duration: 0.8s;
    animation-delay: -0.4s;
  }
  .playing .bars i:nth-child(3) {
    animation-duration: 1.3s;
    animation-delay: -0.7s;
  }
  @keyframes level {
    from {
      transform: scaleY(0.25);
    }
    to {
      transform: scaleY(1);
    }
  }

  /* Controls fade in while the pointer or keyboard focus is on the card. */
  .corner,
  .controls {
    position: absolute;
    z-index: 2;
    opacity: 0;
    transition:
      opacity var(--dur-med) var(--ease-standard),
      translate var(--dur-med) var(--ease-out);
  }
  .card:hover .corner,
  .card:hover .controls,
  .card:hover .photo::after,
  .card:focus-within .corner,
  .card:focus-within .controls,
  .card:focus-within .photo::after {
    opacity: 1;
  }

  /* Stacked in the card's right margin, clear of the photo. */
  .corner {
    top: 10px;
    right: 9px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .round {
    width: 24px;
    height: 24px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--ink-2);
    background: rgb(16 16 19 / 0.55);
    box-shadow: inset 0 0 0 1px var(--glass-line);
    -webkit-backdrop-filter: blur(12px);
    backdrop-filter: blur(12px);
    transition:
      color var(--dur-fast) var(--ease-standard),
      background var(--dur-fast) var(--ease-standard);
  }
  .round:hover {
    color: var(--ink);
    background: rgb(16 16 19 / 0.85);
  }

  .controls {
    left: 50%;
    bottom: 12px;
    translate: -50% 6px;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px;
    border-radius: var(--r-pill);
    background: rgb(16 16 19 / 0.6);
    -webkit-backdrop-filter: blur(16px) saturate(1.6);
    backdrop-filter: blur(16px) saturate(1.6);
    box-shadow:
      inset 0 0 0 1px var(--glass-line),
      0 6px 16px rgb(0 0 0 / 0.35);
  }
  .card:hover .controls,
  .card:focus-within .controls {
    translate: -50% 0;
  }
  .ctl {
    width: 32px;
    height: 32px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--ink);
    transition:
      background var(--dur-fast) var(--ease-standard),
      transform var(--dur-fast) var(--ease-standard);
  }
  .ctl:hover {
    background: var(--soft-hover);
  }
  .ctl:active {
    transform: scale(0.92);
  }
  .ctl.main {
    width: 38px;
    height: 38px;
    background: var(--ink);
    color: #111;
  }
  .ctl.main:hover {
    background: #fff;
  }

  @media (prefers-reduced-motion: reduce) {
    .record,
    .polaroid,
    .corner,
    .controls,
    .photo::after {
      transition: none;
    }
    .playing .bars i {
      animation: none;
      transform: scaleY(0.7);
    }
  }
</style>
