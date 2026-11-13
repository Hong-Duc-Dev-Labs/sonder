// Keeps a `.range` slider's `--fill` (the lit share of its track, as a percentage) in step
// with its value, both while it is dragged and when the value is set from outside. Pass the
// bound value as the parameter so outside changes repaint it.

/** The lit share of the track for `value` between `min` and `max`, as a CSS percentage. */
export function fillPercent(value: number, min: number, max: number): string {
  if (!(max > min)) return "0%";
  const f = Math.min(1, Math.max(0, (value - min) / (max - min)));
  return `${(f * 100).toFixed(2)}%`;
}

export function rangeFill(node: HTMLInputElement, _value?: unknown) {
  const paint = () => {
    node.style.setProperty(
      "--fill",
      fillPercent(Number(node.value), Number(node.min || 0), Number(node.max || 100)),
    );
  };
  paint();
  node.addEventListener("input", paint);
  return {
    // runs after Svelte has written the new value to the element
    update: () => queueMicrotask(paint),
    destroy: () => node.removeEventListener("input", paint),
  };
}
