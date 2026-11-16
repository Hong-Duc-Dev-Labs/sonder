import { mount } from "svelte";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "@fontsource-variable/momo-trust-sans";
import "@fontsource/momo-signature";
import "./app.css";
import App from "./App.svelte";
import MiniPlayer from "./components/MiniPlayer.svelte";
import { trackModality } from "./lib/modality";

trackModality(window, document.documentElement);

// In dev mode outside Tauri (e.g. Vite dev in a browser), provide a lightweight mock
// so the UI can be previewed without crashing on missing IPC.
if (import.meta.env.DEV && typeof window !== "undefined" && !(window as any).__TAURI_INTERNALS__) {
  let cbId = 1;
  (window as any).__TAURI_INTERNALS__ = {
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
      if (cmd === "library_albums") return { items: [], total: 0 };
      if (cmd === "library_stats") return { totalTracks: 0, totalAlbums: 0, totalArtists: 0, totalTimeSec: 0, playCount: 0 };
      if (cmd === "sleep_timer_get") return { endsAtMs: null, endOfTrack: false, fired: false };
      return null;
    },
  };
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
