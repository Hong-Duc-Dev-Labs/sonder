// App-wide reactive state: navigation, player snapshot, queue, scan progress and notices.
// Rust pushes changes as events; this module subscribes once and exposes plain runes.

import {
  commands,
  events,
  type Album,
  type NoticeLevel,
  type PlayerSnapshot,
  type QueueItem,
  type ScanPhase,
  type Settings,
} from "./bindings";
import { tick } from "svelte";
import { reducedMotion } from "./motion";

export type View =
  | { kind: "albums" }
  | { kind: "album"; id: number }
  | { kind: "artists" }
  | { kind: "artist"; id: number }
  | { kind: "search" }
  | { kind: "now" }
  | { kind: "settings" };

export type Notice = { id: number; level: NoticeLevel; message: string };

export type ScanState = {
  phase: ScanPhase | "idle";
  done: number;
  total: number;
  last: string | null;
  failures: string[];
};

/**
 * Applies a view change inside a View Transition where the webview has them (the album cover
 * morphs into the album page), or directly where it does not.
 */
function transition(change: () => void) {
  const start = (document as Document & { startViewTransition?: (cb: () => Promise<void>) => unknown })
    .startViewTransition;
  if (!start || reducedMotion()) {
    change();
    tick().then(focusHeading);
    return;
  }
  start.call(document, async () => {
    change();
    await tick();
    focusHeading();
  });
}

/**
 * Moves focus to the new view's heading: the clicked control is gone, and focus left on the
 * page body would drop screen-reader users at the top of the document.
 */
function focusHeading() {
  document.querySelector<HTMLElement>("[data-view-heading]")?.focus({ preventScroll: true });
}

const emptySnapshot: PlayerSnapshot = {
  state: "stopped",
  index: null,
  elapsedMs: 0,
  durationMs: null,
  source: null,
  output: null,
  underruns: 0,
  volumeDb: null,
};

class AppState {
  view = $state<View>({ kind: "albums" });
  history = $state<View[]>([]);
  player = $state<PlayerSnapshot>(emptySnapshot);
  queue = $state<QueueItem[]>([]);
  queueOpen = $state(false);
  settings = $state<Settings>({ albumSort: "artist" });
  scan = $state<ScanState>({ phase: "idle", done: 0, total: 0, last: null, failures: [] });
  notices = $state<Notice[]>([]);
  searchQuery = $state("");
  /** Bumped whenever the library index changes; views reload when it moves. */
  libraryVersion = $state(0);

  current = $derived<QueueItem | null>(
    this.player.index != null ? (this.queue[this.player.index] ?? null) : null,
  );

  /**
   * Album summaries the user just clicked, so an album page can draw its header (and morph the
   * cover into it) before its track list has loaded.
   */
  albumPreview = new Map<number, Album>();

  /** The album whose cover morphs between its card and its page (the one last opened). */
  morphAlbumId = $state<number | null>(null);

  /** Which way the last track change went through the queue: 1 forward, -1 back. */
  trackDirection = $state<1 | -1>(1);

  /** The open page's own colour (an album's or artist's cover), washed over the glass. */
  pageWash = $state<string | null>(null);

  /** Colour washed over the glass chrome: the open page's, else the playing cover's. */
  wash = $derived(this.pageWash ?? this.current?.artColors[0] ?? null);

  private nextNotice = 1;
  /** Auto-dismiss timers; paused while the pointer or focus is on a notice. */
  private noticeTimers = new Map<number, { left: number; started: number; handle: number }>();

  private setPlayer(next: PlayerSnapshot) {
    const prev = this.player.index;
    if (next.index != null && prev != null && next.index !== prev) {
      this.trackDirection = next.index > prev ? 1 : -1;
    }
    this.player = next;
  }

  go(view: View) {
    if (JSON.stringify(view) === JSON.stringify(this.view)) return;
    transition(() => {
      this.history.push(this.view);
      if (this.history.length > 50) this.history.shift();
      this.view = view;
    });
  }

  back() {
    const prev = this.history.at(-1);
    if (!prev) return;
    transition(() => {
      this.history.pop();
      this.view = prev;
    });
  }

  notify(level: NoticeLevel, message: string) {
    const id = this.nextNotice++;
    this.notices.push({ id, level, message });
    this.startNoticeTimer(id, level === "error" ? 10_000 : 6_000);
  }

  private startNoticeTimer(id: number, ms: number) {
    const handle = window.setTimeout(() => this.dismiss(id), ms);
    this.noticeTimers.set(id, { left: ms, started: performance.now(), handle });
  }

  /** Holds every notice on screen while the user reads or interacts with one. */
  pauseNotices() {
    const now = performance.now();
    for (const [id, t] of this.noticeTimers) {
      window.clearTimeout(t.handle);
      this.noticeTimers.set(id, { ...t, left: Math.max(1500, t.left - (now - t.started)), handle: 0 });
    }
  }

  resumeNotices() {
    for (const [id, t] of [...this.noticeTimers]) {
      if (t.handle === 0) this.startNoticeTimer(id, t.left);
    }
  }

  dismiss(id: number) {
    const timer = this.noticeTimers.get(id);
    if (timer) window.clearTimeout(timer.handle);
    this.noticeTimers.delete(id);
    this.notices = this.notices.filter((n) => n.id !== id);
  }

  /** Runs a command, turning a rejection into a notice. */
  async run<T>(work: Promise<T>): Promise<T | undefined> {
    try {
      return await work;
    } catch (err) {
      this.notify("error", typeof err === "string" ? err : String(err));
      return undefined;
    }
  }

  async init() {
    await Promise.all([
      events.playerStateEvent.listen((e) => this.setPlayer(e.payload)),
      events.queueChangedEvent.listen((e) => (this.queue = e.payload)),
      events.noticeEvent.listen((e) => this.notify(e.payload.level, e.payload.message)),
      events.libraryChangedEvent.listen(() => this.libraryVersion++),
      events.scanProgressEvent.listen((e) => {
        this.scan = { ...this.scan, ...e.payload };
      }),
      events.scanFinishedEvent.listen((e) => {
        const r = e.payload;
        const last = r.error
          ? `Scan failed: ${r.error}`
          : `${r.added} added, ${r.updated} updated, ${r.removed} removed` +
            (r.failureCount ? `, ${r.failureCount} unreadable` : "");
        this.scan = { phase: "idle", done: 0, total: 0, last, failures: r.failures };
        if (r.error) this.notify("error", last);
      }),
    ]);
    const [player, queue, settings, scanning] = await Promise.all([
      commands.playerSnapshot(),
      commands.queueItems(),
      commands.settingsGet(),
      commands.libraryIsScanning(),
    ]);
    this.player = player;
    this.queue = queue;
    this.settings = settings;
    if (scanning && this.scan.phase === "idle") this.scan = { ...this.scan, phase: "walking" };
  }

  async saveSettings(next: Settings) {
    const saved = await this.run(commands.settingsSet(next));
    if (saved) this.settings = saved;
  }
}

export const app = new AppState();
