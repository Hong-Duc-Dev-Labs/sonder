// Dragging a track row onto a playlist in the sidebar. Pointer-driven rather than HTML5
// drag-and-drop, which the desktop webview reserves for files dropped from outside.
// Drop targets mark themselves with `data-drop-playlist="<id>"`.
import { app } from "./app.svelte";
import type { TrackItem } from "./bindings";

/** How far the pointer travels before a press on a row becomes a drag. */
const THRESHOLD_PX = 6;

type Pending = { track: TrackItem; x: number; y: number; pointerId: number; el: HTMLElement };

class TrackDrag {
  /** The track being dragged and where the pointer is; null when no drag is under way. */
  active = $state<{ track: TrackItem; x: number; y: number } | null>(null);
  /** The playlist under the pointer, when there is one. */
  over = $state<number | null>(null);
  private pending: Pending | null = null;

  /** A left press on a row: may become a drag once the pointer moves far enough. */
  press(e: PointerEvent, track: TrackItem) {
    if (e.button !== 0 || e.pointerType === "touch") return;
    this.pending = { track, x: e.clientX, y: e.clientY, pointerId: e.pointerId, el: e.currentTarget as HTMLElement };
  }

  move(e: PointerEvent) {
    const p = this.pending;
    if (!p || e.pointerId !== p.pointerId) return;
    // A press released outside the row never reached its pointerup.
    if (!(e.buttons & 1)) return this.cancel();
    if (!this.active) {
      if (Math.hypot(e.clientX - p.x, e.clientY - p.y) < THRESHOLD_PX) return;
      p.el.setPointerCapture(p.pointerId);
      window.getSelection()?.removeAllRanges();
    }
    this.active = { track: p.track, x: e.clientX, y: e.clientY };
    const target = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>("[data-drop-playlist]");
    this.over = target ? Number(target.dataset.dropPlaylist) : null;
  }

  async release() {
    const track = this.active?.track;
    const over = this.over;
    this.cancel();
    if (track == null || over == null) return;
    const playlist = app.playlists.find((p) => p.id === over);
    if (playlist) await app.addToPlaylist(playlist, [track.id]);
  }

  cancel() {
    this.pending = null;
    this.active = null;
    this.over = null;
  }
}

export const trackDrag = new TrackDrag();
