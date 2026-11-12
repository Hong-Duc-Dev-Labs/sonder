<script lang="ts" module>
  export type PopoverPlacement = "top" | "bottom";
</script>

<script lang="ts">
  // A small glass panel anchored to a trigger: row menus and the format status. Closes on
  // Escape, on a click outside and when its anchor scrolls away; Escape returns focus to the
  // anchor. With `menu`, the arrow keys move between the menu items.
  import { tick, type Snippet } from "svelte";
  import { reducedMotion } from "../lib/motion";

  let {
    anchor,
    open = $bindable(false),
    placement = "bottom",
    menu = false,
    label,
    children,
  }: {
    anchor: HTMLElement | undefined;
    open: boolean;
    placement?: PopoverPlacement;
    menu?: boolean;
    label: string;
    children: Snippet;
  } = $props();

  let panel: HTMLDivElement | undefined = $state();
  let pos = $state({ left: 0, top: 0, above: false });

  const GAP = 8;
  const MARGIN = 12;

  function place() {
    if (!anchor || !panel) return;
    const a = anchor.getBoundingClientRect();
    const p = panel.getBoundingClientRect();
    const roomBelow = window.innerHeight - a.bottom - MARGIN;
    const above = placement === "top" ? a.top - MARGIN > p.height || roomBelow < p.height : roomBelow < p.height;
    const left = Math.min(Math.max(MARGIN, a.right - p.width), window.innerWidth - p.width - MARGIN);
    const top = above ? a.top - GAP - p.height : a.bottom + GAP;
    pos = { left, top, above };
  }

  function items(): HTMLElement[] {
    return panel ? [...panel.querySelectorAll<HTMLElement>('[role="menuitem"]:not([disabled])')] : [];
  }

  function close(returnFocus: boolean) {
    open = false;
    if (returnFocus) anchor?.focus();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close(true);
      return;
    }
    if (!menu) return;
    const list = items();
    const i = list.indexOf(document.activeElement as HTMLElement);
    const moves: Record<string, number> = { ArrowDown: 1, ArrowUp: -1 };
    if (e.key in moves) {
      e.preventDefault();
      list[(i + moves[e.key] + list.length) % list.length]?.focus();
    } else if (e.key === "Home" || e.key === "End") {
      e.preventDefault();
      (e.key === "Home" ? list[0] : list.at(-1))?.focus();
    } else if (e.key === "Tab") {
      close(false);
    }
  }

  function onOutside(e: PointerEvent) {
    const target = e.target as Node;
    if (panel?.contains(target) || anchor?.contains(target)) return;
    close(false);
  }

  $effect(() => {
    if (!open) return;
    tick().then(() => {
      place();
      (menu ? items()[0] : panel)?.focus();
    });
    const onScroll = () => close(false);
    window.addEventListener("pointerdown", onOutside, true);
    window.addEventListener("resize", onScroll);
    document.addEventListener("scroll", onScroll, true);
    return () => {
      window.removeEventListener("pointerdown", onOutside, true);
      window.removeEventListener("resize", onScroll);
      document.removeEventListener("scroll", onScroll, true);
    };
  });

  function pop(_node: Element) {
    if (reducedMotion()) return { duration: 1 };
    return {
      duration: 180,
      css: (t: number) =>
        `opacity: ${t}; transform: translateY(${(1 - t) * (pos.above ? 6 : -6)}px) scale(${0.97 + 0.03 * t})`,
    };
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    bind:this={panel}
    class="popover glass"
    class:above={pos.above}
    role={menu ? "menu" : "dialog"}
    aria-label={label}
    tabindex="-1"
    style:left="{pos.left}px"
    style:top="{pos.top}px"
    onkeydown={onKey}
    transition:pop
  >
    {@render children()}
  </div>
{/if}

<style>
  .popover {
    position: fixed;
    z-index: 50;
    min-width: 220px;
    max-width: 360px;
    padding: 6px;
    border-radius: var(--r-md);
    transform-origin: top right;
    outline: none;
  }
  .popover.above {
    transform-origin: bottom right;
  }
  .popover :global([role="menuitem"]) {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 40px;
    padding: 0 12px;
    border-radius: var(--r-sm);
    color: var(--ink);
    font-weight: 550;
    text-align: left;
  }
  .popover :global([role="menuitem"] svg) {
    color: var(--ink-2);
  }
  .popover :global([role="menuitem"]:hover),
  .popover :global([role="menuitem"]:focus-visible) {
    background: var(--soft-hover);
    outline: none;
    box-shadow: none;
  }
</style>
