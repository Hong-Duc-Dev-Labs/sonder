import { describe, expect, it } from "vitest";
import { keyShowsFocus, trackModality } from "./modality";

function key(k: string): Event {
  return Object.assign(new Event("keydown"), { key: k });
}

describe("trackModality", () => {
  it("switches to pointer on a press and back to keyboard on a navigation key", () => {
    const target = new EventTarget();
    const root = { dataset: {} as DOMStringMap };
    trackModality(target, root);

    target.dispatchEvent(new Event("pointerdown"));
    expect(root.dataset.modality).toBe("pointer");

    // typing into a field the user clicked keeps the ring hidden
    target.dispatchEvent(key("a"));
    expect(root.dataset.modality).toBe("pointer");

    // nudging a clicked slider with the arrows shows where focus is
    target.dispatchEvent(key("ArrowRight"));
    expect(root.dataset.modality).toBe("keyboard");

    target.dispatchEvent(new Event("pointerdown"));
    target.dispatchEvent(key("Tab"));
    expect(root.dataset.modality).toBe("keyboard");
  });

  it("stops tracking once disposed", () => {
    const target = new EventTarget();
    const root = { dataset: {} as DOMStringMap };
    const stop = trackModality(target, root);
    stop();
    target.dispatchEvent(new Event("pointerdown"));
    expect(root.dataset.modality).toBeUndefined();
  });

  it("counts only keys that move focus or operate a control", () => {
    expect(keyShowsFocus("Tab")).toBe(true);
    expect(keyShowsFocus("PageDown")).toBe(true);
    expect(keyShowsFocus("Enter")).toBe(false);
    expect(keyShowsFocus("Shift")).toBe(false);
  });
});
