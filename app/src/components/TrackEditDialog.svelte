<script lang="ts">
  // The metadata editor: the tags stored in the track's own file, read fresh when it opens and
  // written back into that same file on Save, so a rescan or another player sees the edit.
  import { app } from "../lib/app.svelte";
  import { commands, type TrackEdit } from "../lib/bindings";
  import { t } from "../lib/i18n.svelte";
  import { numberField, splitNames } from "../lib/tagEdit";

  let el: HTMLDialogElement | undefined = $state();
  /** The track whose tags are on screen; a different one reloads the fields. */
  let shown: number | null = null;
  /** The file's tags, `null` while they load. */
  let tags = $state<TrackEdit | null>(null);
  let saving = $state(false);
  /** The path of the track being edited, kept while the dialog is open. */
  let path = $state("");

  // Numbers stay text, so an empty box clears the tag instead of reading as zero.
  let title = $state("");
  let artists = $state("");
  let album = $state("");
  let albumArtists = $state("");
  let trackNo = $state("");
  let discNo = $state("");
  let year = $state("");
  let genre = $state("");

  $effect(() => {
    if (!el) return;
    const track = app.editing;
    if (!track) {
      shown = null;
      tags = null;
      if (el.open) el.close();
      return;
    }
    if (!el.open) el.showModal();
    if (shown === track.id) return;
    shown = track.id;
    tags = null;
    saving = false;
    path = track.path;
    void load(track.id);
  });

  async function load(id: number) {
    const found = await app.run(commands.trackTags(id));
    if (shown !== id || !found) return;
    tags = found;
    title = found.title;
    artists = found.artists.join("; ");
    album = found.album;
    albumArtists = found.albumArtists.join("; ");
    trackNo = found.trackNo?.toString() ?? "";
    discNo = found.discNo?.toString() ?? "";
    year = found.year?.toString() ?? "";
    genre = found.genre;
  }

  let canSave = $derived(tags !== null && title.trim().length > 0 && !saving);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const track = app.editing;
    if (!track || !canSave) return;
    saving = true;
    const edit: TrackEdit = {
      title: title.trim(),
      artists: splitNames(artists),
      album: album.trim(),
      albumArtists: splitNames(albumArtists),
      trackNo: numberField(trackNo),
      discNo: numberField(discNo),
      year: numberField(year),
      genre: genre.trim(),
    };
    const saved = await app.run(commands.trackUpdateTags(track.id, edit));
    saving = false;
    // A failure is already a notice with the reason; the editor stays open with the values.
    if (saved !== undefined) app.closeEditor();
  }
</script>

<!-- Escape fires `cancel`, then `close`; both close the editor. -->
<dialog bind:this={el} class="glass" aria-labelledby="tags-title" onclose={() => app.closeEditor()}>
  {#if app.editing}
    <form onsubmit={submit}>
      <h2 id="tags-title">{t("tags.editTitle")}</h2>
      <p class="path dim" title={path}>{path}</p>
      {#if tags}
        <div class="fields">
          <label class="wide">
            {t("tags.title")}
            <!-- svelte-ignore a11y_autofocus -->
            <input bind:value={title} required maxlength="500" autofocus />
          </label>
          <label class="wide">
            {t("tags.artists")}
            <input bind:value={artists} placeholder={t("tags.namesHint")} />
          </label>
          <label>
            {t("tags.album")}
            <input bind:value={album} />
          </label>
          <label>
            {t("tags.albumArtists")}
            <input bind:value={albumArtists} placeholder={t("tags.namesHint")} />
          </label>
          <label>
            {t("tags.trackNo")}
            <input type="number" min="0" max="9999" bind:value={trackNo} />
          </label>
          <label>
            {t("tags.discNo")}
            <input type="number" min="0" max="9999" bind:value={discNo} />
          </label>
          <label>
            {t("tags.year")}
            <input type="number" min="0" max="9999" bind:value={year} />
          </label>
          <label>
            {t("tags.genre")}
            <input bind:value={genre} />
          </label>
        </div>
        <p class="dim hint">{t("tags.inFile")}</p>
        <div class="actions">
          <button type="button" class="btn" disabled={saving} onclick={() => app.closeEditor()}>
            {t("common.cancel")}
          </button>
          <button type="submit" class="btn primary" disabled={!canSave}>
            {saving ? t("tags.saving") : t("tags.save")}
          </button>
        </div>
      {:else}
        <p class="dim">{t("tags.reading")}</p>
      {/if}
    </form>
  {/if}
</dialog>

<style>
  dialog {
    width: min(620px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    overflow-y: auto;
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
  .path {
    margin: 0;
    font-size: var(--fs-small);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 12px;
  }
  .wide {
    grid-column: 1 / -1;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: var(--fs-label);
    font-weight: 600;
    color: var(--ink-2);
  }
  input {
    width: 100%;
  }
  .hint {
    margin: 0;
    font-size: var(--fs-small);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 4px;
  }
</style>
