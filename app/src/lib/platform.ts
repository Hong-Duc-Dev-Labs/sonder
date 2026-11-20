export type DesktopOs = "macos" | "windows" | "linux";

export type ShortcutKind = "search" | "sidebar" | "back";

export type GlobalShortcutAction =
  | "togglePlay"
  | "search"
  | "toggleSidebar"
  | "closeInspector"
  | "back";

export type ShortcutKeyEvent = {
  key: string;
  code: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  repeat?: boolean;
  defaultPrevented?: boolean;
};

export type ShortcutContext = {
  modalOpen: boolean;
  editable: boolean;
  ownsSpace: boolean;
  narrow: boolean;
  hasInspector: boolean;
};

function isDesktopOs(value: unknown): value is DesktopOs {
  return value === "macos" || value === "windows" || value === "linux";
}

/**
 * Detects the host desktop OS from platform / user-agent strings.
 * Falls back to `"linux"` for unrecognized Unix-like desktops.
 */
export function detectOs(userAgent?: string, platform?: string): DesktopOs {
  const nav = typeof navigator !== "undefined" ? navigator : undefined;
  const ua = userAgent ?? (platform === undefined ? (nav?.userAgent ?? "") : "");
  const plat = platform ?? (userAgent === undefined ? (nav?.platform ?? "") : "");

  if (/mac/i.test(plat) || /macintosh|mac os x/i.test(ua)) {
    return "macos";
  }
  if (/win/i.test(plat) || /windows/i.test(ua)) {
    return "windows";
  }
  return "linux";
}

/**
 * Resolves the active desktop OS:
 * 1. `?os=macos|windows|linux` preview override in Vite dev mode,
 * 2. Rust compile-time `window.__SONDER_OS__` injected at window creation,
 * 3. `detectOs()` fallback for non-Tauri browser previews.
 */
export function resolveDesktopOs(): DesktopOs {
  if (import.meta.env?.DEV && typeof window !== "undefined") {
    const override = new URLSearchParams(window.location.search).get("os");
    if (isDesktopOs(override)) return override;
  }
  const injected = (globalThis as { __SONDER_OS__?: unknown }).__SONDER_OS__;
  if (isDesktopOs(injected)) return injected;
  return detectOs();
}

const NON_MAC_SHORTCUTS: Record<ShortcutKind, string> = {
  search: "Ctrl F",
  sidebar: "Ctrl+B",
  back: "Alt+←",
};

const SHORTCUTS: Record<DesktopOs, Record<ShortcutKind, string>> = {
  macos: {
    search: "⌘F",
    sidebar: "⌘B",
    back: "⌘[",
  },
  windows: NON_MAC_SHORTCUTS,
  linux: NON_MAC_SHORTCUTS,
};

/** Returns a compact, OS-idiomatic shortcut label for tooltips and nav hints. */
export function formatShortcut(os: DesktopOs, kind: ShortcutKind): string {
  return SHORTCUTS[os][kind];
}

/** Returns keycap segments for inline `<kbd>` rendering. */
export function shortcutKeys(os: DesktopOs, _kind: "search"): [string, string] {
  return os === "macos" ? ["⌘", "F"] : ["Ctrl", "F"];
}

/**
 * Resolves global shell keyboard shortcuts with strict OS-idiomatic modifiers and input/modal guards.
 * Never intercepts `Option+Left` word-navigation on macOS or fires shortcuts behind open modals.
 */
export function resolveGlobalShortcut(
  e: ShortcutKeyEvent,
  os: DesktopOs,
  ctx: ShortcutContext,
): GlobalShortcutAction | null {
  if (ctx.modalOpen) return null;

  if (e.code === "Space" && !e.repeat && !ctx.ownsSpace) {
    return "togglePlay";
  }

  const primary =
    os === "macos"
      ? e.metaKey && !e.ctrlKey && !e.altKey
      : e.ctrlKey && !e.metaKey && !e.altKey;
  const lower = e.key.toLowerCase();

  if (primary && lower === "f") {
    return "search";
  }
  if (primary && lower === "b" && !ctx.narrow) {
    return "toggleSidebar";
  }
  if (e.key === "Escape" && !e.defaultPrevented && ctx.hasInspector) {
    return "closeInspector";
  }
  if (os === "macos") {
    if (e.metaKey && !e.ctrlKey && !e.altKey && e.key === "[") {
      return "back";
    }
  } else if (!ctx.editable && e.altKey && !e.ctrlKey && !e.metaKey && e.key === "ArrowLeft") {
    return "back";
  }

  return null;
}
