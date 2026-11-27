<script lang="ts">
  // Cross-platform cosmic glass title bar:
  // - macOS: clears native traffic lights (78px inset, collapsing to 14px in fullscreen)
  //   with TitleBarStyle::Overlay handling minimize/maximize/close natively.
  // - Windows: Fluent 11 flush caption buttons (46px wide, Fitts's Law top-right corner)
  //   and 8-way frameless resize handles stopping clear of the close button.
  // - Linux: GNOME/Adwaita-inspired circular frosted controls + 8-way edge/corner resize
  //   handles for frameless Wayland/X11 windows.
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { app } from "../lib/app.svelte";
  import { t } from "../lib/i18n.svelte";
  import { formatShortcut, resolveDesktopOs, type DesktopOs } from "../lib/platform";
  import Icon from "./Icon.svelte";

  type ResizeDirection = Parameters<ReturnType<typeof getCurrentWindow>["startResizeDragging"]>[0];
  type AppWindow = ReturnType<typeof getCurrentWindow>;

  export type WindowChromeState = {
    maximized: boolean;
    fullscreen: boolean;
    focused: boolean;
  };

  let {
    onNow = false,
    onboarding = false,
    os = resolveDesktopOs(),
    onWindowStateChange,
  }: {
    onNow?: boolean;
    onboarding?: boolean;
    os?: DesktopOs;
    onWindowStateChange?: (state: WindowChromeState) => void;
  } = $props();

  let maximized = $state(false);
  let fullscreen = $state(false);
  let focused = $state(true);
  let pinned = $state(false);

  let resizeSeq = 0;
  let pinSeq = 0;

  let canGoBack = $derived(app.history.length > 0);
  let searchHint = $derived(formatShortcut(os, "search"));
  let sidebarLabel = $derived(
    t(app.sidebarCollapsed ? "nav.expandSidebarShortcut" : "nav.collapseSidebarShortcut", {
      shortcut: formatShortcut(os, "sidebar"),
    }),
  );
  let backLabel = $derived(t("nav.backShortcut", { shortcut: formatShortcut(os, "back") }));

  $effect(() => {
    onWindowStateChange?.({ maximized, fullscreen, focused });
  });

  function currentWindow(): AppWindow | null {
    try {
      return getCurrentWindow();
    } catch {
      return null;
    }
  }

  async function withWindow(work: (win: AppWindow) => Promise<unknown>) {
    const win = currentWindow();
    if (!win) return;
    try {
      await work(win);
    } catch {
      // Ignore unavailable window commands in browser preview or tiling WMs.
    }
  }

  async function syncBoundsState(win: AppWindow | null = currentWindow()) {
    if (!win) return;
    const seq = ++resizeSeq;
    const [maxRes, fullRes] = await Promise.allSettled([win.isMaximized(), win.isFullscreen()]);
    if (seq !== resizeSeq) return;
    if (maxRes.status === "fulfilled") maximized = maxRes.value;
    if (fullRes.status === "fulfilled") fullscreen = fullRes.value;
  }

  onMount(() => {
    let alive = true;
    const unlisteners: (() => void)[] = [];
    const settleTimers: number[] = [];
    const win = currentWindow();
    if (!win) return;

    const clearSettleTimers = () => {
      while (settleTimers.length) window.clearTimeout(settleTimers.pop());
    };

    const scheduleBoundsSync = () => {
      void syncBoundsState(win);
      if (os !== "macos") return;
      clearSettleTimers();
      // AppKit animates Space fullscreen transitions across ~500ms; re-read once settled.
      for (const delay of [180, 600]) {
        settleTimers.push(
          window.setTimeout(() => {
            if (alive) void syncBoundsState(win);
          }, delay),
        );
      }
    };

    const attach = async (register: Promise<() => void>) => {
      try {
        const unlisten = await register;
        if (alive) unlisteners.push(unlisten);
        else unlisten();
      } catch {
        // Listener unsupported in browser preview.
      }
    };

    void (async () => {
      await Promise.all([
        attach(win.onResized(scheduleBoundsSync)),
        attach(
          win.onFocusChanged(({ payload }) => {
            if (alive) focused = payload;
          }),
        ),
      ]);

      const [maxRes, fullRes, focRes, pinRes] = await Promise.allSettled([
        win.isMaximized(),
        win.isFullscreen(),
        win.isFocused(),
        win.isAlwaysOnTop(),
      ]);
      if (!alive) return;
      if (maxRes.status === "fulfilled") maximized = maxRes.value;
      if (fullRes.status === "fulfilled") fullscreen = fullRes.value;
      if (focRes.status === "fulfilled") focused = focRes.value;
      if (pinRes.status === "fulfilled") pinned = pinRes.value;
    })();

    return () => {
      alive = false;
      clearSettleTimers();
      for (const unlisten of unlisteners) unlisten();
    };
  });

  const minimize = () => withWindow((win) => win.minimize());
  const closeWindow = () => withWindow((win) => win.close());

  async function toggleMaximize() {
    await withWindow(async (win) => {
      await win.toggleMaximize();
      await syncBoundsState(win);
    });
  }

  async function togglePin() {
    const win = currentWindow();
    if (!win) return;
    const seq = ++pinSeq;
    const prev = pinned;
    const next = !prev;
    pinned = next;
    try {
      await win.setAlwaysOnTop(next);
      const actual = await win.isAlwaysOnTop().catch(() => next);
      if (seq === pinSeq) pinned = actual;
    } catch {
      if (seq === pinSeq) pinned = prev;
    }
  }

  async function startResize(direction: ResizeDirection, e: PointerEvent) {
    if (e.button !== 0 || maximized || fullscreen) return;
    e.preventDefault();
    await withWindow((win) => win.startResizeDragging(direction));
  }

  type ResizeHandle = { dir: ResizeDirection; cls: string };
  const RESIZE_HANDLES: Record<"linux" | "windows", ResizeHandle[]> = {
    linux: [
      { dir: "North", cls: "top" },
      { dir: "South", cls: "bottom" },
      { dir: "West", cls: "left" },
      { dir: "East", cls: "right" },
      { dir: "NorthWest", cls: "top-left" },
      { dir: "NorthEast", cls: "top-right" },
      { dir: "SouthWest", cls: "bottom-left" },
      { dir: "SouthEast", cls: "bottom-right" },
    ],
    windows: [
      { dir: "North", cls: "win-top" },
      { dir: "South", cls: "bottom" },
      { dir: "West", cls: "left" },
      { dir: "East", cls: "right" },
      { dir: "NorthWest", cls: "top-left" },
      { dir: "NorthEast", cls: "top-right" },
      { dir: "SouthWest", cls: "bottom-left" },
      { dir: "SouthEast", cls: "bottom-right" },
    ],
  };
</script>

<header
  class="titlebar"
  data-titlebar
  data-os={os}
  class:on-now={onNow}
  class:maximized
  class:fullscreen
  class:unfocused={!focused}
  data-tauri-drag-region
  aria-label={t("window.titlebar")}
>
  <div class="left" data-tauri-drag-region>
    {#if onboarding}
      <div class="brand-static" data-tauri-drag-region>
        <svg class="mark" viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="12" cy="12" r="10.5" fill="#0b0b0d" stroke="currentColor" stroke-width="1.5" />
          <circle cx="12" cy="12" r="7" fill="none" stroke="currentColor" stroke-opacity=".35" stroke-width=".9" />
          <circle cx="12" cy="12" r="3.6" fill="currentColor" />
          <circle cx="12" cy="12" r="1" fill="#0b0b0d" />
        </svg>
        <span class="wordmark">Sonder</span>
      </div>
    {:else}
      <button
        class="brand-btn"
        aria-label={t("window.home")}
        title={t("nav.home")}
        onclick={() => app.go({ kind: "home" })}
      >
        <svg class="mark" viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="12" cy="12" r="10.5" fill="#0b0b0d" stroke="currentColor" stroke-width="1.5" />
          <circle cx="12" cy="12" r="7" fill="none" stroke="currentColor" stroke-opacity=".35" stroke-width=".9" />
          <circle cx="12" cy="12" r="3.6" fill="currentColor" />
          <circle cx="12" cy="12" r="1" fill="#0b0b0d" />
        </svg>
        <span class="wordmark">Sonder</span>
      </button>
    {/if}

    {#if !onNow && !onboarding && !app.narrow}
      <button
        class="tb-btn sidebar-toggle"
        class:collapsed={app.sidebarCollapsed}
        aria-label={sidebarLabel}
        title={sidebarLabel}
        aria-expanded={!app.sidebarCollapsed}
        onclick={() => app.toggleSidebar()}
      >
        <Icon name="sidebar" size={16} />
      </button>
    {/if}

    {#if !onboarding}
      <button
        class="tb-btn"
        disabled={!canGoBack}
        aria-label={backLabel}
        title={backLabel}
        onclick={() => app.back()}
      >
        <Icon name="back" size={16} />
      </button>
    {/if}
  </div>

  <div class="center" data-tauri-drag-region>
    {#if !onNow && !onboarding}
      <button
        class="search-pill"
        class:active={app.view.kind === "search"}
        aria-label={t("search.label")}
        title={`${t("search.label")} (${searchHint})`}
        onclick={() => app.openSearch()}
      >
        <Icon name="search" size={14} />
        <span class="search-label">
          {app.searchQuery.trim() || t("window.searchPlaceholder")}
        </span>
        <kbd>{searchHint}</kbd>
      </button>
    {/if}
  </div>

  <div class="right" data-tauri-drag-region>
    <button
      class="tb-btn pin"
      class:pinned
      aria-label={pinned ? t("window.unpin") : t("window.pin")}
      title={pinned ? t("window.unpin") : t("window.pin")}
      aria-pressed={pinned}
      onclick={togglePin}
    >
      <Icon name="pin" size={14} />
    </button>

    {#if os === "windows" || os === "linux"}
      {@const isWin = os === "windows"}
      {@const wrapClass = isWin ? "win-controls" : "linux-controls"}
      {@const btnClass = isWin ? "win-btn" : "linux-btn"}
      {@const strokeW = isWin ? 1.2 : 1.35}
      <span class="divider" aria-hidden="true"></span>
      <div class={wrapClass}>
        <button
          class={btnClass}
          aria-label={t("window.minimize")}
          title={t("window.minimize")}
          onclick={minimize}
        >
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <line
              x1={isWin ? 0 : 1.5}
              y1="5"
              x2={isWin ? 10 : 8.5}
              y2="5"
              stroke="currentColor"
              stroke-width={strokeW}
              stroke-linecap={isWin ? undefined : "round"}
            />
          </svg>
        </button>
        <button
          class={btnClass}
          aria-label={maximized ? t("window.restore") : t("window.maximize")}
          title={maximized ? t("window.restore") : t("window.maximize")}
          onclick={toggleMaximize}
        >
          {#if maximized}
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
              {#if isWin}
                <rect x="0.6" y="2.6" width="6.8" height="6.8" rx="1" stroke="currentColor" stroke-width="1.2" />
                <path
                  d="M2.6 2.6V1.6a1 1 0 0 1 1-1h4.8a1 1 0 0 1 1 1v4.8a1 1 0 0 1-1 1h-1"
                  stroke="currentColor"
                  stroke-width="1.2"
                />
              {:else}
                <rect x="1.5" y="3.2" width="5.3" height="5.3" rx="0.8" stroke="currentColor" stroke-width="1.3" />
                <path
                  d="M3.2 3.2V2a0.8 0.8 0 0 1 0.8-0.8h4a0.8 0.8 0 0 1 0.8 0.8v4a0.8 0.8 0 0 1-0.8 0.8H6.8"
                  stroke="currentColor"
                  stroke-width="1.3"
                />
              {/if}
            </svg>
          {:else}
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
              <rect
                x={isWin ? 0.6 : 1.5}
                y={isWin ? 0.6 : 1.5}
                width={isWin ? 8.8 : 7}
                height={isWin ? 8.8 : 7}
                rx={isWin ? 1.2 : 1}
                stroke="currentColor"
                stroke-width={strokeW}
              />
            </svg>
          {/if}
        </button>
        <button
          class="{btnClass} close"
          aria-label={t("window.close")}
          title={t("window.close")}
          onclick={closeWindow}
        >
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <path
              d={isWin ? "M1 1l8 8M9 1L1 9" : "M2 2l6 6M8 2L2 8"}
              stroke="currentColor"
              stroke-width={strokeW}
              stroke-linecap="round"
            />
          </svg>
        </button>
      </div>
    {/if}
  </div>
</header>

{#if !maximized && !fullscreen && (os === "linux" || os === "windows")}
  {#each RESIZE_HANDLES[os] as handle (handle.dir)}
    <div
      class="resize-handle {handle.cls}"
      aria-hidden="true"
      onpointerdown={(e) => startResize(handle.dir, e)}
    ></div>
  {/each}
{/if}

<style>
  .titlebar {
    view-transition-name: titlebar;
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: var(--titlebar-h);
    z-index: 200;
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    background: linear-gradient(180deg, rgb(0 0 0 / 0.36), transparent);
    user-select: none;
    -webkit-user-select: none;
  }

  .left,
  .center,
  .right {
    height: 100%;
    display: flex;
    align-items: center;
    min-width: 0;
    transition:
      opacity var(--dur-fast) var(--ease-standard),
      padding-left var(--dur-med) var(--ease-standard);
  }

  .left {
    gap: 4px;
    padding-left: 14px;
  }
  .titlebar[data-os="macos"]:not(.fullscreen) .left {
    padding-left: 78px;
  }

  .center {
    justify-content: center;
    padding: 0 8px;
  }

  .right {
    justify-content: flex-end;
    gap: 6px;
  }
  .titlebar[data-os="macos"] .right {
    padding-right: 14px;
  }

  .titlebar.on-now {
    background: linear-gradient(180deg, rgb(0 0 0 / 0.28), transparent);
  }
  .titlebar.on-now .left,
  .titlebar.on-now .right {
    opacity: 0.76;
  }
  .titlebar.unfocused .left,
  .titlebar.unfocused .center,
  .titlebar.unfocused .right {
    opacity: 0.72;
  }
  .titlebar:hover .left,
  .titlebar:hover .center,
  .titlebar:hover .right,
  .titlebar:focus-within .left,
  .titlebar:focus-within .center,
  .titlebar:focus-within .right {
    opacity: 1;
  }

  .brand-static,
  .brand-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    padding: 0 8px 0 4px;
    margin-right: 4px;
    border-radius: var(--r-pill);
    color: var(--ink);
    flex: none;
  }
  .brand-btn {
    transition:
      background var(--dur-fast) var(--ease-standard),
      transform var(--dur-med) var(--ease-spring);
  }
  .brand-btn:hover {
    background: var(--soft);
  }
  .brand-btn:active {
    transform: scale(0.96);
  }
  .mark {
    width: 18px;
    height: 18px;
    flex: none;
    color: var(--ink);
    transition: transform 0.6s cubic-bezier(0.34, 1.25, 0.64, 1);
  }
  .brand-btn:hover .mark {
    transform: rotate(360deg);
  }
  .wordmark {
    font-family: var(--font-signature);
    font-size: 16px;
    font-weight: 400;
    line-height: 1;
    letter-spacing: 0;
  }

  .tb-btn {
    width: 28px;
    height: 28px;
    flex: none;
    display: inline-grid;
    place-items: center;
    border-radius: 50%;
    color: var(--ink-2);
    transition:
      background var(--dur-fast) var(--ease-standard),
      color var(--dur-fast) var(--ease-standard),
      transform var(--dur-med) var(--ease-spring),
      opacity var(--dur-fast) var(--ease-standard);
  }
  .tb-btn:hover:not(:disabled) {
    background: var(--soft-hover);
    color: var(--ink);
  }
  .tb-btn:active:not(:disabled) {
    transform: scale(0.9);
  }
  .tb-btn:disabled {
    opacity: 0.28;
    cursor: default;
  }

  .sidebar-toggle :global(svg) {
    transition: transform var(--dur-med) var(--ease-spring);
  }
  .sidebar-toggle.collapsed :global(svg) {
    transform: rotate(180deg);
  }

  .pin :global(svg) {
    transform: rotate(35deg);
    transition: transform var(--dur-med) var(--ease-spring);
  }
  .pin.pinned {
    color: var(--ink);
    background: var(--soft-active);
    box-shadow: inset 0 0 0 1px var(--hairline);
  }
  .pin.pinned :global(svg) {
    transform: rotate(0deg);
  }

  .search-pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    max-width: min(300px, 100%);
    min-width: 0;
    padding: 0 8px 0 12px;
    border-radius: var(--r-pill);
    background: var(--soft);
    box-shadow: inset 0 0 0 1px var(--hairline);
    color: var(--ink-2);
    font-size: var(--fs-small);
    transition:
      background var(--dur-fast) var(--ease-standard),
      color var(--dur-fast) var(--ease-standard),
      box-shadow var(--dur-fast) var(--ease-standard),
      transform var(--dur-med) var(--ease-spring);
  }
  .search-pill:hover,
  .search-pill.active {
    background: var(--soft-hover);
    color: var(--ink);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.14);
  }
  .search-pill:active {
    transform: scale(0.98);
  }
  .search-pill :global(svg) {
    flex: none;
  }
  .search-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .search-pill kbd {
    flex: none;
    font-family: var(--font);
    font-size: 10.5px;
    font-weight: 650;
    line-height: 1;
    color: var(--ink-2);
    padding: 3px 6px;
    border-radius: 999px;
    background: rgb(255 255 255 / 0.05);
    box-shadow: inset 0 0 0 1px var(--hairline);
  }

  .divider {
    width: 1px;
    height: 14px;
    background: var(--hairline);
    pointer-events: none;
    flex: none;
  }

  /* Windows 11 Fluent caption buttons: flush to top-right edge for Fitts's Law. */
  .win-controls {
    display: flex;
    align-items: stretch;
    height: 100%;
    flex: none;
  }
  .win-btn {
    width: 46px;
    height: 100%;
    display: grid;
    place-items: center;
    color: var(--ink-2);
    transition:
      background var(--dur-fast) var(--ease-standard),
      color var(--dur-fast) var(--ease-standard);
  }
  .win-btn:hover {
    background: rgb(255 255 255 / 0.08);
    color: var(--ink);
  }
  .win-btn:active {
    background: rgb(255 255 255 / 0.13);
  }
  .win-btn.close:hover {
    background: rgb(232 17 35 / 0.88);
    color: #fff;
  }
  .win-btn.close:active {
    background: rgb(232 17 35 / 0.72);
  }
  :global(:root:not([data-modality="pointer"])) .win-btn:focus-visible {
    outline: 2px solid var(--ink);
    outline-offset: -3px;
    box-shadow: inset 0 0 0 4px rgb(0 0 0 / 0.55);
  }

  /* Linux GNOME/Adwaita-inspired circular frosted window controls. */
  .linux-controls {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-right: 10px;
    flex: none;
  }
  .linux-btn {
    width: 26px;
    height: 26px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--soft);
    color: var(--ink-2);
    box-shadow: inset 0 0 0 1px var(--hairline);
    transition:
      background var(--dur-fast) var(--ease-standard),
      color var(--dur-fast) var(--ease-standard),
      transform var(--dur-med) var(--ease-spring);
  }
  .linux-btn:hover {
    background: var(--soft-hover);
    color: var(--ink);
  }
  .linux-btn:active {
    transform: scale(0.9);
  }
  .linux-btn.close:hover {
    background: rgb(235 87 87 / 0.85);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.15);
    color: #fff;
  }

  /* Invisible frameless resize handles (8-way on Windows & Linux when floating). */
  .resize-handle {
    position: fixed;
    z-index: 210;
  }
  .resize-handle.top,
  .resize-handle.bottom {
    left: 6px;
    right: 6px;
    height: 5px;
  }
  .resize-handle.top {
    top: 0;
    cursor: n-resize;
  }
  .resize-handle.win-top {
    top: 0;
    left: 6px;
    /* Stop clear of the 138px Windows caption cluster so top-right close stays unobstructed. */
    right: 142px;
    height: 4px;
    cursor: n-resize;
  }
  .resize-handle.bottom {
    bottom: 0;
    cursor: s-resize;
  }
  .resize-handle.left,
  .resize-handle.right {
    top: 6px;
    bottom: 6px;
    width: 5px;
  }
  .resize-handle.left {
    left: 0;
    cursor: w-resize;
  }
  .resize-handle.right {
    right: 0;
    cursor: e-resize;
  }
  .resize-handle.top-left,
  .resize-handle.top-right,
  .resize-handle.bottom-left,
  .resize-handle.bottom-right {
    width: 6px;
    height: 6px;
  }
  .resize-handle.top-left {
    top: 0;
    left: 0;
    cursor: nw-resize;
  }
  .resize-handle.top-right {
    top: 0;
    right: 0;
    cursor: ne-resize;
  }
  .resize-handle.bottom-left {
    bottom: 0;
    left: 0;
    cursor: sw-resize;
  }
  .resize-handle.bottom-right {
    bottom: 0;
    right: 0;
    cursor: se-resize;
  }

  @media (max-width: 780px) {
    .search-pill {
      padding-right: 12px;
    }
    .search-pill kbd {
      display: none;
    }
  }

  @media (max-width: 760px) {
    .titlebar {
      grid-template-columns: auto minmax(0, 1fr) auto;
    }
  }

  @media (max-width: 680px) {
    .brand-btn {
      padding-right: 6px;
      margin-right: 0;
    }
    .search-pill {
      max-width: min(200px, 100%);
      padding: 0 10px;
      gap: 6px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .brand-btn:hover .mark {
      transform: none;
    }
    .brand-btn:active,
    .tb-btn:active:not(:disabled),
    .search-pill:active,
    .linux-btn:active {
      transform: none;
    }
    .pin :global(svg),
    .sidebar-toggle :global(svg) {
      transition: none;
    }
  }
</style>
