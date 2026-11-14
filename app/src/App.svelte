<script lang="ts">
  import { onMount } from "svelte";
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import { app, type View } from "./lib/app.svelte";
  import { commands } from "./lib/bindings";
  import { t } from "./lib/i18n.svelte";
  import { reducedMotion } from "./lib/motion";
  import { fadeQuick, pageIn, rise } from "./lib/transitions";
  import AlbumsView from "./components/AlbumsView.svelte";
  import AlbumView from "./components/AlbumView.svelte";
  import AmbientBackdrop from "./components/AmbientBackdrop.svelte";
  import Cover from "./components/Cover.svelte";
  import DiscFlight from "./components/DiscFlight.svelte";
  import Dialog from "./components/Dialog.svelte";
  import ArtistsView from "./components/ArtistsView.svelte";
  import ArtistView from "./components/ArtistView.svelte";
  import HomeView from "./components/HomeView.svelte";
  import Icon, { type IconName } from "./components/Icon.svelte";
  import MiniDisc from "./components/MiniDisc.svelte";
  import NowPlayingView from "./components/NowPlayingView.svelte";
  import PlaylistsView from "./components/PlaylistsView.svelte";
  import PlaylistView from "./components/PlaylistView.svelte";
  import LyricsInspector from "./components/LyricsInspector.svelte";
  import QueuePanel from "./components/QueuePanel.svelte";
  import SearchView from "./components/SearchView.svelte";
  import SettingsView from "./components/SettingsView.svelte";
  import SongsView from "./components/SongsView.svelte";
  import TransportBar from "./components/TransportBar.svelte";

  type NavItem = { view: View; label: string; icon: IconName; hint?: string };
  let top = $derived<NavItem[]>([
    { view: { kind: "home" }, label: t("nav.home"), icon: "home" },
    { view: { kind: "search" }, label: t("common.search"), icon: "search", hint: "Ctrl F" },
  ]);
  let library = $derived<NavItem[]>([
    { view: { kind: "albums" }, label: t("common.albums"), icon: "albums" },
    { view: { kind: "artists" }, label: t("common.artists"), icon: "artists" },
    { view: { kind: "songs" }, label: t("common.songs"), icon: "note" },
  ]);
  /** Playlists listed in the sidebar itself; the rest are one click away in "All playlists". */
  const SIDEBAR_PLAYLISTS = 8;
  let sidePlaylists = $derived(app.playlists.slice(0, SIDEBAR_PLAYLISTS));
  let openPlaylist = $derived(app.view.kind === "playlist" ? app.view.id : null);
  let allPlaylistsActive = $derived(
    app.view.kind === "playlists" || (openPlaylist != null && !sidePlaylists.some((p) => p.id === openPlaylist)),
  );

  async function newPlaylist() {
    const created = await app.createPlaylist();
    if (created) app.go({ kind: "playlist", id: created.id });
  }

  let ready = $state(false);
  onMount(() => {
    app.init().then(() => (ready = true));
  });

  let onNow = $derived(app.view.kind === "now");
  let errors = $derived(app.notices.filter((n) => n.level === "error"));
  let others = $derived(app.notices.filter((n) => n.level !== "error"));
  const flipMs = reducedMotion() ? 0 : 200;
  let detail = $derived(app.view.kind === "album" || app.view.kind === "artist" || app.view.kind === "playlist");

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
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "b" && !app.narrow) {
      e.preventDefault();
      app.toggleSidebar();
    } else if (e.key === "Escape" && !e.defaultPrevented && !app.dialog && app.inspector) {
      // The inspector's own keydown only fires with focus inside it; opening it from the pill
      // leaves focus on the toggle, so Escape has to be handled here. A control that already
      // consumed it (a menu, a tonearm drag) keeps it.
      e.preventDefault();
      app.closeInspector();
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
  class:rail={app.rail}
  data-glass={app.settings.glass ?? "full"}
  style:--wash={app.wash ?? "transparent"}
  style:--inset-right={app.docked ? "calc(var(--inspector-w) + var(--gutter) * 2)" : "0px"}
>
  <AmbientBackdrop
    colors={app.current?.artColors ?? []}
    strength={onNow ? 1 : 0.6}
    still={(app.settings.glass ?? "full") === "off"}
  />

  <main class:full={onNow}>
    {#if detail && app.history.length}
      <button class="back glass" aria-label={t("nav.back")} onclick={() => app.back()}>
        <Icon name="back" size={20} />
      </button>
    {/if}
    <div class="content">
      {#if ready}
        <!-- Each view fades in on its own; the old one leaves at once, so nothing outside the
             content (sidebar, player) is touched by a view change. -->
        {#key app.view}
          <div class="view" in:pageIn>
            {#if app.view.kind === "home"}
              <HomeView />
            {:else if app.view.kind === "albums"}
              <AlbumsView />
            {:else if app.view.kind === "album"}
              <AlbumView id={app.view.id} />
            {:else if app.view.kind === "artists"}
              <ArtistsView />
            {:else if app.view.kind === "artist"}
              <ArtistView id={app.view.id} />
            {:else if app.view.kind === "songs"}
              <SongsView />
            {:else if app.view.kind === "playlists"}
              <PlaylistsView />
            {:else if app.view.kind === "playlist"}
              <PlaylistView id={app.view.id} />
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

  <nav class="sidebar glass" aria-label={t("nav.main")}>
    <div class="brand">
      <svg class="mark" viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="12" cy="12" r="10.5" fill="#0b0b0d" stroke="currentColor" stroke-width="1.5" />
        <circle cx="12" cy="12" r="7" fill="none" stroke="currentColor" stroke-opacity=".35" stroke-width=".8" />
        <circle cx="12" cy="12" r="3.6" fill="currentColor" />
        <circle cx="12" cy="12" r="1" fill="#0b0b0d" />
      </svg>
      <span class="label">Sonder</span>
      {#if !app.narrow}
        <button
          class="icon-btn fold"
          aria-label={app.sidebarCollapsed ? t("nav.expandSidebar") : t("nav.collapseSidebar")}
          title={app.sidebarCollapsed ? t("nav.expandSidebar") : t("nav.collapseSidebar")}
          aria-expanded={!app.sidebarCollapsed}
          onclick={() => app.toggleSidebar()}
        >
          <Icon name="sidebar" size={18} />
        </button>
      {/if}
    </div>

    <!-- The middle scrolls on short windows; the brand, Now Playing and Settings stay put. -->
    <div class="nav-scroll">
      {#each top as item (item.view.kind)}{@render navButton(item)}{/each}

      <span class="group-gap"></span>
      <span class="group kicker">{t("nav.library")}</span>
      {#each library as item (item.view.kind)}{@render navButton(item)}{/each}

      <span class="group-gap"></span>
      <div class="group-row">
        <span class="group kicker">{t("common.playlists")}</span>
        <button class="add icon-btn" aria-label={t("common.newPlaylist")} title={t("common.newPlaylist")} onclick={newPlaylist}>
          <Icon name="add" size={16} />
        </button>
      </div>
      {#if !app.playlists.length}
        <span class="hint">{t("nav.noPlaylists")}</span>
      {/if}
      {#each sidePlaylists as playlist (playlist.id)}
        <button
          class="nav playlist"
          class:active={openPlaylist === playlist.id}
          aria-current={openPlaylist === playlist.id ? "page" : undefined}
          title={playlist.name}
          onclick={() => app.go({ kind: "playlist", id: playlist.id })}
        >
          <span class="pl-art"><Cover hash={playlist.art} alt="" /></span>
          <span class="label">{playlist.name}</span>
        </button>
      {/each}
      <button
        class="nav"
        class:active={allPlaylistsActive}
        aria-current={allPlaylistsActive ? "page" : undefined}
        title={t("nav.allPlaylists")}
        onclick={() => app.go({ kind: "playlists" })}
      >
        <Icon name="playlist" size={20} />
        <span class="label">{t("nav.allPlaylists")}</span>
      </button>
    </div>

    <button
      class="nav"
      class:active={onNow}
      aria-current={onNow ? "page" : undefined}
      title={t("common.nowPlaying")}
      onclick={() => app.go({ kind: "now" })}
    >
      {#if app.current}
        <span class="live"><MiniDisc art={app.current.art} paused={app.player.state !== "playing"} /></span>
      {:else}
        <Icon name="now" size={20} />
      {/if}
      <span class="label">{t("common.nowPlaying")}</span>
    </button>

    {#if app.scan.phase !== "idle"}
      <button class="scan" title={t("nav.scanningTitle")} aria-live="polite" onclick={() => app.go({ kind: "settings" })}>
        <span class="spinner" aria-hidden="true"></span>
        <span class="label"
          >{app.scan.total
            ? t("nav.scanningProgress", { done: app.scan.done, total: app.scan.total })
            : t("nav.scanning")}</span
        >
      </button>
    {/if}
    {@render navButton({ view: { kind: "settings" }, label: t("common.settings"), icon: "settings" })}
  </nav>

  {#if app.inspector}
    <div class="inspector" class:docked={app.docked} transition:slidePanel>
      {#if app.inspector === "queue"}
        <QueuePanel />
      {:else}
        <LyricsInspector />
      {/if}
    </div>
  {/if}

  <TransportBar hidden={onNow} />

  <Dialog />

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
          <button class="dismiss" aria-label={t("common.dismiss")} onclick={() => app.dismiss(n.id)}><Icon name="close" size={16} /></button>
        </div>
      {/each}
    </div>
    <div class="region" role="status" aria-live="polite">
      {#each others as n (n.id)}
        <div class="toast glass {n.level}" in:rise out:fadeQuick animate:flip={{ duration: flipMs }}>
          <span class="dot" aria-hidden="true"></span>
          <span class="msg">{n.message}</span>
          <button class="dismiss" aria-label={t("common.dismiss")} onclick={() => app.dismiss(n.id)}><Icon name="close" size={16} /></button>
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

  /* Three blocks side by side: the glass sidebar, this page sheet, and the inspector when it
     is docked. The sheet is a tinted panel (not glass) that clips its page, and the player
     pill floats inside its bottom edge. Now Playing instead spans the whole window under the
     sidebar, its text kept clear of it with --inset-left. */
  main {
    position: absolute;
    z-index: 1;
    top: var(--gutter);
    bottom: var(--gutter);
    left: var(--inset-left);
    right: max(var(--gutter), var(--inset-right));
    border-radius: var(--r-lg);
    overflow: hidden;
    background: color-mix(in srgb, var(--wash) 5%, rgb(255 255 255 / 0.035));
    transition: background 900ms var(--ease-standard);
  }
  /* the sheet's edge, drawn over the page so full-bleed heroes do not hide it */
  main::after {
    content: "";
    position: absolute;
    inset: 0;
    z-index: 6;
    border-radius: inherit;
    box-shadow: inset 0 0 0 1px var(--hairline);
    pointer-events: none;
  }
  main.full {
    inset: 0;
    border-radius: 0;
    background: none;
  }
  main.full::after {
    content: none;
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
    left: 20px;
    z-index: 7;
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
  .fold {
    width: 36px;
    height: 36px;
    margin-right: -4px;
  }
  .rail .fold {
    margin-right: 0;
  }
  .group {
    padding: 0 14px 6px;
    /* on glass: the fainter kicker ink is for the canvas and soft surfaces only */
    color: var(--ink-2);
  }
  .hint {
    padding: 2px 14px 8px;
    font-size: var(--fs-small);
    color: var(--ink-2);
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
  .nav-scroll {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0 -6px;
    padding: 0 6px 8px;
    overflow-y: auto;
    scrollbar-width: none;
  }
  .nav-scroll::-webkit-scrollbar {
    display: none;
  }
  .group-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-right: 6px;
  }
  .group-row .group {
    padding-bottom: 0;
  }
  .add {
    width: 32px;
    height: 32px;
    color: var(--ink-2);
  }
  .nav.playlist {
    height: 40px;
    gap: 12px;
    padding-left: 12px;
    font-weight: 550;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pl-art {
    width: 24px;
    flex: none;
    border-radius: 6px;
    overflow: hidden;
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

  /* Queue or lyrics. Floating, it is a card over the page that stops above the player pill;
     docked, it runs the full height and the page and pill narrow beside it (--inset-right).
     The page's width changes at once rather than animating, so long grids reflow once. */
  .inspector {
    position: absolute;
    z-index: 15;
    top: var(--gutter);
    right: var(--gutter);
    bottom: calc(var(--dock-h) + var(--gutter) * 3);
    width: var(--inspector-w);
    display: flex;
  }
  .inspector.docked {
    bottom: var(--gutter);
  }

  .toasts {
    position: absolute;
    z-index: 30;
    right: calc(var(--gutter) * 2 + var(--inset-right));
    bottom: calc(var(--gutter) * 2);
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 440px;
    transition: bottom var(--dur-slow) var(--ease-out);
  }
  .toasts.raised {
    bottom: calc(var(--dock-h) + var(--gutter) * 4);
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

  /* The icon rail: folded by the user, or forced on narrow windows (app.rail). */
  .shell.rail {
    --sidebar: 72px;
  }
  .rail .sidebar {
    padding: 16px 10px 10px;
    align-items: center;
  }
  .rail .brand {
    flex-direction: column;
    gap: 10px;
    padding: 4px 0 18px;
  }
  .rail .label,
  .rail kbd,
  .rail .group,
  .rail .group-row,
  .rail .hint,
  .rail .nav.playlist {
    display: none;
  }
  .rail .nav-scroll {
    align-items: center;
  }
  .rail .nav,
  .rail .scan {
    width: 52px;
    padding: 0;
    justify-content: center;
  }
</style>
