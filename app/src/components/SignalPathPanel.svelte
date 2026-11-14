<script lang="ts">
  // The Roon-style signal path as a chain read top to bottom: the source file, every stage the
  // engine ran (named by the engine itself), then the device. Source and device are solid
  // nodes, stages hollow ones, joined by one line. The verdict up top says whether the device
  // receives the decoded samples unchanged, in words as well as colour.
  import type { SignalPathInfo } from "../lib/bindings";
  import { rate } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import Icon from "./Icon.svelte";

  let { path }: { path: SignalPathInfo } = $props();

  let outputMode = $derived(
    path.mode === "dop" ? "DoP" : path.mode === "nativeDsd" ? t("path.nativeDsd") : "PCM",
  );
  let source = $derived(`${path.source.bits}-bit / ${rate(path.source.rate)} · ${path.source.channels} ch`);
  let output = $derived(`${outputMode} ${path.outputBits}-bit / ${rate(path.outputRate)} · ${path.outputChannels} ch`);
</script>

<section class="sp" aria-label={t("common.signalPath")}>
  <header>
    <h3>{t("common.signalPath")}</h3>
    <span class="verdict" class:ok={path.bitPerfect} class:warn={!path.bitPerfect}>
      {#if path.bitPerfect}<Icon name="check" size={14} />{/if}
      {path.bitPerfect ? t("common.bitPerfect") : t("path.processed")}
    </span>
  </header>

  <ol class="chain">
    <li class="node end">
      <span class="name">{t("path.source")}</span>
      <span class="detail">{source}</span>
    </li>
    {#each path.stages as stage (stage.name + stage.detail)}
      <li class="node">
        <span class="name">{stage.name}</span>
        <span class="detail">{stage.detail}</span>
      </li>
    {:else}
      <li class="direct">{t("path.noProcessing")}</li>
    {/each}
    <li class="node end">
      <span class="name">{t("path.output")}</span>
      <span class="detail">{output}</span>
    </li>
  </ol>

  {#if path.transition}
    <p class="transition"><span class="name">{t("path.transition")}</span> {path.transition}</p>
  {/if}
</section>

<style>
  .sp {
    padding: 14px 16px 12px;
    border-radius: var(--r-md);
    background: var(--soft);
    box-shadow: inset 0 0 0 1px var(--hairline);
    text-align: left;
    max-height: 280px;
    overflow: auto;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    margin-bottom: 12px;
  }
  h3 {
    margin: 0;
    font-size: var(--fs-label);
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ink-2);
  }
  .verdict {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 10px;
    border-radius: var(--r-pill);
    font-size: var(--fs-label);
    font-weight: 700;
  }
  .verdict.ok {
    color: var(--ok);
    background: color-mix(in srgb, var(--ok) 14%, transparent);
  }
  .verdict.warn {
    color: var(--warn);
    background: color-mix(in srgb, var(--warn) 14%, transparent);
  }

  /* One line runs down the left; each step hangs a dot on it. */
  .chain {
    position: relative;
    margin: 0;
    padding: 0 0 0 22px;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .chain::before {
    content: "";
    position: absolute;
    left: 5px;
    top: 8px;
    bottom: 8px;
    width: 2px;
    border-radius: 1px;
    background: rgb(255 255 255 / 0.14);
  }
  .node {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .node::before {
    content: "";
    position: absolute;
    left: -21px;
    top: 4px;
    width: 10px;
    height: 10px;
    box-sizing: border-box;
    border-radius: 50%;
    border: 2px solid var(--ink-2);
    background: #141417;
  }
  .node.end::before {
    border-color: var(--ink);
    background: var(--ink);
  }
  .name {
    font-size: var(--fs-small);
    font-weight: 650;
    color: var(--ink);
  }
  .node:not(.end) .name {
    color: var(--ink-2);
  }
  .detail {
    font-family: var(--font);
    font-size: 11.5px;
    color: var(--ink-2);
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }
  .direct {
    font-size: var(--fs-label);
    font-style: italic;
    color: var(--ink-3);
  }
  .transition {
    margin: 12px 0 0;
    padding-top: 10px;
    border-top: 1px solid var(--hairline);
    font-size: var(--fs-small);
    color: var(--ink-2);
  }
  .transition .name {
    margin-right: 6px;
  }
</style>
