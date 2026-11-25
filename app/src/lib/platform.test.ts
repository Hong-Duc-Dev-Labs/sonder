import { afterEach, describe, expect, it } from "vitest";
import {
  detectOs,
  formatShortcut,
  resolveDesktopOs,
  resolveGlobalShortcut,
  shortcutKeys,
  type ShortcutContext,
  type ShortcutKeyEvent,
} from "./platform";

const baseCtx: ShortcutContext = {
  modalOpen: false,
  editable: false,
  ownsSpace: false,
  narrow: false,
  hasInspector: false,
};

function key(overrides: Partial<ShortcutKeyEvent>): ShortcutKeyEvent {
  return {
    key: "",
    code: "",
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    ...overrides,
  };
}

describe("detectOs", () => {
  it("identifies macOS from platform or user agent", () => {
    expect(detectOs("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", "MacIntel")).toBe("macos");
    expect(detectOs("Mozilla/5.0 (Macintosh; Apple Silicon Mac OS X)")).toBe("macos");
    expect(detectOs(undefined, "MacARM64")).toBe("macos");
  });

  it("identifies Windows from platform or user agent", () => {
    expect(detectOs("Mozilla/5.0 (Windows NT 10.0; Win64; x64)", "Win32")).toBe("windows");
    expect(detectOs("Mozilla/5.0 (Windows NT 11.0; Win64; x64)")).toBe("windows");
    expect(detectOs(undefined, "Win64")).toBe("windows");
  });

  it("identifies Linux and falls back to linux for unknown Unix hosts", () => {
    expect(detectOs("Mozilla/5.0 (X11; Linux x86_64)", "Linux x86_64")).toBe("linux");
    expect(detectOs("Mozilla/5.0 (X11; Ubuntu; Linux x86_64)")).toBe("linux");
    expect(detectOs("", "")).toBe("linux");
  });
});

describe("resolveDesktopOs", () => {
  const g = globalThis as { __SONDER_OS__?: unknown; window?: unknown };
  const prevOs = g.__SONDER_OS__;
  const prevWindow = g.window;

  afterEach(() => {
    if (prevOs === undefined) delete g.__SONDER_OS__;
    else g.__SONDER_OS__ = prevOs;
    if (prevWindow === undefined) delete g.window;
    else g.window = prevWindow;
  });

  it("prefers compile-time __SONDER_OS__ when valid and ignores invalid values", () => {
    g.__SONDER_OS__ = "macos";
    expect(resolveDesktopOs()).toBe("macos");

    g.__SONDER_OS__ = "freebsd";
    expect(resolveDesktopOs()).toBe(detectOs());
  });

  it("allows ?os= override in DEV while ignoring invalid query values", () => {
    g.__SONDER_OS__ = "windows";
    g.window = { location: { search: "?os=linux" } };
    expect(resolveDesktopOs()).toBe("linux");

    g.window = { location: { search: "?os=beos" } };
    expect(resolveDesktopOs()).toBe("windows");
  });
});

describe("shortcut helpers", () => {
  it("formats OS-idiomatic shortcut strings", () => {
    expect(formatShortcut("macos", "search")).toBe("⌘F");
    expect(formatShortcut("macos", "sidebar")).toBe("⌘B");
    expect(formatShortcut("macos", "back")).toBe("⌘[");

    for (const os of ["windows", "linux"] as const) {
      expect(formatShortcut(os, "search")).toBe("Ctrl F");
      expect(formatShortcut(os, "sidebar")).toBe("Ctrl+B");
      expect(formatShortcut(os, "back")).toBe("Alt+←");
    }
  });

  it("returns keycap pairs for search", () => {
    expect(shortcutKeys("macos", "search")).toEqual(["⌘", "F"]);
    expect(shortcutKeys("windows", "search")).toEqual(["Ctrl", "F"]);
    expect(shortcutKeys("linux", "search")).toEqual(["Ctrl", "F"]);
  });
});

describe("resolveGlobalShortcut", () => {
  it("never intercepts Option+Left on macOS and uses Cmd+[ for Back", () => {
    const optionLeft = key({ key: "ArrowLeft", code: "ArrowLeft", altKey: true });
    expect(resolveGlobalShortcut(optionLeft, "macos", baseCtx)).toBeNull();
    expect(resolveGlobalShortcut(optionLeft, "macos", { ...baseCtx, editable: true })).toBeNull();

    const cmdBracket = key({ key: "[", code: "BracketLeft", metaKey: true });
    expect(resolveGlobalShortcut(cmdBracket, "macos", baseCtx)).toBe("back");
  });

  it("gates macOS search and sidebar to Cmd without Ctrl", () => {
    expect(resolveGlobalShortcut(key({ key: "f", metaKey: true }), "macos", baseCtx)).toBe("search");
    expect(resolveGlobalShortcut(key({ key: "b", metaKey: true }), "macos", baseCtx)).toBe("toggleSidebar");
    expect(resolveGlobalShortcut(key({ key: "f", ctrlKey: true }), "macos", baseCtx)).toBeNull();
    expect(resolveGlobalShortcut(key({ key: "b", ctrlKey: true }), "macos", baseCtx)).toBeNull();
  });

  it("guards Alt+ArrowLeft on Windows/Linux inside editable fields", () => {
    const altLeft = key({ key: "ArrowLeft", code: "ArrowLeft", altKey: true });
    expect(resolveGlobalShortcut(altLeft, "windows", baseCtx)).toBe("back");
    expect(resolveGlobalShortcut(altLeft, "linux", baseCtx)).toBe("back");
    expect(resolveGlobalShortcut(altLeft, "windows", { ...baseCtx, editable: true })).toBeNull();
    expect(resolveGlobalShortcut(altLeft, "linux", { ...baseCtx, editable: true })).toBeNull();
  });

  it("blocks all shortcuts while a modal dialog is open", () => {
    const modal = { ...baseCtx, modalOpen: true, hasInspector: true };
    expect(resolveGlobalShortcut(key({ code: "Space" }), "windows", modal)).toBeNull();
    expect(resolveGlobalShortcut(key({ key: "f", ctrlKey: true }), "windows", modal)).toBeNull();
    expect(resolveGlobalShortcut(key({ key: "b", ctrlKey: true }), "windows", modal)).toBeNull();
    expect(resolveGlobalShortcut(key({ key: "Escape" }), "windows", modal)).toBeNull();
    expect(resolveGlobalShortcut(key({ key: "ArrowLeft", altKey: true }), "windows", modal)).toBeNull();
  });

  it("respects narrow mode for sidebar toggle and ownsSpace for Space", () => {
    expect(
      resolveGlobalShortcut(key({ key: "b", ctrlKey: true }), "windows", { ...baseCtx, narrow: true }),
    ).toBeNull();
    expect(resolveGlobalShortcut(key({ code: "Space" }), "windows", { ...baseCtx, ownsSpace: true })).toBeNull();
    expect(resolveGlobalShortcut(key({ code: "Space" }), "windows", baseCtx)).toBe("togglePlay");
  });

  it("handles seek, track step, volume, panel toggles, and fullscreen shortcuts with guards", () => {
    expect(resolveGlobalShortcut(key({ key: "ArrowLeft" }), "windows", baseCtx)).toBe("seekBack");
    expect(resolveGlobalShortcut(key({ key: "ArrowRight", shiftKey: true }), "windows", baseCtx)).toBe("seekForwardLong");
    expect(resolveGlobalShortcut(key({ key: "ArrowLeft" }), "windows", { ...baseCtx, ownsSpace: true })).toBeNull();
    expect(resolveGlobalShortcut(key({ key: "ArrowRight" }), "windows", { ...baseCtx, editable: true })).toBeNull();

    expect(resolveGlobalShortcut(key({ key: "ArrowLeft", ctrlKey: true }), "windows", baseCtx)).toBe("previousTrack");
    expect(resolveGlobalShortcut(key({ key: "ArrowRight", metaKey: true }), "macos", baseCtx)).toBe("nextTrack");
    expect(resolveGlobalShortcut(key({ key: "ArrowUp", ctrlKey: true }), "linux", baseCtx)).toBe("volumeUp");
    expect(resolveGlobalShortcut(key({ key: "ArrowDown", metaKey: true }), "macos", baseCtx)).toBe("volumeDown");
    expect(
      resolveGlobalShortcut(key({ key: "ArrowLeft", ctrlKey: true }), "windows", { ...baseCtx, editable: true }),
    ).toBeNull();
    expect(
      resolveGlobalShortcut(key({ key: "ArrowRight", metaKey: true }), "macos", { ...baseCtx, editable: true }),
    ).toBeNull();

    expect(resolveGlobalShortcut(key({ key: "l" }), "windows", baseCtx)).toBe("toggleLyrics");
    expect(resolveGlobalShortcut(key({ key: "q" }), "windows", baseCtx)).toBe("toggleQueue");
    expect(resolveGlobalShortcut(key({ key: "n" }), "windows", baseCtx)).toBe("toggleNowPlaying");
    expect(resolveGlobalShortcut(key({ key: "l" }), "windows", { ...baseCtx, editable: true })).toBeNull();

    expect(resolveGlobalShortcut(key({ key: "F11" }), "windows", baseCtx)).toBe("toggleFullscreen");
    expect(resolveGlobalShortcut(key({ key: "f", metaKey: true, ctrlKey: true }), "macos", baseCtx)).toBe("toggleFullscreen");
    expect(resolveGlobalShortcut(key({ key: "l", defaultPrevented: true }), "windows", baseCtx)).toBeNull();
  });
});
