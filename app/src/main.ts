import { mount } from "svelte";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "@fontsource-variable/inter";
import "./app.css";
import App from "./App.svelte";
import MiniPlayer from "./components/MiniPlayer.svelte";
import { trackModality } from "./lib/modality";

trackModality(window, document.documentElement);

// Both windows load this page; the mini player window (see src-tauri/src/mini.rs) gets its own UI.
const mini = getCurrentWebviewWindow().label === "mini";
if (mini) document.documentElement.dataset.window = "mini";

const app = mount(mini ? MiniPlayer : App, { target: document.getElementById("app")! });

export default app;
