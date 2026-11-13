// App-wide reactive state: navigation, player snapshot, queue, scan progress and notices.
// Rust pushes changes as events; this module subscribes once and exposes plain runes.

import {
  commands,
  events,
  type Album,
  type NoticeLevel,
  type PlayerSnapshot,
  type Playlist,
  type QueueItem,
  type ScanPhase,
  type Settings,
} from "./bindings";
import { tick } from "svelte";
import { reducedMotion } from "./motion";

export type View =
  | { kind: "home" }
  | { kind: "albums" }
  | { kind: "album"; id: number }
  | { kind: "artists" }
  | { kind: "artist"; id: number }
  | { kind: "playlists" }
  | { kind: "playlist"; id: number }
  | { kind: "search" }
  | { kind: "now" }
  | { kind: "settings" };

/** Notices from Rust are warnings or errors; the UI adds plain confirmations ("info"). */
export type Notice = { id: number; level: NoticeLevel | "info"; message: string };

/** A question the app asks in its modal dialog; see `AppState.ask`. */
export type DialogRequest = {
  title: string;
  message?: string;
  /** Shows a text field starting with this value; the answer is its trimmed text. */
  input?: string;
  confirm: string;
  /** The confirm button reads as destructive. */
  danger?: boolean;
  resolve: (answer: string | null) => void;
};

/**
 * The record flying between Now Playing's stage and the player pill when either view opens.
 * `from` and `to` are in viewport pixels; `to` is where the target settles once its own
 * entrance (the pill sliding up, the page fading in) has finished.
 */
export type DiscFlight = {
  from: DOMRect;
  to: DOMRect;
  target: "pill" | "stage";
  art: string | null;
  /** How the record looks at each end: the square cover style flies as a rounded square. */
  fromRound: boolean;
  toRound: boolean;
  /**
   * Set when the flying record reaches its target: the target shows from then on, underneath
   * it, and the flying copy fades away on top. Until then the target stays hidden.
   */
  landed?: boolean;
};

/** Where `el` will be once the transforms its ancestors are animating through have ended. */
function settledRect(el: HTMLElement): DOMRect {
  const r = el.getBoundingClientRect();
  let dx = 0;
  let dy = 0;
  for (let a: HTMLElement | null = el.parentElement; a; a = a.parentElement) {
    const t = getComputedStyle(a).transform;
    if (t && t !== "none") {
      const m = new DOMMatrix(t);
      dx += m.m41;
      dy += m.m42;
    }
  }
  return new DOMRect(r.left - dx, r.top - dy, r.width, r.height);
}

export type ScanState = {
  phase: ScanPhase | "idle";
  done: number;
  total: number;
  last: string | null;
  failures: string[];
};

/** The background loudness analysis, as its events describe it. */
export type LoudnessState = {
  running: boolean;
  done: number;
  total: number;
  /** Tracks still waiting for a measurement; `null` until known. */
  pending: number | null;
  /** Summary of the last finished pass. */
  last: string | null;
};

/** Set while a View Transition animates the page, so the content's own entrance stays still. */
let viewTransitionRunning = false;
export function viewTransitionActive(): boolean {
  return viewTransitionRunning;
}

/**
 * Applies a view change. Only a change that morphs a shared cover (an album card into its
 * page, or back) runs inside a View Transition: it snapshots the whole window, freezing the
 * spinning disc and the live glass for its duration. Every other change swaps the view
 * directly and the content animates in by itself, so the sidebar and player stay live.
 */
function transition(change: () => void, morph: boolean) {
  const start = (document as Document & {
    startViewTransition?: (cb: () => Promise<void>) => { finished: Promise<void> };
  }).startViewTransition;
  if (!morph || !start || reducedMotion()) {
    change();
    tick().then(focusHeading);
    return;
  }
  viewTransitionRunning = true;
  const running = start.call(document, async () => {
    change();
    await tick();
    focusHeading();
  });
  running.finished.finally(() => {
    viewTransitionRunning = false;
  });
}

/**
 * Moves focus to the new view's heading: the clicked control is gone, and focus left on the
 * page body would drop screen-reader users at the top of the document.
 */
function focusHeading() {
  document.querySelector<HTMLElement>("[data-view-heading]")?.focus({ preventScroll: true });
}

/** Narrowest window that docks the inspector; below it the inspector floats over the page. */
const INSPECTOR_DOCK_MIN_PX = 1280;
/** Widest window that always shows the sidebar as an icon rail. */
const SIDEBAR_RAIL_MAX_PX = 1099;
const SIDEBAR_KEY = "sonder.sidebarCollapsed";

function readSidebarCollapsed(): boolean {
  try {
    return localStorage.getItem(SIDEBAR_KEY) === "1";
  } catch {
    return false;
  }
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
  signalPath: null,
  repeat: "off",
};

class AppState {
  view = $state<View>({ kind: "home" });
  history = $state<View[]>([]);
  player = $state<PlayerSnapshot>(emptySnapshot);
  queue = $state<QueueItem[]>([]);
  queueOpen = $state(false);
  /**
   * Lyrics are open: on Now Playing in place of the title and badges, on every other page in
   * the inspector column.
   */
  lyricsOpen = $state(false);
  /** The window has room to dock the inspector beside the page instead of over it. */
  wide = $state(false);
  /** The window is too narrow for the full sidebar; it shows as an icon rail regardless. */
  narrow = $state(false);
  /** The user folded the sidebar into its icon rail; remembered on this machine. */
  sidebarCollapsed = $state(readSidebarCollapsed());
  /** The sidebar shows as an icon rail. */
  rail = $derived(this.sidebarCollapsed || this.narrow);
  /** What the right-hand inspector shows: one panel at a time, none on Now Playing's lyrics. */
  inspector = $derived<"queue" | "lyrics" | null>(
    this.queueOpen ? "queue" : this.lyricsOpen && this.view.kind !== "now" ? "lyrics" : null,
  );
  /**
   * The inspector sits beside the page, which narrows to make room for it. Now Playing keeps
   * its full width for the stage, so its queue stays a card over it.
   */
  docked = $derived(this.inspector != null && this.wide && this.view.kind !== "now");
  /** Now Playing shows the engine's stage-by-stage signal path under the transport. */
  signalPathOpen = $state(false);
  settings = $state<Settings>({ albumSort: "artist" });
  scan = $state<ScanState>({ phase: "idle", done: 0, total: 0, last: null, failures: [] });
  loudness = $state<LoudnessState>({ running: false, done: 0, total: 0, pending: null, last: null });
  /** The user started the current analysis from Settings, so even an empty result is news. */
  private loudnessAsked = false;
  notices = $state<Notice[]>([]);
  searchQuery = $state("");
  /** Bumped whenever the library index changes; views reload when it moves. */
  libraryVersion = $state(0);
  /** Every playlist, for the Playlists page and the row menus' "Add to playlist". */
  playlists = $state<Playlist[]>([]);
  /** Bumped after every playlist change made here; playlist pages reload when it moves. */
  playlistsVersion = $state(0);
  /** The question the modal dialog is showing, if any. */
  dialog = $state<DialogRequest | null>(null);

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

  /** The record in flight between Now Playing and the player pill, if any. */
  flight = $state<DiscFlight | null>(null);

  /**
   * Before a view change that opens or leaves Now Playing: where the record is now. Returns
   * nothing when there is no record to fly (nothing playing) or motion is reduced.
   */
  private flightStart(next: View) {
    if (reducedMotion() || !this.current) return null;
    const leaving = this.view.kind === "now" && next.kind !== "now";
    const entering = this.view.kind !== "now" && next.kind === "now";
    if (!leaving && !entering) return null;
    const el = document.querySelector<HTMLElement>(leaving ? '[data-flight="stage"]' : '[data-flight="pill"]');
    if (!el) return null;
    const square = (this.settings.coverStyle ?? "vinyl") === "square";
    return {
      from: el.getBoundingClientRect(),
      target: leaving ? ("pill" as const) : ("stage" as const),
      fromRound: leaving ? !square : true,
      toRound: leaving ? true : !square,
    };
  }

  /** After the change: finds where the record lands and launches the flight. */
  private flightLaunch(start: ReturnType<AppState["flightStart"]>) {
    if (!start) return;
    tick().then(() => {
      const el = document.querySelector<HTMLElement>(`[data-flight="${start.target}"]`);
      if (!el) return;
      this.flight = { ...start, to: settledRect(el), art: this.current?.art ?? null };
    });
  }

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
    // The card the user clicked set morphAlbumId; its cover morphs into the album page.
    const morph = view.kind === "album" && view.id === this.morphAlbumId;
    const flight = this.flightStart(view);
    transition(() => {
      this.history.push(this.view);
      if (this.history.length > 50) this.history.shift();
      this.view = view;
    }, morph);
    this.flightLaunch(flight);
  }

  back() {
    const prev = this.history.at(-1);
    if (!prev) return;
    // Leaving the album page the cover morphed into: it morphs back into its card.
    const morph = this.view.kind === "album" && this.view.id === this.morphAlbumId;
    const flight = this.flightStart(prev);
    transition(() => {
      this.history.pop();
      this.view = prev;
    }, morph);
    this.flightLaunch(flight);
  }

  /** Folds the sidebar into its icon rail or opens it out, and remembers the choice. */
  toggleSidebar() {
    this.sidebarCollapsed = !this.sidebarCollapsed;
    try {
      localStorage.setItem(SIDEBAR_KEY, this.sidebarCollapsed ? "1" : "0");
    } catch {
      // storage blocked: the choice lasts for this session only
    }
  }

  /** Opens or closes the queue; outside Now Playing it takes the inspector from the lyrics. */
  toggleQueue() {
    const open = !this.queueOpen;
    if (open && this.view.kind !== "now") this.lyricsOpen = false;
    this.queueOpen = open;
  }

  /** Opens or closes the lyrics; outside Now Playing they take the inspector from the queue. */
  toggleLyrics() {
    const open = !this.lyricsOpen;
    if (open && this.view.kind !== "now") this.queueOpen = false;
    this.lyricsOpen = open;
  }

  notify(level: Notice["level"], message: string) {
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

  /**
   * Asks a question in the modal dialog. Resolves to the field's trimmed text (or "" without
   * a field) when confirmed, and to `null` when cancelled.
   */
  ask(request: Omit<DialogRequest, "resolve">): Promise<string | null> {
    this.dialog?.resolve(null);
    return new Promise((resolve) => {
      this.dialog = { ...request, resolve };
    });
  }

  answer(value: string | null) {
    const open = this.dialog;
    this.dialog = null;
    open?.resolve(value);
  }

  async refreshPlaylists() {
    const list = await this.run(commands.libraryPlaylists());
    if (list) this.playlists = list;
  }

  /** Reloads everything that shows playlists after a change. */
  playlistsChanged() {
    this.playlistsVersion++;
    return this.refreshPlaylists();
  }

  /** Asks for a name and creates a playlist; returns it, or nothing when cancelled. */
  async createPlaylist(): Promise<Playlist | undefined> {
    const name = await this.ask({ title: "New playlist", input: "", confirm: "Create" });
    if (!name) return undefined;
    const created = await this.run(commands.playlistCreate(name));
    if (created) await this.playlistsChanged();
    return created;
  }

  async addToPlaylist(playlist: Pick<Playlist, "id" | "name">, trackIds: number[]) {
    const added = await this.run(commands.playlistAddTracks(playlist.id, trackIds));
    if (added == null) return;
    await this.playlistsChanged();
    this.notify("info", added === 1 ? `Added to ${playlist.name}` : `${added} tracks added to ${playlist.name}`);
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
    const roomy = window.matchMedia(`(min-width: ${INSPECTOR_DOCK_MIN_PX}px)`);
    this.wide = roomy.matches;
    roomy.addEventListener("change", (e) => (this.wide = e.matches));
    const cramped = window.matchMedia(`(max-width: ${SIDEBAR_RAIL_MAX_PX}px)`);
    this.narrow = cramped.matches;
    cramped.addEventListener("change", (e) => (this.narrow = e.matches));
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
      events.loudnessProgressEvent.listen((e) => {
        this.loudness = { ...this.loudness, running: true, done: e.payload.done, total: e.payload.total };
      }),
      events.loudnessFinishedEvent.listen((e) => {
        const r = e.payload;
        const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
        const didWork = r.tracks || r.albums || r.failed;
        // The backend already raised a notice for a failed pass. Scans and folder changes
        // queue passes too: one that found nothing keeps the result the user last saw,
        // unless the user asked for it.
        const last =
          r.error ??
          (didWork
            ? `${plural(r.tracks, "track")} measured, ${plural(r.albums, "album")} updated` +
              (r.failed ? `, ${r.failed} could not be read` : "")
            : this.loudnessAsked
              ? "Nothing to measure"
              : this.loudness.last);
        this.loudnessAsked = false;
        this.loudness = { running: false, done: 0, total: 0, pending: r.error ? this.loudness.pending : r.pending, last };
      }),
    ]);
    const [player, queue, settings, scanning, analyzing, pending] = await Promise.all([
      commands.playerSnapshot(),
      commands.queueItems(),
      commands.settingsGet(),
      commands.libraryIsScanning(),
      commands.libraryIsAnalyzing(),
      // The only start-up call that reads the database: a failure leaves the count unknown
      // instead of failing the whole start-up.
      commands.libraryLoudnessPending().catch(() => null),
    ]);
    this.player = player;
    this.queue = queue;
    this.settings = settings;
    if (scanning && this.scan.phase === "idle") this.scan = { ...this.scan, phase: "walking" };
    this.loudness = { ...this.loudness, running: this.loudness.running || analyzing, pending };
    // Not part of start-up: a failure only leaves the row menus without playlists.
    void this.refreshPlaylists();
  }

  /** Starts (or queues) a loudness analysis pass; its events update `loudness`. */
  async analyzeLoudness() {
    this.loudnessAsked = true;
    this.loudness = { ...this.loudness, running: true, last: null };
    await this.run(commands.libraryAnalyzeLoudness());
  }

  async saveSettings(next: Settings) {
    const saved = await this.run(commands.settingsSet(next));
    if (saved) this.settings = saved;
  }
}

export const app = new AppState();
