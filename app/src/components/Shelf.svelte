<script lang="ts">
  // A titled row of cards that scrolls sideways: "See all" opens the full page, the arrow
  // buttons page through the row, and Left/Right move focus between the cards. The row's
  // edges fade out so a cut card reads as "more this way".
  import type { Snippet } from "svelte";
  import { reducedMotion } from "../lib/motion";
  import Icon from "./Icon.svelte";

  let { title, onSeeAll, children }: { title: string; onSeeAll: () => void; children: Snippet } = $props();

  let row: HTMLDivElement;
  let atStart = $state(true);
  let atEnd = $state(true);

  function measure() {
    if (!row) return;
    atStart = row.scrollLeft <= 1;
    atEnd = row.scrollLeft + row.clientWidth >= row.scrollWidth - 1;
  }

  function page(dir: 1 | -1) {
    row.scrollBy({ left: dir * row.clientWidth * 0.85, behavior: reducedMotion() ? "auto" : "smooth" });
  }

  /** The card (a direct child of the row) holding `el`. */
  function cardOf(el: Element | null): HTMLElement | null {
    while (el && el.parentElement !== row) el = el.parentElement;
    return el as HTMLElement | null;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const card = cardOf(e.target as Element);
    const next = (e.key === "ArrowRight" ? card?.nextElementSibling : card?.previousElementSibling) as HTMLElement | null;
    const target = next?.querySelector<HTMLElement>("button, a, [tabindex]");
    if (!target) return;
    e.preventDefault();
    target.focus();
    target.scrollIntoView({ block: "nearest", inline: "nearest", behavior: reducedMotion() ? "auto" : "smooth" });
  }

  $effect(() => {
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(row);
    return () => ro.disconnect();
  });
</script>

<section class="shelf" aria-label={title}>
  <header class="head">
    <h2>{title}</h2>
    <span class="rule" aria-hidden="true"></span>
    {#if !(atStart && atEnd)}
      <button class="icon-btn" aria-label="Scroll {title} back" title="Back" disabled={atStart} onclick={() => page(-1)}>
        <Icon name="chevron" size={18} />
      </button>
      <button class="icon-btn fwd" aria-label="Scroll {title} on" title="More" disabled={atEnd} onclick={() => page(1)}>
        <Icon name="chevron" size={18} />
      </button>
    {/if}
    <button class="chip" onclick={onSeeAll}>See all</button>
  </header>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="row" class:fade-start={!atStart} class:fade-end={!atEnd} bind:this={row} onscroll={measure} onkeydown={onKey}>
    {@render children()}
  </div>
</section>

<style>
  .shelf {
    margin-top: 36px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 14px;
  }
  h2 {
    font-size: var(--fs-title);
    font-weight: 800;
    letter-spacing: -0.01em;
    white-space: nowrap;
  }
  .rule {
    flex: 1;
    height: 1px;
    margin: 0 8px;
    background: var(--hairline);
  }
  /* the chevron points on; turned round for back */
  .icon-btn:not(.fwd) :global(svg) {
    transform: rotate(180deg);
  }
  .icon-btn:disabled {
    opacity: 0.35;
    pointer-events: none;
  }
  .row {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 172px;
    gap: 22px;
    overflow-x: auto;
    scrollbar-width: none;
    scroll-snap-type: x proximity;
    /* snap cards to the padded edge, so the row rests at its very start (no fade there) */
    scroll-padding-inline: 4px;
    /* room for focus rings and the album card's lifting record */
    padding: 6px 4px 10px;
    margin: -6px -4px 0;
    --fade-l: 0px;
    --fade-r: 0px;
    mask-image: linear-gradient(to right, transparent, #000 var(--fade-l), #000 calc(100% - var(--fade-r)), transparent);
  }
  .row::-webkit-scrollbar {
    display: none;
  }
  .row.fade-start {
    --fade-l: 48px;
  }
  .row.fade-end {
    --fade-r: 48px;
  }
  .row > :global(*) {
    scroll-snap-align: start;
    min-width: 0;
  }
</style>
