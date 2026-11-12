<script lang="ts">
  import { onMount } from "svelte";
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import { app, type View } from "./lib/app.svelte";
  import { commands } from "./lib/bindings";
  import { reducedMotion } from "./lib/motion";
  import { fadeQuick, pageIn, rise } from "./lib/transitions";
  import AlbumsView from "./components/AlbumsView.svelte";
  import AlbumView from "./components/AlbumView.svelte";
  import AmbientBackdrop from "./components/AmbientBackdrop.svelte";
  import DiscFlight from "./components/DiscFlight.svelte";
  import ArtistsView from "./components/ArtistsView.svelte";
  import ArtistView from "./components/ArtistView.svelte";
  import Icon, { type IconName } from "./components/Icon.svelte";
  import MiniDisc from "./components/MiniDisc.svelte";
  import NowPlayingView from "./components/NowPlayingView.svelte";
  import QueuePanel from "./components/QueuePanel.svelte";
  import SearchView from "./components/SearchView.svelte";
  import SettingsView from "./components/SettingsView.svelte";
  import TransportBar from "./components/TransportBar.svelte";

  type NavItem = { view: View; label: string; icon: IconName; hint?: string };
  const library: NavItem[] = [
    { view: { kind: "albums" }, label: "Albums", icon: "albums" },
    { view: { kind: "artists" }, label: "Artists", icon: "artists" },
  ];
  const tools: NavItem[] = [{ view: { kind: "search" }, label: "Search", icon: "search", hint: "Ctrl F" }];

  let ready = $state(false);
  onMount(() => {
    app.init().then(() => (ready = true));
  });

  let onNow = $derived(app.view.kind === "now");
  let errors = $derived(app.notices.filter((n) => n.level === "error"));
  let others = $derived(app.notices.filter((n) => n.level !== "error"));
  const flipMs = reducedMotion() ? 0 : 200;
  let detail = $derived(app.view.kind === "album" || app.view.kind === "artist");

  /** Controls that handle Space themselves: a global shortcut must not steal it from them. */
  const OWNS_SPACE = 'input, select, textarea, button, a, [role="button"], [role="slider"], [role="menuitem"], [role="radio"], [role="switch"], [contenteditable]';

  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    if (e.code === "Space" && !e.repeat && !target.closest(OWNS_SPACE)) {
      e.preventDefault();
      app.run(commands.playerToggle());
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
      e.preventDefault();
      app.go({ kind: "search" });
    } else if (e.altKey && e.key === "ArrowLeft") {
      app.back();
    }
  }

  function isActive(v: View): boolean {
    const k = app.view.kind;
    return v.kind === k || (v.kind === "albums" && k === "album") || (v.kind === "artists" && k === "artist");
  }

  function slidePanel(_node: Element) {
    if (reducedMotion()) return { duration: 1 };
    return {
      duration: 300,
      easing: cubicOut,
      css: (t: number, u: number) => `transform: translateX(${u * 105}%); opacity: ${t}`,
    };
  }
</script>

<svelte:window onkeydown={onKey} />

{#snippet navButton(item: NavItem)}
  <button
    class="nav"
    class:active={isActive(item.view)}
    aria-current={isActive(item.view) ? "page" : undefined}
    title={item.label}
    onclick={() => app.go(item.view)}
  >
    <Icon name={item.icon} size={20} />
    <span class="label">{item.label}</span>
    {#if item.hint}<kbd>{item.hint}</kbd>{/if}
  </button>
{/snippet}

<div
  class="shell"
  data-glass={app.settings.glass ?? "full"}
  style:--wash={app.wash ?? "transparent"}
>
  <AmbientBackdrop colors={app.current?.artColors ?? []} strength={onNow ? 1 : 0.45} />

  <main class:full={onNow}>
    {#if detail && app.history.length}
      <button class="back glass" aria-label="Back (Alt+Left)" onclick={() => app.back()}>
        <Icon name="back" size={20} />
      </button>
    {/if}
    <div class="content">
      {#if ready}
        <!-- Each view fades in on its own; the old one leaves at once, so nothing outside the
             content (sidebar, player) is touched by a view change. -->
        {#key app.view}
          <div class="view" in:pageIn>
            {#if app.view.kind === "albums"}
              <AlbumsView />
            {:else if app.view.kind === "album"}
              <AlbumView id={app.view.id} />
            {:else if app.view.kind === "artists"}
              <ArtistsView />
            {:else if app.view.kind === "artist"}
              <ArtistView id={app.view.id} />
            {:else if app.view.kind === "search"}
              <SearchView />
            {:else if app.view.kind === "now"}
              <NowPlayingView />
            {:else if app.view.kind === "settings"}
              <SettingsView />
            {/if}
          </div>
        {/key}
      {/if}
    </div>
  </main>

  <nav class="sidebar glass" aria-label="Main">
    <div class="brand">
      <svg class="mark" viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="12" cy="12" r="10.5" fill="#0b0b0d" stroke="currentColor" stroke-width="1.5" />
        <circle cx="12" cy="12" r="7" fill="none" stroke="currentColor" stroke-opacity=".35" stroke-width=".8" />
        <circle cx="12" cy="12" r="3.6" fill="currentColor" />
        <circle cx="12" cy="12" r="1" fill="#0b0b0d" />
      </svg>
      <span class="label">Sonder</span>
    </div>

    <span class="group kicker">Library</span>
    {#each library as item (item.label)}{@render navButton(item)}{/each}
    <span class="group-gap"></span>
    {#each tools as item (item.label)}{@render navButton(item)}{/each}
    <button
      class="nav"
      class:active={onNow}
      aria-current={onNow ? "page" : undefined}
      title="Now Playing"
      onclick={() => app.go({ kind: "now" })}
    >
      {#if app.current}
        <span class="live"><MiniDisc art={app.current.art} paused={app.player.state !== "playing"} /></span>
      {:else}
        <Icon name="now" size={20} />
      {/if}
      <span class="label">Now Playing</span>
    </button>

    <span class="spacer"></span>

    {#if app.scan.phase !== "idle"}
      <button class="scan" title="Scanning the library" aria-live="polite" onclick={() => app.go({ kind: "settings" })}>
        <span class="spinner" aria-hidden="true"></span>
        <span class="label">Scanning{app.scan.total ? ` ${app.scan.done}/${app.scan.total}` : "…"}</span>
      </button>
    {/if}
    {@render navButton({ view: { kind: "settings" }, label: "Settings", icon: "settings" })}
  </nav>

  {#if app.queueOpen}
    <div class="queue-slot" transition:slidePanel>
      <QueuePanel />
    </div>
  {/if}

  <TransportBar hidden={onNow} />

  {#if app.flight}
    {#key app.flight}
      <DiscFlight flight={app.flight} />
    {/key}
  {/if}

  <!-- Errors are announced assertively, everything else politely. Hovering or focusing a
       notice holds them all on screen. -->
  <div
    class="toasts"
    class:raised={!onNow}
    role="presentation"
    onpointerenter={() => app.pauseNotices()}
    onpointerleave={() => app.resumeNotices()}
    onfocusin={() => app.pauseNotices()}
    onfocusout={() => app.resumeNotices()}
  >
    <div class="region" role="alert">
      {#each errors as n (n.id)}
        <div class="toast glass error" in:rise out:fadeQuick animate:flip={{ duration: flipMs }}>
          <span class="dot" aria-hidden="true"></span>
          <span class="msg">{n.message}</span>
          <button class="dismiss" aria-label="Dismiss" onclick={() => app.dismiss(n.id)}><Icon name="close" size={16} /></button>
        </div>
      {/each}
    </div>
    <div class="region" role="status" aria-live="polite">
      {#each others as n (n.id)}
        <div class="toast glass {n.level}" in:rise out:fadeQuick animate:flip={{ duration: flipMs }}>
          <span class="dot" aria-hidden="true"></span>
          <span class="msg">{n.message}</span>
          <button class="dismiss" aria-label="Dismiss" onclick={() => app.dismiss(n.id)}><Icon name="close" size={16} /></button>
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .shell {
    position: relative;
    height: 100%;
    overflow: hidden;
    transition: --wash 900ms var(--ease-standard);
    /* What the floating sidebar covers. Declared here (not on :root) so the narrow-window
       rail below, which changes --sidebar on this element, changes it too. */
    --inset-left: calc(var(--sidebar) + var(--gutter) * 2);
  }

  /* Content spans the whole window and scrolls under the glass sidebar; each page keeps its
     own text clear of it with --inset-left, while backdrops run to the window's edge. */
  main {
    position: absolute;
    inset: 0;
    z-index: 1;
  }
  .content {
    view-transition-name: content;
    height: 100%;
  }
  .view {
    height: 100%;
  }
  .back {
    position: absolute;
    top: 20px;
    left: calc(var(--inset-left) + 20px);
    z-index: 5;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    color: var(--ink);
    transition: transform var(--dur-med) var(--ease-spring);
  }
  .back:active {
    transform: scale(0.9);
  }

  .sidebar {
    /* its own layer during a cover morph: above the moving page, not cross-faded */
    view-transition-name: sidebar;
    position: absolute;
    z-index: 10;
    top: var(--gutter);
    bottom: var(--gutter);
    left: var(--gutter);
    width: var(--sidebar);
    border-radius: var(--r-lg);
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 18px 12px 12px;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 10px 22px;
    font-size: 20px;
    font-weight: 800;
    letter-spacing: -0.03em;
  }
  .mark {
    width: 28px;
    height: 28px;
    flex: none;
    color: var(--ink);
  }
  .group {
    padding: 0 14px 6px;
  }
  .group-gap {
    height: 14px;
  }
  .nav {
    position: relative;
    display: flex;
    align-items: center;
    gap: 14px;
    height: 44px;
    padding: 0 14px;
    border-radius: var(--r-pill);
    color: var(--ink-2);
    font-weight: 600;
    text-align: left;
    transition:
      background var(--dur-fast) var(--ease-standard),
      color var(--dur-fast) var(--ease-standard),
      transform var(--dur-med) var(--ease-spring);
  }
  .nav:hover {
    color: var(--ink);
    background: var(--soft-hover);
  }
  .nav:active {
    transform: scale(0.97);
  }
  .nav.active {
    color: var(--ink);
    background: var(--soft-active);
    box-shadow: inset 0 0 0 1px var(--hairline);
  }
  .nav :global(svg) {
    flex: none;
  }
  .label {
    flex: 1;
    white-space: nowrap;
  }
  kbd {
    font-family: var(--font);
    font-size: 10.5px;
    font-weight: 650;
    color: var(--ink-2);
    padding: 2px 6px;
    border-radius: 6px;
    box-shadow: inset 0 0 0 1px var(--hairline);
  }
  .live {
    width: 22px;
    flex: none;
    margin: 0 -1px;
  }
  .spacer {
    flex: 1;
  }
  .scan {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 36px;
    padding: 0 14px;
    border-radius: var(--r-pill);
    font-size: var(--fs-small);
    color: var(--ink-2);
    margin-bottom: 4px;
  }
  .spinner {
    width: 14px;
    height: 14px;
    flex: none;
    border-radius: 50%;
    border: 2px solid rgb(255 255 255 / 0.2);
    border-top-color: var(--ink);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .queue-slot {
    position: absolute;
    z-index: 15;
    top: var(--gutter);
    right: var(--gutter);
    bottom: calc(var(--dock-h) + var(--gutter) * 2);
    display: flex;
  }

  .toasts {
    position: absolute;
    z-index: 30;
    right: calc(var(--gutter) * 2);
    bottom: calc(var(--gutter) * 2);
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 440px;
    transition: bottom var(--dur-slow) var(--ease-out);
  }
  .toasts.raised {
    bottom: calc(var(--dock-h) + var(--gutter) * 3);
  }
  .region {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .toast {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 48px;
    padding: 8px 8px 8px 16px;
    border-radius: 24px;
    font-size: 13px;
    line-height: 1.4;
    user-select: text;
  }
  .toast .dot {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
    background: var(--ink-2);
  }
  .toast.warning .dot {
    background: var(--warn);
  }
  .toast.error .dot {
    background: var(--error);
  }
  .msg {
    flex: 1;
  }
  .dismiss {
    width: 32px;
    height: 32px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--ink-2);
  }
  .dismiss:hover {
    background: var(--soft-hover);
    color: var(--ink);
  }

  /* Narrow windows: the sidebar becomes an icon rail. */
  @media (max-width: 1099px) {
    .shell {
      --sidebar: 72px;
    }
    .sidebar {
      padding: 16px 10px 10px;
      align-items: center;
    }
    .brand {
      padding: 4px 0 18px;
    }
    .label,
    kbd,
    .group {
      display: none;
    }
    .nav,
    .scan {
      width: 52px;
      padding: 0;
      justify-content: center;
    }
  }
</style>
