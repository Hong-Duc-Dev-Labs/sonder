<script lang="ts">
  // The app's one modal dialog: a confirmation or a single text field, as `app.ask` requests.
  // A native <dialog> keeps focus inside, closes on Escape and returns focus when it closes.
  import { app } from "../lib/app.svelte";

  let el: HTMLDialogElement | undefined = $state();
  let value = $state("");
  let request = $derived(app.dialog);
  /** The request the field was last filled for; a new one (even replacing an open one) refills it. */
  let shown: typeof request = null;

  $effect(() => {
    if (!el) return;
    if (request && request !== shown) {
      shown = request;
      value = request.input ?? "";
      if (!el.open) el.showModal();
    } else if (!request) {
      shown = null;
      if (el.open) el.close();
    }
  });

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!request) return;
    if (request.input == null) app.answer("");
    else if (value.trim()) app.answer(value.trim());
  }
</script>

<!-- Escape fires `cancel`, then `close`; both settle the request as cancelled. -->
<dialog bind:this={el} class="glass" aria-labelledby="dialog-title" onclose={() => app.answer(null)}>
  {#if request}
    <form onsubmit={submit}>
      <h2 id="dialog-title">{request.title}</h2>
      {#if request.message}<p class="dim">{request.message}</p>{/if}
      {#if request.input != null}
        <!-- svelte-ignore a11y_autofocus -->
        <input bind:value aria-label={request.title} maxlength="200" autofocus />
      {/if}
      <div class="actions">
        <button type="button" class="btn" onclick={() => app.answer(null)}>Cancel</button>
        <button
          type="submit"
          class="btn primary"
          class:danger={request.danger}
          disabled={request.input != null && !value.trim()}
        >
          {request.confirm}
        </button>
      </div>
    </form>
  {/if}
</dialog>

<style>
  dialog {
    width: min(420px, calc(100vw - 32px));
    padding: 24px;
    border: none;
    border-radius: var(--r-lg);
    color: var(--ink);
  }
  dialog::backdrop {
    background: rgb(0 0 0 / 0.5);
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  h2 {
    font-size: var(--fs-title);
  }
  p {
    margin: 0;
    line-height: 1.5;
  }
  input {
    width: 100%;
    height: 44px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 6px;
  }
  .btn.primary.danger {
    background: var(--error);
  }
</style>
