import { mount } from "svelte";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "@fontsource-variable/momo-trust-sans";
import "@fontsource/momo-signature";
import "./app.css";
import App from "./App.svelte";
import MiniPlayer from "./components/MiniPlayer.svelte";
import { trackModality } from "./lib/modality";

trackModality(window, document.documentElement);

interface DevTauriWindow extends Window {
  __TAURI_INTERNALS__?: {
    metadata: {
      currentWindow: { label: string };
      currentWebview: { label: string };
    };
    transformCallback: () => number;
    transformPlugin: () => void;
    invoke: (cmd: string) => Promise<unknown>;
  };
}

// In dev mode outside Tauri (e.g. Vite dev in a browser), provide a lightweight mock
// so the UI can be previewed without crashing on missing IPC.
if (import.meta.env.DEV && typeof window !== "undefined") {
  const devWin = window as DevTauriWindow;
  if (!devWin.__TAURI_INTERNALS__) {
    let cbId = 1;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => {},
    };
    devWin.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      transformCallback: () => cbId++,
      transformPlugin: () => {},
      invoke: async (cmd: string) => {
        if (cmd === "settings_get") return { albumSort: "artist", language: "en", glass: "full", coverStyle: "vinyl" };
        if (cmd === "player_snapshot") return { state: "stopped", index: null, elapsedMs: 0, durationMs: null, source: null, output: null, underruns: 0, volumeDb: null, signalPath: null, repeat: "off" };
        if (cmd === "queue_items" || cmd === "library_playlists" || cmd === "library_artists") return [];
        if (cmd === "library_albums") return { albums: [], offset: 0, total: 0 };
        if (cmd === "history_stats") return { list: [], plays: [], totalTracks: 0, totalPlays: 0, totalMs: 0, topTracks: [], topArtists: [], topAlbums: [], recent: [] };
        if (cmd === "library_is_scanning" || cmd === "library_is_analyzing") return false;
        if (cmd === "library_loudness_pending") return 0;
        if (cmd === "sleep_timer_get") return { endsAtMs: null, endOfTrack: false, fired: false };
        if (cmd === "plugin:window|is_maximized" || cmd === "plugin:window|is_fullscreen" || cmd === "plugin:window|is_always_on_top") return false;
        if (cmd === "plugin:window|is_focused") return true;
        if (cmd.startsWith("plugin:window|")) return null;
        if (cmd === "plugin:event|listen") return cbId++;
        if (cmd === "plugin:event|unlisten") return null;
        return null;
      },
    };
  }
}

// Both windows load this page; the mini player window (see src-tauri/src/mini.rs) gets its own UI.
let isMini = false;
try {
  isMini = getCurrentWebviewWindow().label === "mini";
} catch {
  // Running in a standard web browser or preview mode
}
if (isMini) document.documentElement.dataset.window = "mini";

const app = mount(isMini ? MiniPlayer : App, { target: document.getElementById("app")! });

export default app;
