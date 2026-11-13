// Kept as a component so Now Playing reads as one thought at a time: the panel is the
// Roon-style "signal path" list, source at the top, device at the bottom, every stage in
// between named by the engine that ran it.
<script lang="ts">
  import type { SignalPathInfo } from "../lib/bindings";
  import { rate } from "../lib/format";

  let { path }: { path: SignalPathInfo } = $props();

  let outputMode = $derived(
    path.mode === "dop" ? "DoP" : path.mode === "nativeDsd" ? "Native DSD" : "PCM",
  );
  let source = $derived(`${path.source.bits}-bit / ${rate(path.source.rate)}`);
  let output = $derived(`${path.outputBits}-bit / ${rate(path.outputRate)}`);
</script>

<section class="sp" aria-label="Signal path">
  <header>
    <h3>Signal path</h3>
    <span class:ok={path.bitPerfect} class:warn={!path.bitPerfect}>
      {path.bitPerfect ? "Bit-perfect" : "DSP active"}
    </span>
  </header>
  <ol>
    <li>
      <span class="name">Source</span>
      <span class="detail">{source} · {path.source.channels} ch</span>
    </li>
    {#each path.stages as stage (stage.name + stage.detail)}
      <li>
        <span class="name">{stage.name}</span>
        <span class="detail">{stage.detail}</span>
      </li>
    {/each}
    <li>
      <span class="name">Output</span>
      <span class="detail">{outputMode} {output} · {path.outputChannels} ch</span>
    </li>
    {#if path.transition}
      <li>
        <span class="name">Transition</span>
        <span class="detail">{path.transition}</span>
      </li>
    {/if}
  </ol>
</section>

<style>
  .sp {
    border-radius: 12px;
    background: var(--soft);
    border: 1px solid var(--hairline);
    padding: 10px 12px;
    font-size: 12px;
    text-align: left;
    max-height: 240px;
    overflow: auto;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 10px;
    margin-bottom: 6px;
  }
  h3 {
    margin: 0;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.02em;
  }
  .ok {
    color: var(--ok);
  }
  .warn {
    color: var(--warn);
  }
  ol {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  li {
    display: grid;
    grid-template-columns: minmax(74px, auto) 1fr;
    gap: 10px;
    align-items: baseline;
  }
  .name {
    color: var(--ink-2);
    font-weight: 600;
  }
  .detail {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--ink);
    overflow-wrap: anywhere;
  }
</style>
