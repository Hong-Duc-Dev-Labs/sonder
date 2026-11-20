import { describe, expect, it } from "vitest";
import { AppState } from "./app.svelte";

/** A shell on a window wide enough to dock, starting on Now Playing. */
function shell(): AppState {
  const app = new AppState();
  app.wide = true;
  app.view = { kind: "now" };
  return app;
}

describe("the inspector", () => {
  it("holds one panel at a time, so no toggle claims a panel that is not showing", () => {
    const app = shell();
    app.toggleLyrics();
    expect([app.queueOpen, app.lyricsOpen]).toEqual([false, true]);
    app.toggleQueue();
    expect([app.queueOpen, app.lyricsOpen]).toEqual([true, false]);
    app.toggleLyrics();
    expect([app.queueOpen, app.lyricsOpen]).toEqual([false, true]);
  });

  it("keeps lyrics in the page on Now Playing and docks them beside a page", () => {
    const app = shell();
    app.toggleLyrics();
    expect(app.inspector).toBe(null);
    expect(app.docked).toBe(false);

    app.view = { kind: "albums" };
    expect(app.inspector).toBe("lyrics");
    expect(app.docked).toBe(true);

    app.wide = false;
    expect(app.docked).toBe(false);
  });

  it("shows the queue as the inspector only when it is open", () => {
    const app = shell();
    app.view = { kind: "album", id: 1 };
    expect(app.inspector).toBe(null);
    app.toggleQueue();
    expect(app.inspector).toBe("queue");
    expect(app.docked).toBe(true);
  });

  it("keeps the queue an overlay on Now Playing", () => {
    const app = shell();
    app.toggleQueue();
    expect(app.inspector).toBe("queue");
    expect(app.docked).toBe(false);
    // The lyrics toggle closes it, so the pressed state and the panel agree.
    app.toggleLyrics();
    expect(app.inspector).toBe(null);
    expect(app.docked).toBe(false);
  });

  it("resets signalPathOpen when navigating away from Now Playing", () => {
    const app = shell();
    app.signalPathOpen = true;
    app.go({ kind: "albums" });
    expect(app.signalPathOpen).toBe(false);

    app.go({ kind: "now" });
    app.signalPathOpen = true;
    app.back();
    expect(app.signalPathOpen).toBe(false);
  });
});
