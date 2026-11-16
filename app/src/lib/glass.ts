// Glass blur configuration and hardware acceleration capabilities.
// Tuned for high visual fidelity on modern GPUs with graceful fallbacks.

export const GLASS_CONFIG = {
  full: {
    sidebarBlur: 24,
    dockBlur: 16,
    tooltipBlur: 12,
    modalBlur: 20,
  },
  reduced: {
    sidebarBlur: 12,
    dockBlur: 8,
    tooltipBlur: 6,
    modalBlur: 10,
  },
} as const;

let supportsBackdropFilter: boolean | null = null;

export function hasBackdropFilterSupport(): boolean {
  if (supportsBackdropFilter !== null) return supportsBackdropFilter;
  if (typeof window === "undefined" || typeof CSS === "undefined") {
    supportsBackdropFilter = false;
    return false;
  }
  supportsBackdropFilter =
    CSS.supports("backdrop-filter", "blur(10px)") ||
    CSS.supports("-webkit-backdrop-filter", "blur(10px)");
  return supportsBackdropFilter;
}
