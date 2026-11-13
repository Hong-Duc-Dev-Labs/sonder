// Which device the user last moved through the UI with. Focus rings belong to keyboard use:
// the webview counts a click on an <input> (a slider, a text field) as "focus visible", and
// :focus-within outlines cannot tell a click from a Tab at all. The stylesheet hides rings
// while `data-modality="pointer"` is set on the root and shows them otherwise.

export type Modality = "pointer" | "keyboard";

/** Keys that move focus or operate a focused control; typing text does not count. */
const NAVIGATION_KEYS = new Set([
  "Tab",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Home",
  "End",
  "PageUp",
  "PageDown",
]);

export function keyShowsFocus(key: string): boolean {
  return NAVIGATION_KEYS.has(key);
}

/**
 * Keeps `root.dataset.modality` on the last input device. Listens in the capture phase so a
 * control that stops propagation still counts. Returns a function that stops tracking.
 */
export function trackModality(target: EventTarget, root: { dataset: DOMStringMap }): () => void {
  const set = (m: Modality) => {
    if (root.dataset.modality !== m) root.dataset.modality = m;
  };
  const onPointer = () => set("pointer");
  const onKey = (e: Event) => {
    if (keyShowsFocus((e as KeyboardEvent).key)) set("keyboard");
  };
  const capture = { capture: true };
  target.addEventListener("pointerdown", onPointer, capture);
  target.addEventListener("keydown", onKey, capture);
  return () => {
    target.removeEventListener("pointerdown", onPointer, capture);
    target.removeEventListener("keydown", onKey, capture);
  };
}
